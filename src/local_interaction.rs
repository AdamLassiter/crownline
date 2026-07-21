use bevy::prelude::*;
use crownline_core::{
    Action, apply_action, is_in_check, legal_moves,
    scenario::{BoardSize, Coord},
    state::{MatchState, PieceId, TurnPhase},
};

use crate::rendering::{
    DisplayedGame, HoveredBoardSquare, LocalTransitionEventQueue, OverlaySelection, PointerCapture,
    coordinates::BoardGeometry,
};

const FOCUS_Z: f32 = 6.0;

#[derive(Debug, Resource, Default)]
struct BoardInteraction {
    keyboard_focus: Option<Coord>,
    observed_revision: Option<u64>,
    status: String,
    submitting: bool,
}

#[derive(Component)]
struct KeyboardFocusVisual;

#[derive(Component)]
struct InteractionHelpText;

type FocusAffordanceQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut Visibility),
    (With<KeyboardFocusVisual>, Without<InteractionHelpText>),
>;

type HelpAffordanceQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Text2d, &'static mut Transform),
    (With<InteractionHelpText>, Without<KeyboardFocusVisual>),
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Activation {
    Select(PieceId),
    Move { piece: PieceId, to: Coord },
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HoldAvailability {
    Available,
    Disabled(&'static str),
}

pub struct LocalInteractionPlugin;

impl Plugin for LocalInteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardInteraction>()
            .add_systems(Startup, spawn_interaction_affordances)
            .add_systems(
                Update,
                (handle_board_input, sync_interaction_affordances).chain(),
            );
    }
}

fn spawn_interaction_affordances(mut commands: Commands) {
    commands.spawn((
        Sprite::from_color(Color::srgba(1.0, 0.82, 0.18, 0.16), Vec2::splat(30.0)),
        Transform::from_xyz(0.0, 0.0, FOCUS_Z),
        Visibility::Hidden,
        Name::new("keyboard board focus"),
        KeyboardFocusVisual,
    ));
    commands.spawn((
        Text2d::new("Arrow keys: focus board · Enter: select/move · Esc: leave board · H: Hold"),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.91, 0.95)),
        TextLayout::justify(Justify::Center),
        Transform::from_xyz(0.0, 0.0, FOCUS_Z),
        Name::new("local interaction help"),
        InteractionHelpText,
    ));
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn handle_board_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    hovered: Res<HoveredBoardSquare>,
    capture: Res<PointerCapture>,
    mut game: ResMut<DisplayedGame>,
    mut selection: ResMut<OverlaySelection>,
    mut interaction: ResMut<BoardInteraction>,
    mut transitions: ResMut<LocalTransitionEventQueue>,
) {
    if interaction.observed_revision != Some(game.state.revision) {
        selection.piece = None;
        interaction.observed_revision = Some(game.state.revision);
        interaction.submitting = false;
    }

    if keys.just_pressed(KeyCode::Escape) {
        interaction.keyboard_focus = None;
        selection.piece = None;
        "Board focus released.".clone_into(&mut interaction.status);
        return;
    }

    if let Some(step) = navigation_step(&keys) {
        let origin = interaction
            .keyboard_focus
            .or_else(|| selected_coord(&game.state, selection.piece))
            .or(hovered.0)
            .unwrap_or(Coord::new(0, 0));
        interaction.keyboard_focus = Some(move_focus(origin, step, game.scenario.board));
    }

    if keys.just_pressed(KeyCode::KeyH) {
        submit_hold(
            &mut game,
            &mut selection,
            &mut interaction,
            &mut transitions,
        );
        return;
    }

    let activated = if keys.just_pressed(KeyCode::Enter) {
        interaction.keyboard_focus
    } else if mouse.just_pressed(MouseButton::Left) && !capture.ui_has_pointer {
        hovered.0
    } else {
        None
    };
    let Some(at) = activated else {
        return;
    };
    interaction.keyboard_focus = Some(at);

    match activation_for(&game, selection.piece, at) {
        Activation::Select(piece) => {
            selection.piece = Some(piece);
            "Piece selected. Choose a highlighted destination.".clone_into(&mut interaction.status);
        }
        Activation::Move { piece, to } => submit_action(
            &Action::Move {
                player: game.state.active_player,
                piece,
                to,
            },
            &mut game,
            &mut selection,
            &mut interaction,
            &mut transitions,
        ),
        Activation::Clear => {
            selection.piece = None;
            "Selection cleared.".clone_into(&mut interaction.status);
        }
    }
}

