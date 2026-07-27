use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::PathBuf,
    sync::LazyLock,
};

use bevy::prelude::*;
use crownline_core::{
    Action, AtomicSaveStorage, GuidedAiConfig, GuidedContent, GuidedEventPredicate,
    GuidedPredicate, GuidedPredicateContext, MAX_PERSISTED_BYTES, MatchState, ObjectiveResult,
    SaveEnvelope, SaveReader, write_bytes_atomically,
};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::{
    lifecycle::{ClientFlow, LocalSetup, SeatController},
    local_ai::{AiCancellationEpoch, validate_guided_ai_content},
    panels::{PanelBody, PanelKind, PanelSurface},
    rendering::{
        DisplayedGame, LocalTransitionEventQueue, LocalTransitionNoticeLog, OverlaySelection,
    },
};

const GUIDED_PROGRESS_VERSION: u16 = 1;
const MAX_PROGRESS_ENTRIES: usize = 256;
const MAX_METRIC: u32 = 1_000_000;

static GUIDED_TEXT: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    ron::from_str(include_str!("../assets/guidance/en-US.ron"))
        .expect("bundled guided text catalog is valid RON")
});

#[derive(Resource)]
struct GuidedScenarioCatalog(Vec<crownline_core::ScenarioDefinition>);

impl Default for GuidedScenarioCatalog {
    fn default() -> Self {
        let scenarios = [
            include_str!("../assets/scenarios/guided/guided-movement-capture.ron"),
            include_str!("../assets/scenarios/guided/guided-movement-knight.ron"),
            include_str!("../assets/scenarios/guided/guided-terrain-forest.ron"),
            include_str!("../assets/scenarios/guided/guided-terrain-mountain.ron"),
            include_str!("../assets/scenarios/guided/guided-crossing-bridge.ron"),
            include_str!("../assets/scenarios/guided/guided-crossing-tower-rook.ron"),
            include_str!("../assets/scenarios/guided/guided-movement-open-practice.ron"),
            include_str!("../assets/scenarios/guided/guided-realm-claim.ron"),
            include_str!("../assets/scenarios/guided/guided-realm-governance.ron"),
            include_str!("../assets/scenarios/guided/guided-realm-production.ron"),
            include_str!("../assets/scenarios/guided/guided-realm-transfer.ron"),
            include_str!("../assets/scenarios/guided/guided-realm-transfer-cancel.ron"),
            include_str!("../assets/scenarios/guided/guided-realm-open-practice.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-en-passant.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-promotion-knight.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-promotion-batch.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-answer-check.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-castling.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-checkmate.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-draw.ron"),
            include_str!("../assets/scenarios/guided/guided-royal-open-practice.ron"),
            include_str!("../assets/scenarios/guided/challenge-mate-court.ron"),
            include_str!("../assets/scenarios/guided/challenge-capture-line.ron"),
            include_str!("../assets/scenarios/guided/challenge-terrain-route.ron"),
            include_str!("../assets/scenarios/guided/challenge-settlement-defense.ron"),
            include_str!("../assets/scenarios/guided/challenge-production-deployment.ron"),
            include_str!("../assets/scenarios/guided/challenge-underpromotion.ron"),
            include_str!("../assets/scenarios/guided/challenge-warden-realm.ron"),
        ]
        .into_iter()
        .map(|source| {
            let scenario: crownline_core::ScenarioDefinition =
                ron::from_str(source).expect("bundled guided scenario must parse");
            scenario
                .validate()
                .expect("bundled guided scenario must validate");
            let guided = scenario
                .guided
                .as_ref()
                .expect("guided catalog entries contain guided content");
            for key in std::iter::once(&guided.category_key)
                .chain(
                    guided
                        .completion
                        .iter()
                        .map(|completion| &completion.completion_key),
                )
                .chain(guided.stages.iter().flat_map(|stage| {
                    std::iter::once(&stage.title_key)
                        .chain(std::iter::once(&stage.explanation_key))
                        .chain(stage.hint_keys.iter())
                }))
            {
                assert!(
                    GUIDED_TEXT.contains_key(key),
                    "missing guided text key {key:?}"
                );
            }
            scenario
        })
        .collect();
        Self(scenarios)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct GuidedMetrics {
    completed: bool,
    attempts: u16,
    retries: u16,
    hints_revealed: u16,
    best_actions: Option<u16>,
    best_turns: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuidedResume {
    guided_id: String,
    scenario_ron: String,
    state: SaveEnvelope,
    stage_start: MatchState,
    stage_index: usize,
    completed_stage_ids: Vec<String>,
    hint_reveals: Vec<u8>,
    actions_taken: u16,
    turns_elapsed: u16,
    #[serde(default)]
    total_actions: u16,
    #[serde(default)]
    total_turns: u16,
    #[serde(default)]
    ai_actions_taken: u16,
    retries: u16,
    failed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuidedProgressDocument {
    format_version: u16,
    entries: BTreeMap<String, GuidedMetrics>,
    resume: Option<GuidedResume>,
}

impl Default for GuidedProgressDocument {
    fn default() -> Self {
        Self {
            format_version: GUIDED_PROGRESS_VERSION,
            entries: BTreeMap::new(),
            resume: None,
        }
    }
}

#[derive(Debug, Clone)]
struct GuidedSession {
    guided_id: String,
    scenario_index: usize,
    stage_index: usize,
    completed_stage_ids: Vec<String>,
    hint_reveals: Vec<u8>,
    actions_taken: u16,
    turns_elapsed: u16,
    total_actions: u16,
    total_turns: u16,
    ai_actions_taken: u16,
    retries: u16,
    failed: bool,
    complete: bool,
    stage_start: MatchState,
    last_turn_number: u64,
}

#[derive(Resource, Default)]
pub(crate) struct GuidedRuntime {
    browser_open: bool,
    selected: usize,
    session: Option<GuidedSession>,
    progress: GuidedProgressDocument,
    message: String,
    reset_armed: Option<String>,
}

impl GuidedRuntime {
    pub(crate) const fn browser_open(&self) -> bool {
        self.browser_open
    }

    pub(crate) const fn is_active(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn ai_configuration(&self, game: &DisplayedGame) -> Option<(GuidedAiConfig, u16)> {
        let session = self.session.as_ref()?;
        if session.failed || session.complete {
            return None;
        }
        let guided = game.scenario.guided.as_ref()?;
        (session.guided_id == guided.id).then(|| {
            guided
                .ai
                .clone()
                .map(|config| (config, session.ai_actions_taken))
        })?
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Component)]
pub(crate) enum GuidedControl {
    Open,
    Previous,
    Next,
    Start,
    Resume,
    Reset,
    Close,
    Hint,
    Retry,
    Leave,
    Replay,
}

#[derive(Component)]
struct GuidedBrowserRoot;
#[derive(Component)]
struct GuidedBrowserText;
#[derive(Component)]
struct GuidedOpenButton;
#[derive(Component)]
struct GuidedObjectiveRoot;
#[derive(Component)]
struct GuidedObjectiveText;

pub struct GuidedPlayPlugin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub(crate) struct GuidedInputSet;

impl Plugin for GuidedPlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GuidedRuntime>()
            .init_resource::<GuidedScenarioCatalog>()
            .add_systems(
                Startup,
                (load_guided_progress, spawn_guided_browser).chain(),
            )
            .add_systems(PostStartup, attach_guided_objective_surface)
            .add_systems(
                Update,
                (handle_guided_controls, sync_guided_ui)
                    .chain()
                    .after(crate::lifecycle::LifecycleInputSet)
                    .in_set(GuidedInputSet),
            )
            .add_systems(PostUpdate, process_guided_transitions);
    }
}

pub(crate) fn open_guided_button() -> impl Bundle {
    (
        Button,
        Node {
            min_width: px(190),
            min_height: px(36),
            padding: UiRect::axes(px(10), px(6)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.13, 0.24, 0.3)),
        Interaction::default(),
        GuidedControl::Open,
        GuidedOpenButton,
        children![(
            Text::new("Guided scenarios [G]"),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::srgb(0.78, 0.95, 0.96)),
        )],
    )
}

fn spawn_guided_browser(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(20),
                top: percent(10),
                width: percent(60),
                max_height: percent(75),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(10),
                padding: UiRect::all(px(18)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.035, 0.06, 0.99)),
            GlobalZIndex(81),
            Visibility::Hidden,
            GuidedBrowserRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("GUIDED SCENARIOS\nNo guided content is installed."),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.94, 1.0)),
                TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
                GuidedBrowserText,
            ));
            root.spawn((
                Node {
                    display: Display::Flex,
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    column_gap: px(8),
                    row_gap: px(8),
                    ..default()
                },
                children![
                    guided_button("Previous [←]", GuidedControl::Previous),
                    guided_button("Next [→]", GuidedControl::Next),
                    guided_button("Start [Enter]", GuidedControl::Start),
                    guided_button("Resume [R]", GuidedControl::Resume),
                    guided_button("Reset [Delete]", GuidedControl::Reset),
                    guided_button("Back [Esc]", GuidedControl::Close),
                ],
            ));
        });
}

