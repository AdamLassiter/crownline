use bevy::{
    input_focus::tab_navigation::{TabIndex, TabNavigationPlugin},
    prelude::*,
    text::{EditableText, TextCursorStyle},
};
use crownline_core::{
    Action, ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES,
    advance_clock, apply_timed_action,
    scenario::{Player, ScenarioDefinition},
    start_clocks,
    state::MatchState,
};

use crate::{
    config::unmodified_just_pressed,
    rendering::{
        DisplayedGame, LocalTransitionEventQueue, LocalTransitionNoticeLog, OverlaySelection,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource, Default)]
pub(crate) enum ClientFlow {
    #[default]
    Setup,
    OnlineLobby,
    OnlinePlaying,
    Playing,
    Paused,
    ConfirmResign,
    Outcome,
}

#[derive(Resource)]
pub(crate) struct ScenarioCatalog(pub(crate) Vec<ScenarioDefinition>);

impl Default for ScenarioCatalog {
    fn default() -> Self {
        Self(vec![
            ron::from_str(include_str!("../assets/scenarios/introductory.ron")).unwrap(),
            ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap(),
            ron::from_str(include_str!("../assets/scenarios/large.ron")).unwrap(),
        ])
    }
}

#[derive(Resource)]
pub(crate) struct LocalSetup {
    pub(crate) selected_scenario: usize,
    pub(crate) session_id: u64,
    pub(crate) north_name: String,
    pub(crate) south_name: String,
    pub(crate) error: String,
    pub(crate) clock: Option<ClockSettings>,
}

impl Default for LocalSetup {
    fn default() -> Self {
        Self {
            selected_scenario: 1,
            session_id: 0,
            north_name: "North Player".to_owned(),
            south_name: "South Player".to_owned(),
            error: String::new(),
            clock: None,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct LocalClockRuntime {
    pub(crate) sub_millisecond_nanos: u32,
}

#[derive(Component)]
struct SetupRoot;
#[derive(Component)]
struct PauseRoot;
#[derive(Component)]
struct OutcomeRoot;
#[derive(Component)]
struct LifecycleText;
#[derive(Component)]
struct PlayerNameInput(Player);

pub struct LocalLifecyclePlugin;

impl Plugin for LocalLifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TabNavigationPlugin)
            .init_resource::<ClientFlow>()
            .init_resource::<LocalSetup>()
            .init_resource::<LocalClockRuntime>()
            .init_resource::<ScenarioCatalog>()
            .add_systems(Startup, spawn_lifecycle_ui)
            .add_systems(PreUpdate, tick_local_clock)
            .add_systems(Update, (handle_lifecycle_input, sync_lifecycle_ui).chain());
    }
}

fn spawn_lifecycle_ui(mut commands: Commands) {
    commands
        .spawn((modal_node(), SetupRoot))
        .with_children(|root| {
            root.spawn(title_text("CROWNLINES\nLocal match setup", LifecycleText));
            root.spawn(name_input("North Player", Player::North, 0));
            root.spawn(name_input("South Player", Player::South, 1));
        });
    commands.spawn((modal_node(), Visibility::Hidden, PauseRoot, children![title_text(
        "PAUSED · SETTINGS\nP resume · F1 rules · I panels\nClocks and gameplay input are paused.",
        LifecycleText,
    )]));
    commands.spawn((
        modal_node(),
        Visibility::Hidden,
        OutcomeRoot,
        children![title_text("Match outcome pending", LifecycleText,)],
    ));
}

fn modal_node() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: percent(20),
            top: percent(15),
            width: percent(60),
            min_height: percent(45),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(12),
            padding: UiRect::all(px(18)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        BackgroundColor(Color::srgba(0.025, 0.035, 0.06, 0.98)),
        GlobalZIndex(80),
    )
}

fn title_text(text: &str, marker: impl Component) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::srgb(0.92, 0.94, 1.0)),
        TextLayout::justify(Justify::Center),
        marker,
    )
}