fn activation_for(game: &DisplayedGame, selected: Option<PieceId>, at: Coord) -> Activation {
    if let Some(piece) = game
        .state
        .pieces
        .values()
        .find(|piece| piece.at == at && piece.owner == game.state.active_player)
    {
        return Activation::Select(piece.id);
    }
    if let Some(piece) = selected
        && legal_moves(&game.scenario, &game.state)
            .unwrap_or_default()
            .iter()
            .any(|candidate| candidate.piece == piece && candidate.to == at)
    {
        return Activation::Move { piece, to: at };
    }
    Activation::Clear
}

fn submit_hold(
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    interaction: &mut BoardInteraction,
    transitions: &mut LocalTransitionEventQueue,
) {
    match hold_availability(&game.scenario, &game.state) {
        HoldAvailability::Available => submit_action(
            &Action::Hold {
                player: game.state.active_player,
            },
            game,
            selection,
            interaction,
            transitions,
        ),
        HoldAvailability::Disabled(reason) => reason.clone_into(&mut interaction.status),
    }
}

fn submit_action(
    action: &Action,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    interaction: &mut BoardInteraction,
    transitions: &mut LocalTransitionEventQueue,
) {
    if interaction.submitting {
        return;
    }
    interaction.submitting = true;
    match apply_action(&game.scenario, &game.state, action) {
        Ok(transition) => {
            transitions.push_transition(&transition);
            game.state = transition.state;
            interaction.observed_revision = Some(game.state.revision);
            selection.piece = None;
            "Command accepted.".clone_into(&mut interaction.status);
        }
        Err(error) => interaction.status = format!("Command rejected: {error}"),
    }
    interaction.submitting = false;
}

fn hold_availability(
    scenario: &crownline_core::scenario::ScenarioDefinition,
    state: &MatchState,
) -> HoldAvailability {
    if state.outcome.is_some() {
        return HoldAvailability::Disabled("Hold disabled: the match is finished.");
    }
    if matches!(state.phase, TurnPhase::ResolvingChoices { .. }) {
        return HoldAvailability::Disabled("Hold disabled: resolve the mandatory choice first.");
    }
    if is_in_check(scenario, state, state.active_player).unwrap_or(true) {
        return HoldAvailability::Disabled("Hold disabled: your King is in check.");
    }
    HoldAvailability::Available
}

fn navigation_step(keys: &ButtonInput<KeyCode>) -> Option<(i8, i8)> {
    if keys.just_pressed(KeyCode::ArrowLeft) {
        Some((-1, 0))
    } else if keys.just_pressed(KeyCode::ArrowRight) {
        Some((1, 0))
    } else if keys.just_pressed(KeyCode::ArrowUp) {
        Some((0, -1))
    } else if keys.just_pressed(KeyCode::ArrowDown) {
        Some((0, 1))
    } else {
        None
    }
}

fn move_focus(at: Coord, step: (i8, i8), board: BoardSize) -> Coord {
    let x = i32::from(at.x) + i32::from(step.0);
    let y = i32::from(at.y) + i32::from(step.1);
    Coord::new(
        u16::try_from(x.clamp(0, i32::from(board.width) - 1)).expect("clamped x fits"),
        u16::try_from(y.clamp(0, i32::from(board.height) - 1)).expect("clamped y fits"),
    )
}

fn selected_coord(state: &MatchState, selected: Option<PieceId>) -> Option<Coord> {
    selected.and_then(|piece| state.pieces.get(&piece).map(|piece| piece.at))
}