fn guided_button(label: &'static str, control: GuidedControl) -> impl Bundle {
    (
        Button,
        Node {
            min_height: px(34),
            padding: UiRect::axes(px(9), px(5)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.11, 0.18, 0.27)),
        Interaction::default(),
        PanelSurface,
        control,
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.93, 0.98)),
        )],
    )
}

fn attach_guided_objective_surface(mut commands: Commands, bodies: Query<(Entity, &PanelBody)>) {
    let Some((body, _)) = bodies.iter().find(|(_, body)| body.0 == PanelKind::Match) else {
        return;
    };
    commands.entity(body).with_child((
        Node {
            width: percent(100),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            padding: UiRect::all(px(6)),
            border: UiRect::all(px(1)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.48, 0.57, 0.75)),
        BackgroundColor(Color::srgb(0.07, 0.1, 0.17)),
        Interaction::default(),
        PanelSurface,
        GuidedObjectiveRoot,
        children![
            (
                Text::new("GUIDED OBJECTIVE"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.89, 0.61)),
                TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                GuidedObjectiveText,
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(5),
                    row_gap: px(5),
                    ..default()
                },
                children![
                    guided_button("Hint [J]", GuidedControl::Hint),
                    guided_button("Retry [T]", GuidedControl::Retry),
                    guided_button("Replay", GuidedControl::Replay),
                    guided_button("Leave [Esc]", GuidedControl::Leave),
                ],
            ),
        ],
    ));
}

fn load_guided_progress(mut runtime: ResMut<GuidedRuntime>) {
    match read_progress() {
        Ok(progress) => runtime.progress = progress,
        Err(error) => runtime.message = format!("Progress was not loaded: {error}"),
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]
fn handle_guided_controls(
    keys: Res<ButtonInput<KeyCode>>,
    pressed: Query<(&Interaction, &GuidedControl), Changed<Interaction>>,
    catalog: Res<GuidedScenarioCatalog>,
    mut runtime: ResMut<GuidedRuntime>,
    mut flow: ResMut<ClientFlow>,
    mut setup: ResMut<LocalSetup>,
    mut game: ResMut<DisplayedGame>,
    mut selection: ResMut<OverlaySelection>,
    mut history: ResMut<LocalTransitionNoticeLog>,
    mut transitions: ResMut<LocalTransitionEventQueue>,
    mut ai_epoch: Option<ResMut<AiCancellationEpoch>>,
) {
    let mut controls = pressed
        .iter()
        .filter_map(|(interaction, control)| {
            (*interaction == Interaction::Pressed).then_some(*control)
        })
        .collect::<Vec<_>>();
    if *flow == ClientFlow::Setup && !runtime.browser_open && keys.just_pressed(KeyCode::KeyG) {
        controls.push(GuidedControl::Open);
    }
    if runtime.browser_open {
        if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::PageUp) {
            controls.push(GuidedControl::Previous);
        }
        if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::PageDown) {
            controls.push(GuidedControl::Next);
        }
        if keys.just_pressed(KeyCode::Enter) {
            controls.push(GuidedControl::Start);
        }
        if keys.just_pressed(KeyCode::KeyR) {
            controls.push(GuidedControl::Resume);
        }
        if keys.just_pressed(KeyCode::Delete) {
            controls.push(GuidedControl::Reset);
        }
        if keys.just_pressed(KeyCode::Escape) {
            controls.push(GuidedControl::Close);
        }
    } else if runtime.session.is_some()
        && matches!(*flow, ClientFlow::Playing | ClientFlow::Outcome)
    {
        if keys.just_pressed(KeyCode::KeyJ) {
            controls.push(GuidedControl::Hint);
        }
        if keys.just_pressed(KeyCode::KeyT) {
            controls.push(GuidedControl::Retry);
        }
        if keys.just_pressed(KeyCode::Escape) {
            controls.push(GuidedControl::Leave);
        }
    }

    let mut cancel_ai = false;
    let mut persist_now = false;
    for control in controls {
        match control {
            GuidedControl::Open if *flow == ClientFlow::Setup => {
                runtime.browser_open = true;
                runtime.reset_armed = None;
            }
            GuidedControl::Previous if runtime.browser_open => {
                select_relative(&catalog, &mut runtime, -1);
            }
            GuidedControl::Next if runtime.browser_open => {
                select_relative(&catalog, &mut runtime, 1);
            }
            GuidedControl::Start if runtime.browser_open => {
                cancel_ai = true;
                start_selected(
                    &catalog,
                    &mut runtime,
                    &mut flow,
                    &mut setup,
                    &mut game,
                    &mut selection,
                    &mut history,
                    &mut transitions,
                );
            }
            GuidedControl::Resume if runtime.browser_open => {
                cancel_ai = true;
                resume_selected(
                    &catalog,
                    &mut runtime,
                    &mut flow,
                    &mut setup,
                    &mut game,
                    &mut selection,
                    &mut history,
                    &mut transitions,
                );
            }
            GuidedControl::Reset if runtime.browser_open => reset_selected(&catalog, &mut runtime),
            GuidedControl::Close if runtime.browser_open => {
                runtime.browser_open = false;
                runtime.reset_armed = None;
            }
            GuidedControl::Hint => {
                reveal_hint(&catalog, &mut runtime);
                persist_now = true;
            }
            GuidedControl::Retry => {
                cancel_ai = true;
                retry_stage(
                    &mut runtime,
                    &mut flow,
                    &mut setup,
                    &mut game,
                    &mut selection,
                    &mut history,
                    &mut transitions,
                );
            }
            GuidedControl::Replay => {
                cancel_ai = true;
                replay_session(
                    &catalog,
                    &mut runtime,
                    &mut flow,
                    &mut setup,
                    &mut game,
                    &mut selection,
                    &mut history,
                    &mut transitions,
                );
            }
            GuidedControl::Leave => {
                cancel_ai = true;
                leave_guided(&mut runtime, &mut flow, &game);
            }
            _ => {}
        }
    }
    if persist_now {
        persist_runtime(&mut runtime, &game);
    }
    if cancel_ai && let Some(epoch) = ai_epoch.as_deref_mut() {
        epoch.cancel_pending();
    }
}