fn name_input(value: &str, player: Player, tab: i32) -> impl Bundle {
    (
        Node {
            width: percent(70),
            min_height: px(38),
            border: UiRect::all(px(2)),
            padding: UiRect::all(px(6)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.42, 0.55, 0.72)),
        BackgroundColor(Color::srgb(0.08, 0.1, 0.16)),
        EditableText::new(value),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextCursorStyle::default(),
        TabIndex(tab),
        PlayerNameInput(player),
    )
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]
fn handle_lifecycle_input(
    keys: Res<ButtonInput<KeyCode>>,
    catalog: Res<ScenarioCatalog>,
    mut setup: ResMut<LocalSetup>,
    mut flow: ResMut<ClientFlow>,
    mut game: ResMut<DisplayedGame>,
    mut selection: ResMut<OverlaySelection>,
    mut events: ResMut<LocalTransitionEventQueue>,
    mut history: ResMut<LocalTransitionNoticeLog>,
    mut names: Query<(&mut EditableText, &PlayerNameInput)>,
) {
    match *flow {
        ClientFlow::Setup => {
            if keys.just_pressed(KeyCode::PageUp) {
                setup.selected_scenario =
                    (setup.selected_scenario + catalog.0.len() - 1) % catalog.0.len();
            }
            if keys.just_pressed(KeyCode::PageDown) {
                setup.selected_scenario = (setup.selected_scenario + 1) % catalog.0.len();
            }
            update_clock_setup(&keys, &mut setup);
            if keys.just_pressed(KeyCode::KeyX) {
                let mut north = String::new();
                let mut south = String::new();
                for (input, player) in &names {
                    match player.0 {
                        Player::North => north = input.value().to_string(),
                        Player::South => south = input.value().to_string(),
                    }
                }
                for (mut input, player) in &mut names {
                    input.editor_mut().set_text(match player.0 {
                        Player::North => &south,
                        Player::South => &north,
                    });
                }
            }
            if keys.just_pressed(KeyCode::F2) {
                let mut north = String::new();
                let mut south = String::new();
                for (input, player) in &names {
                    match player.0 {
                        Player::North => north = input.value().to_string(),
                        Player::South => south = input.value().to_string(),
                    }
                }
                match validate_names(&north, &south) {
                    Ok((north, south)) => {
                        setup.north_name = north;
                        setup.south_name = south;
                        setup.error.clear();
                        start_fresh_match(
                            &catalog.0[setup.selected_scenario],
                            &mut setup,
                            &mut game,
                            &mut selection,
                            &mut history,
                        );
                        *flow = ClientFlow::Playing;
                    }
                    Err(error) => error.clone_into(&mut setup.error),
                }
            }
        }
        ClientFlow::OnlineLobby | ClientFlow::OnlinePlaying => {}
        ClientFlow::Playing => {
            if game.state.outcome.is_some() {
                *flow = ClientFlow::Outcome;
            } else if keys.just_pressed(KeyCode::KeyP) {
                *flow = ClientFlow::Paused;
            } else if unmodified_just_pressed(&keys, KeyCode::KeyQ) {
                *flow = ClientFlow::ConfirmResign;
            } else if unmodified_just_pressed(&keys, KeyCode::KeyD) {
                apply_control(
                    &Action::OfferDraw {
                        player: game.state.active_player,
                    },
                    &mut game,
                    &mut events,
                );
            } else if keys.just_pressed(KeyCode::KeyY)
                && game.state.outstanding_draw_offer.is_some()
            {
                apply_control(
                    &Action::RespondToDraw {
                        player: game.state.active_player,
                        accept: true,
                    },
                    &mut game,
                    &mut events,
                );
            } else if keys.just_pressed(KeyCode::KeyN)
                && game.state.outstanding_draw_offer.is_some()
            {
                apply_control(
                    &Action::RespondToDraw {
                        player: game.state.active_player,
                        accept: false,
                    },
                    &mut game,
                    &mut events,
                );
            }
        }
        ClientFlow::Paused => {
            if keys.just_pressed(KeyCode::KeyP) || keys.just_pressed(KeyCode::Escape) {
                *flow = ClientFlow::Playing;
            }
        }
        ClientFlow::ConfirmResign => {
            if keys.just_pressed(KeyCode::Enter) {
                apply_control(
                    &Action::Resign {
                        player: game.state.active_player,
                    },
                    &mut game,
                    &mut events,
                );
                *flow = ClientFlow::Outcome;
            } else if keys.just_pressed(KeyCode::Escape) {
                *flow = ClientFlow::Playing;
            }
        }
        ClientFlow::Outcome => {
            if keys.just_pressed(KeyCode::KeyR) {
                let scenario = game.scenario.clone();
                start_fresh_match(
                    &scenario,
                    &mut setup,
                    &mut game,
                    &mut selection,
                    &mut history,
                );
                *flow = ClientFlow::Playing;
            }
        }
    }
}