#[allow(clippy::needless_pass_by_value)]
fn sync_interaction_affordances(
    game: Res<DisplayedGame>,
    geometry: Res<BoardGeometry>,
    interaction: Res<BoardInteraction>,
    mut focus: FocusAffordanceQuery,
    mut help: HelpAffordanceQuery,
) {
    if let Ok((mut transform, mut visibility)) = focus.single_mut() {
        if let Some(at) = interaction.keyboard_focus
            && let Some(world) = geometry.board_to_world(at)
        {
            transform.translation = world.extend(FOCUS_Z);
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
    if let Ok((mut text, mut transform)) = help.single_mut() {
        let hold = match hold_availability(&game.scenario, &game.state) {
            HoldAvailability::Available => "H: Hold (available)",
            HoldAvailability::Disabled(reason) => reason,
        };
        text.0 = if interaction.status.is_empty() {
            format!("Arrow keys: focus · Enter: select/move · Esc: leave board · {hold}")
        } else {
            format!("{}\n{hold}", interaction.status)
        };
        transform.translation = Vec3::new(
            0.0,
            -(f32::from(geometry.board.height) * geometry.tile_size / 2.0) - 26.0,
            FOCUS_Z,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> DisplayedGame {
        let scenario = ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        DisplayedGame { scenario, state }
    }

    #[test]
    fn activation_selects_reselects_cancels_and_confirms_only_legal_destinations() {
        let game = game();
        let moves = legal_moves(&game.scenario, &game.state).unwrap();
        let legal = moves.first().expect("standard opening has a legal move");
        let selected_at = game.state.pieces[&legal.piece].at;
        assert_eq!(
            activation_for(&game, None, selected_at),
            Activation::Select(legal.piece)
        );

        let other = game
            .state
            .pieces
            .values()
            .find(|piece| piece.owner == game.state.active_player && piece.id != legal.piece)
            .unwrap();
        assert_eq!(
            activation_for(&game, Some(legal.piece), other.at),
            Activation::Select(other.id)
        );
        assert_eq!(
            activation_for(&game, Some(legal.piece), legal.to),
            Activation::Move {
                piece: legal.piece,
                to: legal.to,
            }
        );
        let inactive = game
            .state
            .pieces
            .values()
            .find(|piece| piece.owner != game.state.active_player)
            .unwrap();
        assert_eq!(
            activation_for(&game, Some(legal.piece), inactive.at),
            Activation::Clear
        );
    }

    #[test]
    fn one_submission_advances_exactly_one_revision_and_clears_selection() {
        let mut game = game();
        let legal = legal_moves(&game.scenario, &game.state).unwrap().remove(0);
        let mut selection = OverlaySelection {
            piece: Some(legal.piece),
        };
        let mut interaction = BoardInteraction::default();
        let mut transitions = LocalTransitionEventQueue::default();
        submit_action(
            &Action::Move {
                player: game.state.active_player,
                piece: legal.piece,
                to: legal.to,
            },
            &mut game,
            &mut selection,
            &mut interaction,
            &mut transitions,
        );
        assert_eq!(game.state.revision, 1);
        assert_eq!(selection.piece, None);
        assert!(!interaction.submitting);
    }

    #[test]
    fn hold_reports_choice_and_check_reasons() {
        let mut game = game();
        game.state.phase = TurnPhase::ResolvingChoices { queue: Vec::new() };
        assert_eq!(
            hold_availability(&game.scenario, &game.state),
            HoldAvailability::Disabled("Hold disabled: resolve the mandatory choice first.")
        );

        game.state.phase = TurnPhase::Command;
        let king = game
            .state
            .pieces
            .values()
            .find(|piece| {
                piece.owner == game.state.active_player
                    && piece.kind == crownline_core::scenario::PieceKind::King
            })
            .unwrap()
            .id;
        let opposing_king = game
            .state
            .pieces
            .values()
            .find(|piece| {
                piece.owner != game.state.active_player
                    && piece.kind == crownline_core::scenario::PieceKind::King
            })
            .unwrap()
            .id;
        let opposing_rook = game
            .state
            .pieces
            .values()
            .find(|piece| {
                piece.owner != game.state.active_player
                    && piece.kind == crownline_core::scenario::PieceKind::Rook
            })
            .unwrap()
            .id;
        game.state
            .pieces
            .retain(|id, _| [king, opposing_king, opposing_rook].contains(id));
        game.state.pieces.get_mut(&king).unwrap().at = Coord::new(0, 5);
        game.state.pieces.get_mut(&opposing_rook).unwrap().at = Coord::new(0, 6);
        game.state.pieces.get_mut(&opposing_king).unwrap().at = Coord::new(7, 7);
        assert_eq!(
            hold_availability(&game.scenario, &game.state),
            HoldAvailability::Disabled("Hold disabled: your King is in check.")
        );
    }

    #[test]
    fn keyboard_focus_clamps_to_board_and_can_be_released() {
        let board = BoardSize {
            width: 8,
            height: 8,
        };
        assert_eq!(
            move_focus(Coord::new(0, 0), (-1, -1), board),
            Coord::new(0, 0)
        );
        assert_eq!(
            move_focus(Coord::new(7, 7), (1, 1), board),
            Coord::new(7, 7)
        );
    }

    #[test]
    fn full_local_plugin_update_has_disjoint_affordance_queries() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_plugins(crate::rendering::BoardRenderingPlugin)
            .add_plugins(LocalInteractionPlugin);
        app.update();
        let mut query = app
            .world_mut()
            .query_filtered::<Entity, With<KeyboardFocusVisual>>();
        assert_eq!(query.iter(app.world()).count(), 1);
    }
}