fn selected_scenario<'a>(
    catalog: &'a GuidedScenarioCatalog,
    runtime: &GuidedRuntime,
) -> Option<(
    usize,
    &'a crownline_core::ScenarioDefinition,
    &'a GuidedContent,
)> {
    let index = runtime.selected.min(catalog.0.len().saturating_sub(1));
    let scenario = &catalog.0[index];
    Some((index, scenario, scenario.guided.as_ref()?))
}

fn select_relative(catalog: &GuidedScenarioCatalog, runtime: &mut GuidedRuntime, delta: isize) {
    let count = catalog.0.len();
    if count == 0 {
        runtime.selected = 0;
        return;
    }
    runtime.selected = if delta < 0 {
        (runtime.selected + count - 1) % count
    } else {
        (runtime.selected + 1) % count
    };
    runtime.reset_armed = None;
}

#[allow(clippy::too_many_arguments)]
fn start_selected(
    catalog: &GuidedScenarioCatalog,
    runtime: &mut GuidedRuntime,
    flow: &mut ClientFlow,
    setup: &mut LocalSetup,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    history: &mut LocalTransitionNoticeLog,
    transitions: &mut LocalTransitionEventQueue,
) {
    let Some((scenario_index, scenario, guided)) = selected_scenario(catalog, runtime) else {
        "No guided scenarios are installed.".clone_into(&mut runtime.message);
        return;
    };
    if let Err(error) = validate_guided_ai_content(guided) {
        runtime.message = format!("Guided AI configuration is invalid: {error}");
        return;
    }
    let Ok(state) = MatchState::from_scenario(scenario) else {
        "This guided scenario no longer validates.".clone_into(&mut runtime.message);
        return;
    };
    let guided_id = guided.id.clone();
    let stage_index = first_available_stage(guided, &[]).unwrap_or(0);
    let attempts = runtime
        .progress
        .entries
        .entry(guided_id.clone())
        .or_default();
    attempts.attempts = attempts.attempts.saturating_add(1);
    game.scenario.clone_from(scenario);
    game.state = state.clone();
    setup.session_id = setup.session_id.saturating_add(1);
    setup.clock = None;
    setup.north_controller = SeatController::Human;
    setup.south_controller = SeatController::Human;
    if let Some(ai) = &guided.ai {
        match ai.seat {
            crownline_core::scenario::Player::North => {
                setup.north_controller =
                    SeatController::Ai(crownline_ai::DifficultyProfile::Apprentice);
            }
            crownline_core::scenario::Player::South => {
                setup.south_controller =
                    SeatController::Ai(crownline_ai::DifficultyProfile::Apprentice);
            }
        }
    }
    selection.piece = None;
    history.entries.clear();
    transitions.mark_local_discontinuity();
    runtime.session = Some(GuidedSession {
        guided_id,
        scenario_index,
        stage_index,
        completed_stage_ids: Vec::new(),
        hint_reveals: vec![0; guided.stages.len()],
        actions_taken: 0,
        turns_elapsed: 0,
        total_actions: 0,
        total_turns: 0,
        ai_actions_taken: 0,
        retries: 0,
        failed: false,
        complete: false,
        stage_start: state.clone(),
        last_turn_number: state.turn_number,
    });
    runtime.browser_open = false;
    runtime.reset_armed = None;
    "Guided attempt started.".clone_into(&mut runtime.message);
    *flow = ClientFlow::Playing;
    persist_runtime(runtime, game);
}