fn validate_names(north: &str, south: &str) -> Result<(String, String), &'static str> {
    let north = north.trim();
    let south = south.trim();
    if north.is_empty() || south.is_empty() {
        return Err("Both player names are required.");
    }
    if north.chars().count() > 24 || south.chars().count() > 24 {
        return Err("Player names must be 24 characters or fewer.");
    }
    if north.eq_ignore_ascii_case(south) {
        return Err("Player names must be distinct.");
    }
    Ok((north.to_owned(), south.to_owned()))
}

fn start_fresh_match(
    scenario: &ScenarioDefinition,
    setup: &mut LocalSetup,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    history: &mut LocalTransitionNoticeLog,
) {
    game.scenario.clone_from(scenario);
    let state = MatchState::from_scenario(scenario).expect("catalog scenarios are validated");
    game.state = setup.clock.map_or(state.clone(), |settings| {
        start_clocks(&state, settings).expect("validated local clock settings")
    });
    setup.session_id = setup.session_id.saturating_add(1);
    selection.piece = None;
    history.entries.clear();
}

fn apply_control(
    action: &Action,
    game: &mut DisplayedGame,
    events: &mut LocalTransitionEventQueue,
) {
    if let Ok(transition) = apply_timed_action(&game.scenario, &game.state, action, 0) {
        events.push_local_action(action, &transition);
        game.state = transition.state;
    }
}

