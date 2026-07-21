use bevy::{
    input_focus::tab_navigation::{TabIndex, TabNavigationPlugin},
    prelude::*,
    text::{EditableText, TextCursorStyle},
};
use crownline_core::{
    Action, apply_action,
    scenario::{Player, ScenarioDefinition},
    state::MatchState,
};

use crate::rendering::{
    DisplayedGame, LocalTransitionEventQueue, LocalTransitionNoticeLog, OverlaySelection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource, Default)]
pub(crate) enum ClientFlow {
    #[default]
    Setup,
    Playing,
    Paused,
    ConfirmResign,
    Outcome,
}

#[derive(Resource)]
struct ScenarioCatalog(Vec<ScenarioDefinition>);

#[derive(Resource)]
struct LocalSetup {
    selected_scenario: usize,
    session_id: u64,
    north_name: String,
    south_name: String,
    error: String,
}

impl Default for LocalSetup {
    fn default() -> Self {
        Self {
            selected_scenario: 1,
            session_id: 0,
            north_name: "North Player".to_owned(),
            south_name: "South Player".to_owned(),
            error: String::new(),
        }
    }
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
            .insert_resource(ScenarioCatalog(vec![
                ron::from_str(include_str!("../assets/scenarios/introductory.ron")).unwrap(),
                ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap(),
                ron::from_str(include_str!("../assets/scenarios/large.ron")).unwrap(),
            ]))
            .add_systems(Startup, spawn_lifecycle_ui)
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
        ClientFlow::Playing => {
            if game.state.outcome.is_some() {
                *flow = ClientFlow::Outcome;
            } else if keys.just_pressed(KeyCode::KeyP) {
                *flow = ClientFlow::Paused;
            } else if keys.just_pressed(KeyCode::KeyQ) {
                *flow = ClientFlow::ConfirmResign;
            } else if keys.just_pressed(KeyCode::KeyD) {
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
    game.state = MatchState::from_scenario(scenario).expect("catalog scenarios are validated");
    setup.session_id = setup.session_id.saturating_add(1);
    selection.piece = None;
    history.entries.clear();
}

fn apply_control(
    action: &Action,
    game: &mut DisplayedGame,
    events: &mut LocalTransitionEventQueue,
) {
    if let Ok(transition) = apply_action(&game.scenario, &game.state, action) {
        events.push_transition(&transition);
        game.state = transition.state;
    }
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
fn sync_lifecycle_ui(
    flow: Res<ClientFlow>,
    setup: Res<LocalSetup>,
    catalog: Res<ScenarioCatalog>,
    game: Res<DisplayedGame>,
    mut roots: Query<(
        &mut Visibility,
        Option<&SetupRoot>,
        Option<&PauseRoot>,
        Option<&OutcomeRoot>,
    )>,
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
        "CROWNLINES — LOCAL SETUP\nScenario: {} · {}×{} · {}–{} minutes\nDouble-step: {} · en passant: {} · castling routes: {}\nTab edits names · X swaps color assignments · PageUp/PageDown scenario · F2 start\nNorth blue/pale: {} · South orange/dark: {}\n{}",
        scenario.metadata.name,
        scenario.board.width,
        scenario.board.height,
        scenario.metadata.expected_minutes.0,
        scenario.metadata.expected_minutes.1,
        yes_no(scenario.rules.allow_pawn_double_step),
        yes_no(scenario.rules.allow_en_passant),
        scenario.castling_routes.len(),
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
}