#[allow(clippy::too_many_arguments)]
fn resume_selected(
    catalog: &GuidedScenarioCatalog,
    runtime: &mut GuidedRuntime,
    flow: &mut ClientFlow,
    setup: &mut LocalSetup,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    history: &mut LocalTransitionNoticeLog,
    transitions: &mut LocalTransitionEventQueue,
) {
    let Some((scenario_index, scenario, guided)) = selected_scenario(catalog, runtime) else {
        "No guided scenario is selected.".clone_into(&mut runtime.message);
        return;
    };
    if let Err(error) = validate_guided_ai_content(guided) {
        runtime.message = format!("Guided AI configuration is invalid: {error}");
        return;
    }
    let Some(resume) = runtime.progress.resume.clone() else {
        "No guided attempt is available to resume.".clone_into(&mut runtime.message);
        return;
    };
    if resume.guided_id != guided.id {
        "The saved attempt belongs to another guided scenario.".clone_into(&mut runtime.message);
        return;
    }
    let Ok(saved_scenario) =
        ron::from_str::<crownline_core::ScenarioDefinition>(&resume.scenario_ron)
    else {
        "The guided resume scenario is malformed.".clone_into(&mut runtime.message);
        return;
    };
    if saved_scenario != *scenario {
        "Guided content changed; start a fresh attempt.".clone_into(&mut runtime.message);
        return;
    }
    let Ok(state) = validate_resume(&resume, scenario) else {
        "The guided resume state failed canonical validation.".clone_into(&mut runtime.message);
        return;
    };
    game.scenario.clone_from(scenario);
    game.state = state.clone();
    setup.session_id = setup.session_id.saturating_add(1);
    setup.clock = None;
    setup.north_controller = SeatController::Human;
    setup.south_controller = SeatController::Human;
    selection.piece = None;
    history.entries.clear();
    transitions.mark_local_discontinuity();
    runtime.session = Some(GuidedSession {
        guided_id: resume.guided_id,
        scenario_index,
        stage_index: resume.stage_index,
        completed_stage_ids: resume.completed_stage_ids,
        hint_reveals: resume.hint_reveals,
        actions_taken: resume.actions_taken,
        turns_elapsed: resume.turns_elapsed,
        total_actions: resume.total_actions,
        total_turns: resume.total_turns,
        ai_actions_taken: resume.ai_actions_taken,
        retries: resume.retries,
        failed: resume.failed,
        complete: false,
        stage_start: resume.stage_start,
        last_turn_number: state.turn_number,
    });
    runtime.browser_open = false;
    "Guided attempt resumed from its exact canonical state.".clone_into(&mut runtime.message);
    *flow = ClientFlow::Playing;
}

fn reset_selected(catalog: &GuidedScenarioCatalog, runtime: &mut GuidedRuntime) {
    let Some((_, _, guided)) = selected_scenario(catalog, runtime) else {
        "No guided scenario is selected.".clone_into(&mut runtime.message);
        return;
    };
    if !request_progress_reset(&mut runtime.progress, &mut runtime.reset_armed, &guided.id) {
        "Reset is armed but not applied. Press Reset again to confirm."
            .clone_into(&mut runtime.message);
        return;
    }
    "Guided progress reset; ordinary save slots were untouched.".clone_into(&mut runtime.message);
    persist_document(&runtime.progress, &mut runtime.message);
}

fn request_progress_reset(
    progress: &mut GuidedProgressDocument,
    armed: &mut Option<String>,
    guided_id: &str,
) -> bool {
    if armed.as_deref() != Some(guided_id) {
        *armed = Some(guided_id.to_owned());
        return false;
    }
    progress.entries.remove(guided_id);
    if progress
        .resume
        .as_ref()
        .is_some_and(|resume| resume.guided_id == guided_id)
    {
        progress.resume = None;
    }
    *armed = None;
    true
}

fn reveal_hint(catalog: &GuidedScenarioCatalog, runtime: &mut GuidedRuntime) {
    let Some(mut session) = runtime.session.take() else {
        return;
    };
    let Some(guided) = catalog.0[session.scenario_index].guided.as_ref() else {
        runtime.session = Some(session);
        return;
    };
    let available = guided.stages[session.stage_index].hint_keys.len();
    let revealed = &mut session.hint_reveals[session.stage_index];
    if usize::from(*revealed) < available {
        *revealed = revealed.saturating_add(1);
        let metrics = runtime
            .progress
            .entries
            .entry(session.guided_id.clone())
            .or_default();
        metrics.hints_revealed = metrics.hints_revealed.saturating_add(1);
        runtime.message = format!("Hint {revealed} of {available} revealed by request.");
    } else {
        "No further hints are available for this stage.".clone_into(&mut runtime.message);
    }
    runtime.session = Some(session);
}

#[allow(clippy::too_many_arguments)]
fn retry_stage(
    runtime: &mut GuidedRuntime,
    flow: &mut ClientFlow,
    setup: &mut LocalSetup,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    history: &mut LocalTransitionNoticeLog,
    transitions: &mut LocalTransitionEventQueue,
) {
    let Some(session) = runtime.session.as_mut() else {
        return;
    };
    game.state.clone_from(&session.stage_start);
    setup.session_id = setup.session_id.saturating_add(1);
    selection.piece = None;
    history.entries.clear();
    transitions.mark_local_discontinuity();
    session.actions_taken = 0;
    session.turns_elapsed = 0;
    session.last_turn_number = game.state.turn_number;
    session.failed = false;
    session.complete = false;
    session.retries = session.retries.saturating_add(1);
    runtime
        .progress
        .entries
        .entry(session.guided_id.clone())
        .or_default()
        .retries = session.retries;
    "Stage restored to its canonical starting state.".clone_into(&mut runtime.message);
    *flow = ClientFlow::Playing;
    persist_runtime(runtime, game);
}

#[allow(clippy::too_many_arguments)]
fn replay_session(
    catalog: &GuidedScenarioCatalog,
    runtime: &mut GuidedRuntime,
    flow: &mut ClientFlow,
    setup: &mut LocalSetup,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    history: &mut LocalTransitionNoticeLog,
    transitions: &mut LocalTransitionEventQueue,
) {
    let Some(session) = runtime.session.as_ref() else {
        return;
    };
    runtime.selected = session.scenario_index;
    start_selected(
        catalog,
        runtime,
        flow,
        setup,
        game,
        selection,
        history,
        transitions,
    );
}

fn leave_guided(runtime: &mut GuidedRuntime, flow: &mut ClientFlow, game: &DisplayedGame) {
    persist_runtime(runtime, game);
    runtime.session = None;
    runtime.browser_open = true;
    "Attempt saved locally. Resume returns to the same state and stage."
        .clone_into(&mut runtime.message);
    *flow = ClientFlow::Setup;
}