fn update_clock_setup(keys: &ButtonInput<KeyCode>, setup: &mut LocalSetup) {
    if keys.just_pressed(KeyCode::KeyC) {
        setup.clock = setup.clock.is_none().then_some(ClockSettings {
            base_minutes: 10,
            increment_seconds: 0,
        });
    }
    let Some(clock) = setup.clock.as_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::Minus) {
        clock.base_minutes = clock.base_minutes.saturating_sub(1).max(MIN_BASE_MINUTES);
    }
    if keys.just_pressed(KeyCode::Equal) {
        clock.base_minutes = clock.base_minutes.saturating_add(1).min(MAX_BASE_MINUTES);
    }
    if keys.just_pressed(KeyCode::Comma) {
        clock.increment_seconds = clock.increment_seconds.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::Period) {
        clock.increment_seconds = clock
            .increment_seconds
            .saturating_add(1)
            .min(MAX_INCREMENT_SECONDS);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn tick_local_clock(
    time: Option<Res<Time<Real>>>,
    mut runtime: ResMut<LocalClockRuntime>,
    mut flow: ResMut<ClientFlow>,
    mut game: ResMut<DisplayedGame>,
    mut events: ResMut<LocalTransitionEventQueue>,
) {
    if *flow != ClientFlow::Playing || game.state.outcome.is_some() || game.state.clocks.is_none() {
        return;
    }
    let Some(time) = time else {
        return;
    };
    let elapsed_millis = accumulate_elapsed(&mut runtime, time.delta());
    if elapsed_millis == 0 {
        return;
    }
    if let Ok(transition) = advance_clock(&game.state, elapsed_millis) {
        if transition.state.outcome.is_some() {
            events.push_local_clock(&transition);
            *flow = ClientFlow::Outcome;
        }
        game.state = transition.state;
    }
}

fn accumulate_elapsed(runtime: &mut LocalClockRuntime, delta: std::time::Duration) -> u64 {
    let total_nanos = u64::from(runtime.sub_millisecond_nanos) + u64::from(delta.subsec_nanos());
    runtime.sub_millisecond_nanos = u32::try_from(total_nanos % 1_000_000).unwrap();
    delta.as_secs() * 1_000 + total_nanos / 1_000_000
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
fn sync_lifecycle_ui(
    flow: Res<ClientFlow>,
    setup: Res<LocalSetup>,
    catalog: Res<ScenarioCatalog>,
    game: Res<DisplayedGame>,
    mut roots: Query<
        (
            &mut Visibility,
            Option<&SetupRoot>,
            Option<&PauseRoot>,
            Option<&OutcomeRoot>,
        ),
        Or<(With<SetupRoot>, With<PauseRoot>, With<OutcomeRoot>)>,
    >,
    mut texts: Query<&mut Text, With<LifecycleText>>,
) {
    for (mut visibility, setup_root, pause_root, outcome_root) in &mut roots {
        let visible = (setup_root.is_some() && *flow == ClientFlow::Setup)
            || (pause_root.is_some()
                && matches!(*flow, ClientFlow::Paused | ClientFlow::ConfirmResign))
            || (outcome_root.is_some() && *flow == ClientFlow::Outcome);
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let scenario = &catalog.0[setup.selected_scenario];
    let setup_text = format!(
        "CROWNLINES — LOCAL SETUP\nScenario: {} · {}×{} · {}–{} minutes\nDouble-step: {} · en passant: {} · castling routes: {}\nTab edits names · X swaps colors · PageUp/PageDown scenario · F2 local start · F3 online\nClock: {} · C toggle · -/+ base · ,/. increment\nNorth blue/pale: {} · South orange/dark: {}\n{}",
        scenario.metadata.name,
        scenario.board.width,
        scenario.board.height,
        scenario.metadata.expected_minutes.0,
        scenario.metadata.expected_minutes.1,
        yes_no(scenario.rules.allow_pawn_double_step),
        yes_no(scenario.rules.allow_en_passant),
        scenario.castling_routes.len(),
        setup.clock.map_or_else(
            || "untimed (default)".to_owned(),
            |clock| format!(
                "{} min + {} sec",
                clock.base_minutes, clock.increment_seconds
            ),
        ),
        setup.north_name,
        setup.south_name,
        setup.error,
    );
    let outcome_text = game.state.outcome.map_or_else(
        || "Match outcome pending".to_owned(),
        |outcome| {
            let winner = outcome.winner.map_or_else(
                || "Draw".to_owned(),
                |player| match player {
                    Player::North => format!("{} (North)", setup.north_name),
                    Player::South => format!("{} (South)", setup.south_name),
                },
            );
            format!(
                "MATCH ENDED\n{winner}\nReason: {:?}\nR rematch · settings preserved",
                outcome.reason
            )
        },
    );
    for mut text in &mut texts {
        if text.0.contains("LOCAL SETUP") || text.0.contains("Local match setup") {
            text.0.clone_from(&setup_text);
        } else if text.0.contains("outcome") || text.0.contains("MATCH ENDED") {
            text.0.clone_from(&outcome_text);
        } else if *flow == ClientFlow::ConfirmResign {
            text.0 = format!(
                "CONFIRM RESIGNATION\n{:?} will lose. Enter confirms · Esc cancels",
                game.state.active_player
            );
        } else {
            "PAUSED · SETTINGS\nP/Esc resume · F1 rules · I panels\nGameplay input is paused."
                .clone_into(&mut text.0);
        }
    }
}

const fn yes_no(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_names_are_rejected_and_trimmed_names_are_accepted() {
        assert!(validate_names("", "South").is_err());
        assert!(validate_names("Alex", "alex").is_err());
        assert!(validate_names(&"x".repeat(25), "South").is_err());
        assert_eq!(
            validate_names(" North ", " South ").unwrap(),
            ("North".to_owned(), "South".to_owned())
        );
    }

    #[test]
    fn lifecycle_modals_scroll_instead_of_clipping_scaled_content() {
        #[derive(Component)]
        struct UnrelatedVisibleEntity;

        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(crate::rendering::BoardRenderingPlugin)
            .add_plugins(LocalLifecyclePlugin);
        let unrelated = app
            .world_mut()
            .spawn((Visibility::Inherited, UnrelatedVisibleEntity))
            .id();
        app.update();
        let world = app.world_mut();
        let mut roots = world.query_filtered::<(&Node, &Visibility), With<SetupRoot>>();
        let (node, visibility) = roots.single(world).unwrap();
        assert_eq!(node.overflow, Overflow::scroll_y());
        assert_eq!(*visibility, Visibility::Visible);

        let mut text = world.query_filtered::<(&Text, &Visibility), With<LifecycleText>>();
        let (_, visibility) = text
            .iter(world)
            .find(|(text, _)| {
                text.0.contains("LOCAL SETUP")
                    && text.0.contains("F2 local start")
                    && text.0.contains("F3 online")
            })
            .expect("startup setup instructions");
        assert_eq!(*visibility, Visibility::Inherited);
        assert_eq!(
            *world.get::<Visibility>(unrelated).unwrap(),
            Visibility::Inherited,
            "lifecycle visibility updates must not hide unrelated entities"
        );
    }

    #[test]
    fn rematch_preserves_setup_but_creates_fresh_session_and_state() {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let mut game = DisplayedGame {
            state: MatchState::from_scenario(&scenario).unwrap(),
            scenario: scenario.clone(),
        };
        game.state.revision = 9;
        let mut setup = LocalSetup::default();
        let mut selection = OverlaySelection::default();
        let mut history = LocalTransitionNoticeLog {
            entries: vec!["old".to_owned()],
        };
        start_fresh_match(
            &scenario,
            &mut setup,
            &mut game,
            &mut selection,
            &mut history,
        );
        assert_eq!(setup.session_id, 1);
        assert_eq!(game.state.revision, 0);
        assert!(history.entries.is_empty());
        assert_eq!(setup.north_name, "North Player");
    }

    #[test]
    fn clock_setup_is_untimed_by_default_and_clamps_documented_bounds() {
        let mut setup = LocalSetup::default();
        assert_eq!(setup.clock, None);
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyC);
        update_clock_setup(&keys, &mut setup);
        assert_eq!(setup.clock.unwrap().base_minutes, 10);
        setup.clock = Some(ClockSettings {
            base_minutes: MIN_BASE_MINUTES,
            increment_seconds: 0,
        });
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Minus);
        keys.press(KeyCode::Comma);
        update_clock_setup(&keys, &mut setup);
        assert_eq!(setup.clock.unwrap().base_minutes, MIN_BASE_MINUTES);
        setup.clock = Some(ClockSettings {
            base_minutes: MAX_BASE_MINUTES,
            increment_seconds: MAX_INCREMENT_SECONDS,
        });
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Equal);
        keys.press(KeyCode::Period);
        update_clock_setup(&keys, &mut setup);
        assert_eq!(setup.clock.unwrap().base_minutes, MAX_BASE_MINUTES);
        assert_eq!(
            setup.clock.unwrap().increment_seconds,
            MAX_INCREMENT_SECONDS
        );
    }

    #[test]
    fn monotonic_elapsed_carry_is_independent_of_frame_partitioning() {
        let mut split = LocalClockRuntime::default();
        let split_total: u64 = [333_333, 333_333, 333_334]
            .into_iter()
            .map(|micros| accumulate_elapsed(&mut split, std::time::Duration::from_micros(micros)))
            .sum();
        let mut single = LocalClockRuntime::default();
        assert_eq!(
            split_total,
            accumulate_elapsed(&mut single, std::time::Duration::from_secs(1))
        );
        assert_eq!(split_total, 1_000);
    }

    #[test]
    fn exact_deadline_clock_transition_prevents_the_action() {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let state = start_clocks(
            &MatchState::from_scenario(&scenario).unwrap(),
            ClockSettings {
                base_minutes: 1,
                increment_seconds: 5,
            },
        )
        .unwrap();
        let action = Action::Hold {
            player: state.active_player,
        };
        let transition = apply_timed_action(&scenario, &state, &action, 60_000).unwrap();
        assert!(transition.state.outcome.is_some());
        assert_eq!(transition.state.turn_number, 1);
    }
}