#[allow(clippy::needless_pass_by_value)]
fn process_guided_transitions(
    mut transitions: ResMut<LocalTransitionEventQueue>,
    catalog: Res<GuidedScenarioCatalog>,
    mut runtime: ResMut<GuidedRuntime>,
    game: Res<DisplayedGame>,
) {
    let records = transitions.drain_guided_records().collect::<Vec<_>>();
    let Some(mut session) = runtime.session.take() else {
        return;
    };
    let Some(guided) = catalog.0[session.scenario_index].guided.as_ref() else {
        runtime.session = Some(session);
        return;
    };
    let mut changed = false;
    for record in records {
        if session.failed || session.complete {
            continue;
        }
        if record.action.is_some() {
            session.actions_taken = session.actions_taken.saturating_add(1);
            session.total_actions = session.total_actions.saturating_add(1);
            if record.action.as_ref().is_some_and(|action| {
                Some(action_player(action)) == guided.ai.as_ref().map(|ai| ai.seat)
            }) {
                session.ai_actions_taken = session.ai_actions_taken.saturating_add(1);
            }
        }
        let elapsed_turns = u16::try_from(
            record
                .state
                .turn_number
                .saturating_sub(session.last_turn_number),
        )
        .unwrap_or(u16::MAX);
        session.turns_elapsed = session.turns_elapsed.saturating_add(elapsed_turns);
        session.total_turns = session.total_turns.saturating_add(elapsed_turns);
        session.last_turn_number = record.state.turn_number;
        let stage = &guided.stages[session.stage_index];
        let context = GuidedPredicateContext {
            scenario: &game.scenario,
            state: &record.state,
            events: &record.events,
            actions_taken: session.actions_taken,
            turns_elapsed: session.turns_elapsed,
        };
        match stage.evaluate(&context) {
            Ok(ObjectiveResult::InProgress) => {}
            Ok(ObjectiveResult::Failed) => {
                session.failed = true;
                "Objective failed. Review the explanation, then Retry."
                    .clone_into(&mut runtime.message);
            }
            Ok(ObjectiveResult::Succeeded) => {
                session.completed_stage_ids.push(stage.id.clone());
                if let Some(next) = first_available_stage(guided, &session.completed_stage_ids) {
                    session.stage_index = next;
                    session.stage_start = record.state.clone();
                    session.actions_taken = 0;
                    session.turns_elapsed = 0;
                    session.last_turn_number = record.state.turn_number;
                    "Objective complete. The next explanation is now active."
                        .clone_into(&mut runtime.message);
                } else {
                    session.complete = true;
                    runtime.message = guided.completion.as_ref().map_or_else(
                        || "Guided scenario complete.".to_owned(),
                        |completion| resolve_key(&completion.completion_key),
                    );
                    update_completion_metrics(&mut runtime.progress, &session, guided);
                }
            }
            Err(error) => {
                session.failed = true;
                runtime.message = format!("Objective evaluation failed safely: {error}");
            }
        }
        changed = true;
    }
    runtime.session = Some(session);
    if changed {
        persist_runtime(&mut runtime, &game);
    }
}

fn action_player(action: &Action) -> crownline_core::scenario::Player {
    match action {
        Action::Move { player, .. }
        | Action::Hold { player }
        | Action::ChoosePromotion { player, .. }
        | Action::PlacePawn { player, .. }
        | Action::Resign { player }
        | Action::OfferDraw { player }
        | Action::RespondToDraw { player, .. } => *player,
    }
}

fn first_available_stage(guided: &GuidedContent, completed: &[String]) -> Option<usize> {
    let completed = completed
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    guided.stages.iter().enumerate().find_map(|(index, stage)| {
        (!completed.contains(stage.id.as_str())
            && stage
                .prerequisites
                .iter()
                .all(|required| completed.contains(required.as_str())))
        .then_some(index)
    })
}

fn update_completion_metrics(
    progress: &mut GuidedProgressDocument,
    session: &GuidedSession,
    guided: &GuidedContent,
) {
    let metrics = progress
        .entries
        .entry(session.guided_id.clone())
        .or_default();
    metrics.completed = true;
    metrics.retries = metrics.retries.max(session.retries);
    if let Some(completion) = &guided.completion {
        if completion.records_best_actions {
            metrics.best_actions =
                Some(metrics.best_actions.map_or(session.total_actions, |best| {
                    best.min(session.total_actions)
                }));
        }
        if completion.records_best_turns {
            metrics.best_turns = Some(
                metrics
                    .best_turns
                    .map_or(session.total_turns, |best| best.min(session.total_turns)),
            );
        }
    }
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn sync_guided_ui(
    flow: Res<ClientFlow>,
    catalog: Res<GuidedScenarioCatalog>,
    runtime: Res<GuidedRuntime>,
    mut browser_roots: Query<&mut Visibility, With<GuidedBrowserRoot>>,
    mut browser_texts: Query<&mut Text, (With<GuidedBrowserText>, Without<GuidedObjectiveText>)>,
    mut objective_roots: Query<&mut Node, With<GuidedObjectiveRoot>>,
    mut objective_texts: Query<&mut Text, (With<GuidedObjectiveText>, Without<GuidedBrowserText>)>,
    mut open_buttons: Query<&mut Visibility, With<GuidedOpenButton>>,
) {
    for mut visibility in &mut browser_roots {
        *visibility = if runtime.browser_open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut visibility in &mut open_buttons {
        *visibility = if *flow == ClientFlow::Setup && !runtime.browser_open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let show_objective = runtime.session.is_some()
        && !matches!(
            *flow,
            ClientFlow::Setup | ClientFlow::OnlineLobby | ClientFlow::OnlinePlaying
        );
    for mut node in &mut objective_roots {
        node.display = if show_objective {
            Display::Flex
        } else {
            Display::None
        };
    }
    let browser_text = browser_summary(&catalog, &runtime);
    for mut text in &mut browser_texts {
        text.0.clone_from(&browser_text);
    }
    let objective_text = objective_summary(&catalog, &runtime);
    for mut text in &mut objective_texts {
        text.0.clone_from(&objective_text);
    }
}

fn browser_summary(catalog: &GuidedScenarioCatalog, runtime: &GuidedRuntime) -> String {
    let Some((_, scenario, guided)) = selected_scenario(catalog, runtime) else {
        return format!(
            "GUIDED SCENARIOS\nNo guided content is installed.\n{}",
            runtime.message
        );
    };
    let metrics = runtime.progress.entries.get(&guided.id);
    let resumable = runtime
        .progress
        .resume
        .as_ref()
        .is_some_and(|resume| resume.guided_id == guided.id);
    format!(
        "GUIDED SCENARIOS - {}\n{}\nCategory: {} - {:?} - {} stages\nProgress: {} - attempts {} - retries {} - hints {}\nResume available: {}\n{}",
        catalog.0.len(),
        scenario.metadata.name,
        resolve_key(&guided.category_key),
        guided.kind,
        guided.stages.len(),
        if metrics.is_some_and(|metrics| metrics.completed) {
            "complete"
        } else {
            "not complete"
        },
        metrics.map_or(0, |metrics| metrics.attempts),
        metrics.map_or(0, |metrics| metrics.retries),
        metrics.map_or(0, |metrics| metrics.hints_revealed),
        if resumable { "yes" } else { "no" },
        runtime.message,
    )
}

fn objective_summary(catalog: &GuidedScenarioCatalog, runtime: &GuidedRuntime) -> String {
    let Some(session) = runtime.session.as_ref() else {
        return String::new();
    };
    let Some(guided) = catalog.0[session.scenario_index].guided.as_ref() else {
        return String::new();
    };
    let stage = &guided.stages[session.stage_index];
    let revealed = usize::from(session.hint_reveals[session.stage_index]);
    let hints = stage
        .hint_keys
        .iter()
        .take(revealed)
        .enumerate()
        .map(|(index, key)| format!("Hint {}: {}", index + 1, resolve_key(key)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "GUIDED - {} of {} complete\n{}\nPurpose: {}\nObjective: {}\nActions {}{} - turns {}{}\nState: {}\n{}{}",
        session.completed_stage_ids.len(),
        guided.stages.len(),
        resolve_key(&stage.title_key),
        resolve_key(&stage.explanation_key),
        observable_objective(stage),
        session.actions_taken,
        stage
            .action_limit
            .map_or_else(String::new, |limit| format!("/{limit}")),
        session.turns_elapsed,
        stage
            .turn_limit
            .map_or_else(String::new, |limit| format!("/{limit}")),
        if session.complete {
            "complete - Replay or Leave"
        } else if session.failed {
            "failed - Retry restores the stage start"
        } else {
            "in progress"
        },
        if hints.is_empty() { "" } else { "\n" },
        hints,
    )
}

fn observable_objective(stage: &crownline_core::GuidedStage) -> String {
    stage
        .success
        .iter()
        .map(describe_predicate)
        .collect::<Vec<_>>()
        .join(" and ")
}

fn describe_predicate(predicate: &GuidedPredicate) -> String {
    match predicate {
        GuidedPredicate::LegalMove { player, piece, to } => format!(
            "make {:?} piece {:?} able to move to ({}, {})",
            player, piece, to.x, to.y
        ),
        GuidedPredicate::PieceAt { player, kind, at } => {
            format!("place the {:?} {:?} on ({}, {})", player, kind, at.x, at.y)
        }
        GuidedPredicate::PieceSurvives { piece } => format!("keep piece {piece:?} alive"),
        GuidedPredicate::PieceOnTerrain { piece, terrain } => {
            format!("move piece {piece:?} onto {terrain:?} terrain")
        }
        GuidedPredicate::MaterialAtLeast {
            player,
            kind,
            count,
        } => format!("keep at least {count} {player:?} {kind:?}"),
        GuidedPredicate::InCheck { player, expected } => {
            if *expected {
                format!("put the {player:?} King in check")
            } else {
                format!("leave the {player:?} King safe")
            }
        }
        GuidedPredicate::TurnPhase { phase } => format!("reach the {phase:?} phase"),
        GuidedPredicate::SettlementOwned {
            settlement_index,
            player,
        } => format!("claim settlement {settlement_index} for {player:?}"),
        GuidedPredicate::SettlementGoverned {
            settlement_index,
            player,
        } => format!("govern settlement {settlement_index} for {player:?}"),
        GuidedPredicate::SettlementEstablished {
            settlement_index,
            expected,
        } => format!(
            "make settlement {settlement_index} {}",
            if *expected {
                "established"
            } else {
                "unestablished"
            }
        ),
        GuidedPredicate::SettlementProducedPawn {
            settlement_index,
            expected,
        } => format!(
            "{} a Pawn at settlement {settlement_index}",
            if *expected { "produce" } else { "prevent" }
        ),
        GuidedPredicate::Outcome { winner, reason } => match winner {
            Some(player) => format!("finish with {player:?} winning by {reason:?}"),
            None => format!("finish with a draw by {reason:?}"),
        },
        GuidedPredicate::Event(event) => describe_event(event),
    }
}

fn describe_event(event: &GuidedEventPredicate) -> String {
    match event {
        GuidedEventPredicate::Move { piece } => piece.map_or_else(
            || "complete the required move".to_owned(),
            |piece| format!("move piece {piece:?}"),
        ),
        GuidedEventPredicate::Capture { piece } => piece.map_or_else(
            || "make the required capture".to_owned(),
            |piece| format!("capture piece {piece:?}"),
        ),
        GuidedEventPredicate::CrossEdge { piece, kind } => piece.map_or_else(
            || format!("cross a {kind:?} edge"),
            |piece| format!("cross a {kind:?} edge with piece {piece:?}"),
        ),
        GuidedEventPredicate::EnterTerrain { piece, terrain } => piece.map_or_else(
            || format!("enter {terrain:?} terrain"),
            |piece| format!("enter {terrain:?} terrain with piece {piece:?}"),
        ),
        GuidedEventPredicate::SettlementClaimed { settlement_index } => {
            describe_settlement_event("claim", *settlement_index)
        }
        GuidedEventPredicate::SettlementContested { settlement_index } => {
            describe_settlement_event("contest", *settlement_index)
        }
        GuidedEventPredicate::SettlementContinuityInterrupted { settlement_index } => {
            describe_settlement_event("interrupt continuity at", *settlement_index)
        }
        GuidedEventPredicate::SettlementDevelopmentAdvanced { settlement_index } => {
            describe_settlement_event("advance development at", *settlement_index)
        }
        GuidedEventPredicate::SettlementEstablished { settlement_index } => {
            describe_settlement_event("establish", *settlement_index)
        }
        GuidedEventPredicate::SettlementProductionAdvanced { settlement_index } => {
            describe_settlement_event("advance production at", *settlement_index)
        }
        GuidedEventPredicate::PawnProduced { settlement_index } => {
            describe_settlement_event("produce a Pawn at", *settlement_index)
        }
        GuidedEventPredicate::SettlementTransferCancelled { settlement_index } => {
            describe_settlement_event("cancel transfer at", *settlement_index)
        }
        GuidedEventPredicate::SettlementTransferred { settlement_index } => {
            describe_settlement_event("transfer", *settlement_index)
        }
        GuidedEventPredicate::Promotion { pawn, kind } => match (pawn, kind) {
            (Some(pawn), Some(kind)) => format!("promote Pawn {pawn:?} to {kind:?}"),
            (Some(pawn), None) => format!("promote Pawn {pawn:?}"),
            (None, Some(kind)) => format!("promote a Pawn to {kind:?}"),
            (None, None) => "complete a promotion".to_owned(),
        },
        GuidedEventPredicate::MatchEnded => "finish the match".to_owned(),
    }
}

fn describe_settlement_event(verb: &str, index: Option<u16>) -> String {
    index.map_or_else(
        || format!("{verb} a settlement"),
        |index| format!("{verb} settlement {index}"),
    )
}

fn resolve_key(key: &str) -> String {
    GUIDED_TEXT
        .get(key)
        .cloned()
        .unwrap_or_else(|| format!("[missing text: {key}]"))
}

fn persist_runtime(runtime: &mut GuidedRuntime, game: &DisplayedGame) {
    if let Some(session) = runtime.session.as_ref() {
        let Ok(scenario_ron) = ron::ser::to_string(&game.scenario) else {
            "Guided progress could not serialize its scenario.".clone_into(&mut runtime.message);
            return;
        };
        let Ok(state) = SaveEnvelope::new(env!("CARGO_PKG_VERSION"), game.state.clone()) else {
            "Guided progress could not serialize canonical state.".clone_into(&mut runtime.message);
            return;
        };
        runtime.progress.resume = (!session.complete).then(|| GuidedResume {
            guided_id: session.guided_id.clone(),
            scenario_ron,
            state,
            stage_start: session.stage_start.clone(),
            stage_index: session.stage_index,
            completed_stage_ids: session.completed_stage_ids.clone(),
            hint_reveals: session.hint_reveals.clone(),
            actions_taken: session.actions_taken,
            turns_elapsed: session.turns_elapsed,
            total_actions: session.total_actions,
            total_turns: session.total_turns,
            ai_actions_taken: session.ai_actions_taken,
            retries: session.retries,
            failed: session.failed,
        });
    }
    persist_document(&runtime.progress, &mut runtime.message);
}

fn persist_document(progress: &GuidedProgressDocument, message: &mut String) {
    if let Err(error) = write_progress(progress) {
        *message = format!("Progress remains in memory; local write failed: {error}");
    }
}

fn write_progress(progress: &GuidedProgressDocument) -> Result<PathBuf, String> {
    validate_progress(progress)?;
    let bytes = serde_json::to_vec_pretty(progress).map_err(|error| error.to_string())?;
    let path = progress_path()?;
    let mut storage = GuidedFileStorage::new(path.clone());
    write_bytes_atomically(&mut storage, &bytes, |temporary| {
        decode_progress(temporary)
            .map(|_| ())
            .map_err(crownline_core::PersistenceError::MalformedJson)
    })
    .map_err(|error| error.to_string())?;
    Ok(path)
}

fn read_progress() -> Result<GuidedProgressDocument, String> {
    let path = progress_path()?;
    if !path.exists() {
        return Ok(GuidedProgressDocument::default());
    }
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > u64::try_from(MAX_PERSISTED_BYTES).unwrap() {
        return Err("guided progress exceeds the bounded file size".to_owned());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    decode_progress(&bytes)
}

fn decode_progress(bytes: &[u8]) -> Result<GuidedProgressDocument, String> {
    if bytes.len() > MAX_PERSISTED_BYTES {
        return Err("guided progress exceeds the bounded file size".to_owned());
    }
    let progress: GuidedProgressDocument =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    validate_progress(&progress)?;
    Ok(progress)
}

fn validate_progress(progress: &GuidedProgressDocument) -> Result<(), String> {
    if progress.format_version != GUIDED_PROGRESS_VERSION {
        return Err(format!(
            "guided progress format {} is unsupported",
            progress.format_version
        ));
    }
    if progress.entries.len() > MAX_PROGRESS_ENTRIES {
        return Err("guided progress contains too many entries".to_owned());
    }
    for (id, metrics) in &progress.entries {
        if id.is_empty()
            || id.chars().count() > 128
            || u32::from(metrics.attempts) > MAX_METRIC
            || u32::from(metrics.retries) > MAX_METRIC
            || u32::from(metrics.hints_revealed) > MAX_METRIC
        {
            return Err("guided progress contains invalid bounded metrics".to_owned());
        }
    }
    if let Some(resume) = &progress.resume {
        let scenario: crownline_core::ScenarioDefinition = ron::from_str(&resume.scenario_ron)
            .map_err(|error| format!("resume scenario is invalid: {error}"))?;
        let guided = scenario
            .guided
            .as_ref()
            .ok_or_else(|| "resume scenario is not guided".to_owned())?;
        let completed = resume
            .completed_stage_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if guided.id != resume.guided_id
            || resume.stage_index >= guided.stages.len()
            || resume.hint_reveals.len() != guided.stages.len()
            || resume.completed_stage_ids.len() > guided.stages.len()
            || completed.len() != resume.completed_stage_ids.len()
            || completed
                .iter()
                .any(|id| !guided.stages.iter().any(|stage| stage.id == *id))
            || resume
                .hint_reveals
                .iter()
                .zip(&guided.stages)
                .any(|(revealed, stage)| usize::from(*revealed) > stage.hint_keys.len())
        {
            return Err("guided resume metadata is inconsistent".to_owned());
        }
        validate_resume(resume, &scenario)?;
    }
    Ok(())
}

fn validate_resume(
    resume: &GuidedResume,
    scenario: &crownline_core::ScenarioDefinition,
) -> Result<MatchState, String> {
    scenario
        .validate()
        .map_err(|errors| format!("resume scenario is invalid: {errors:?}"))?;
    let bytes = serde_json::to_vec(&resume.state).map_err(|error| error.to_string())?;
    let state = SaveReader::new()
        .read_with_scenario(&bytes, scenario)
        .map_err(|error| error.to_string())?
        .state;
    let stage_start = SaveEnvelope::new("guided-progress", resume.stage_start.clone())
        .map_err(|error| error.to_string())?;
    let stage_bytes = serde_json::to_vec(&stage_start).map_err(|error| error.to_string())?;
    SaveReader::new()
        .read_with_scenario(&stage_bytes, scenario)
        .map_err(|error| format!("guided stage start is invalid: {error}"))?;
    Ok(state)
}

fn progress_path() -> Result<PathBuf, String> {
    ProjectDirs::from("org", "Crownlines", "Crownlines")
        .map(|dirs| dirs.data_local_dir().join("guided-progress.json"))
        .ok_or_else(|| "platform progress directory is unavailable".to_owned())
}

struct GuidedFileStorage {
    current: PathBuf,
    temporary: PathBuf,
}

impl GuidedFileStorage {
    fn new(current: PathBuf) -> Self {
        let temporary = current.with_extension("json.tmp");
        Self { current, temporary }
    }
}

impl AtomicSaveStorage for GuidedFileStorage {
    fn write_temporary(&mut self, bytes: &[u8]) -> Result<(), String> {
        let parent = self
            .current
            .parent()
            .ok_or_else(|| "guided progress path has no parent".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let mut file = fs::File::create(&self.temporary).map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())
    }

    fn read_temporary(&mut self) -> Result<Vec<u8>, String> {
        fs::read(&self.temporary).map_err(|error| error.to_string())
    }

    fn replace_with_temporary(&mut self) -> Result<(), String> {
        fs::rename(&self.temporary, &self.current).map_err(|error| error.to_string())
    }

    fn discard_temporary(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}

#[cfg(test)]
mod tests {
    use crownline_core::{
        GUIDED_SCHEMA_VERSION, GuidedCompletion, GuidedEventPredicate, GuidedKind, GuidedPredicate,
        GuidedStage, GuidedStart,
    };

    use super::*;

    fn guided_scenario() -> crownline_core::ScenarioDefinition {
        let mut scenario: crownline_core::ScenarioDefinition =
            ron::from_str(include_str!("../assets/scenarios/introductory.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        scenario.guided = Some(GuidedContent {
            schema_version: GUIDED_SCHEMA_VERSION,
            id: "lesson.one".to_owned(),
            kind: GuidedKind::Tutorial,
            category_key: "guided.test.category".to_owned(),
            start: GuidedStart {
                state,
                human_seat: crownline_core::scenario::Player::South,
                allow_clock: false,
                allow_controller_changes: false,
            },
            stages: vec![GuidedStage {
                id: "move".to_owned(),
                title_key: "guided.test.title".to_owned(),
                explanation_key: "guided.test.explanation".to_owned(),
                hint_keys: vec!["guided.test.hint".to_owned()],
                prerequisites: Vec::new(),
                success: vec![GuidedPredicate::Event(GuidedEventPredicate::Move {
                    piece: None,
                })],
                failure: Vec::new(),
                action_limit: Some(3),
                turn_limit: Some(2),
            }],
            ai: None,
            completion: Some(GuidedCompletion {
                completion_key: "guided.test.complete".to_owned(),
                next_guided_id: None,
                records_best_actions: true,
                records_best_turns: true,
            }),
            reply_nodes: Vec::new(),
        });
        scenario.validate().unwrap();
        scenario
    }

    #[test]
    fn progress_document_round_trips_and_rejects_unbounded_entries() {
        let mut progress = GuidedProgressDocument::default();
        progress.entries.insert(
            "lesson.one".to_owned(),
            GuidedMetrics {
                completed: true,
                attempts: 2,
                retries: 1,
                hints_revealed: 1,
                best_actions: Some(3),
                best_turns: Some(2),
            },
        );
        let bytes = serde_json::to_vec(&progress).unwrap();
        assert_eq!(decode_progress(&bytes).unwrap().entries, progress.entries);

        for index in 0..=MAX_PROGRESS_ENTRIES {
            progress
                .entries
                .insert(index.to_string(), GuidedMetrics::default());
        }
        assert!(validate_progress(&progress).is_err());
    }

    #[test]
    fn reset_requires_two_explicit_requests() {
        let mut progress = GuidedProgressDocument::default();
        progress.entries.insert(
            "lesson.one".to_owned(),
            GuidedMetrics {
                completed: true,
                ..default()
            },
        );
        let mut armed = None;
        assert!(!request_progress_reset(
            &mut progress,
            &mut armed,
            "lesson.one"
        ));
        assert!(progress.entries.contains_key("lesson.one"));
        assert_eq!(armed.as_deref(), Some("lesson.one"));
        assert!(request_progress_reset(
            &mut progress,
            &mut armed,
            "lesson.one"
        ));
        assert!(!progress.entries.contains_key("lesson.one"));
        assert_eq!(armed, None);
    }

    #[test]
    fn resume_round_trip_restores_exact_canonical_state_and_rejects_bad_hint_count() {
        let scenario = guided_scenario();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let expected_hash = state.canonical_hash().unwrap();
        let resume = GuidedResume {
            guided_id: "lesson.one".to_owned(),
            scenario_ron: ron::ser::to_string(&scenario).unwrap(),
            state: SaveEnvelope::new("test", state.clone()).unwrap(),
            stage_start: state,
            stage_index: 0,
            completed_stage_ids: Vec::new(),
            hint_reveals: vec![1],
            actions_taken: 1,
            turns_elapsed: 0,
            total_actions: 1,
            total_turns: 0,
            ai_actions_taken: 0,
            retries: 0,
            failed: false,
        };
        let mut progress = GuidedProgressDocument {
            resume: Some(resume),
            ..default()
        };
        let bytes = serde_json::to_vec(&progress).unwrap();
        let decoded = decode_progress(&bytes).unwrap();
        let restored = validate_resume(decoded.resume.as_ref().unwrap(), &scenario).unwrap();
        assert_eq!(restored.canonical_hash().unwrap(), expected_hash);

        progress.resume.as_mut().unwrap().hint_reveals[0] = 2;
        assert!(validate_progress(&progress).is_err());
    }

    #[test]
    fn browser_and_owned_panel_surface_expose_every_pointer_control() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Startup, spawn_guided_browser)
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(PanelBody(PanelKind::Match));
            })
            .add_systems(PostStartup, attach_guided_objective_surface);
        app.update();

        let controls = app
            .world_mut()
            .query::<&GuidedControl>()
            .iter(app.world())
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            controls,
            BTreeSet::from([
                GuidedControl::Previous,
                GuidedControl::Next,
                GuidedControl::Start,
                GuidedControl::Resume,
                GuidedControl::Reset,
                GuidedControl::Close,
                GuidedControl::Hint,
                GuidedControl::Retry,
                GuidedControl::Leave,
                GuidedControl::Replay,
            ])
        );
        let objective = app
            .world_mut()
            .query_filtered::<&Node, With<GuidedObjectiveRoot>>()
            .single(app.world())
            .unwrap();
        assert_eq!(objective.display, Display::None);
        assert_eq!(objective.width, percent(100));
    }

    #[test]
    fn competitive_match_has_no_guided_objective_copy() {
        let catalog = GuidedScenarioCatalog::default();
        assert_eq!(objective_summary(&catalog, &GuidedRuntime::default()), "");
    }
}
