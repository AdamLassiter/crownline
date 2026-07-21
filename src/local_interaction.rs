use bevy::prelude::*;
use crownline_core::{
    Action, apply_timed_action, is_in_check, legal_moves,
    scenario::{BoardSize, Coord, Player},
    state::{MandatoryChoice, MatchState, PieceId, PromotionKind, TurnPhase},
};

use crate::{
    ChessFontText,
    lifecycle::ClientFlow,
    rendering::{
        ChessPieceFont, DisplayedGame, HoveredBoardSquare, LocalTransitionEventQueue,
        OverlaySelection, PointerCapture, coordinates::BoardGeometry,
    },
};

const FOCUS_Z: f32 = 6.0;
const CHOICE_Z: f32 = 5.8;

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

#[derive(Component)]
struct ChoiceVisual;

#[derive(Resource, Default)]
struct ChoicePresentation {
    revision: Option<u64>,
}

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
            .init_resource::<ChoicePresentation>()
            .add_systems(Startup, spawn_interaction_affordances)
            .add_systems(
                Update,
                (
                    handle_board_input,
                    sync_choice_affordances,
                    sync_interaction_affordances,
                )
                    .chain(),
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
    flow: Option<Res<ClientFlow>>,
) {
    if flow.is_some_and(|flow| *flow != ClientFlow::Playing) {
        selection.piece = None;
        return;
    }
    if interaction.observed_revision != Some(game.state.revision) {
        selection.piece = None;
        interaction.observed_revision = Some(game.state.revision);
        interaction.submitting = false;
    }

    if let TurnPhase::ResolvingChoices { queue } = &game.state.phase {
        let current_choice = queue.first().cloned();
        if let Some(choice) = current_choice {
            handle_mandatory_choice(
                &keys,
                &mouse,
                hovered.0,
                capture.ui_has_pointer,
                choice,
                &mut game,
                &mut selection,
                &mut interaction,
                &mut transitions,
            );
        } else {
            selection.piece = None;
            "Board controls disabled while the choice queue resolves."
                .clone_into(&mut interaction.status);
        }
        return;
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

#[allow(clippy::too_many_arguments)]
fn handle_mandatory_choice(
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    hovered: Option<Coord>,
    ui_has_pointer: bool,
    choice: MandatoryChoice,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    interaction: &mut BoardInteraction,
    transitions: &mut LocalTransitionEventQueue,
) {
    selection.piece = None;
    if keys.just_pressed(KeyCode::KeyH) {
        "Hold disabled: resolve the mandatory choice first.".clone_into(&mut interaction.status);
        return;
    }
    match choice {
        MandatoryChoice::Promote { pawn, .. } => {
            interaction.keyboard_focus = None;
            let promotion = if keys.just_pressed(KeyCode::Digit1) {
                Some(PromotionKind::Queen)
            } else if keys.just_pressed(KeyCode::Digit2) {
                Some(PromotionKind::Rook)
            } else if keys.just_pressed(KeyCode::Digit3) {
                Some(PromotionKind::Bishop)
            } else if keys.just_pressed(KeyCode::Digit4) {
                Some(PromotionKind::Knight)
            } else {
                None
            };
            if let Some(promote_to) = promotion {
                submit_action(
                    &Action::ChoosePromotion {
                        player: game.state.active_player,
                        pawn,
                        promote_to,
                    },
                    game,
                    selection,
                    interaction,
                    transitions,
                );
            } else if keys.just_pressed(KeyCode::Escape) {
                "Promotion is mandatory; choose 1, 2, 3, or 4.".clone_into(&mut interaction.status);
            }
        }
        MandatoryChoice::PlacePawn {
            settlement_index,
            legal_squares,
        } => {
            if legal_squares.is_empty() {
                "Pawn placement is waiting for a legal adjacent square."
                    .clone_into(&mut interaction.status);
                return;
            }
            let ordered: Vec<_> = legal_squares.iter().copied().collect();
            if !interaction
                .keyboard_focus
                .is_some_and(|at| legal_squares.contains(&at))
            {
                interaction.keyboard_focus = ordered.first().copied();
            }
            if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::ArrowUp) {
                interaction.keyboard_focus = cycle_choice_focus(
                    &ordered,
                    interaction.keyboard_focus,
                    CycleDirection::Previous,
                );
            } else if keys.just_pressed(KeyCode::ArrowRight)
                || keys.just_pressed(KeyCode::ArrowDown)
            {
                interaction.keyboard_focus =
                    cycle_choice_focus(&ordered, interaction.keyboard_focus, CycleDirection::Next);
            }
            if keys.just_pressed(KeyCode::Escape) {
                "Pawn placement is mandatory; choose a highlighted square."
                    .clone_into(&mut interaction.status);
                return;
            }
            let activated = if keys.just_pressed(KeyCode::Enter) {
                interaction.keyboard_focus
            } else if mouse.just_pressed(MouseButton::Left) && !ui_has_pointer {
                hovered
            } else {
                None
            };
            if let Some(at) = activated {
                if legal_squares.contains(&at) {
                    submit_action(
                        &Action::PlacePawn {
                            player: game.state.active_player,
                            settlement_index,
                            at,
                        },
                        game,
                        selection,
                        interaction,
                        transitions,
                    );
                } else {
                    "Choose one of the highlighted legal Pawn squares."
                        .clone_into(&mut interaction.status);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CycleDirection {
    Previous,
    Next,
}

fn cycle_choice_focus(
    choices: &[Coord],
    current: Option<Coord>,
    direction: CycleDirection,
) -> Option<Coord> {
    if choices.is_empty() {
        return None;
    }
    let current_index = current
        .and_then(|current| choices.iter().position(|choice| *choice == current))
        .unwrap_or(0);
    let index = match direction {
        CycleDirection::Previous => (current_index + choices.len() - 1) % choices.len(),
        CycleDirection::Next => (current_index + 1) % choices.len(),
    };
    Some(choices[index])
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
    match apply_timed_action(&game.scenario, &game.state, action, 0) {
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

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn sync_choice_affordances(
    mut commands: Commands,
    game: Res<DisplayedGame>,
    geometry: Res<BoardGeometry>,
    font: Res<ChessPieceFont>,
    mut presentation: ResMut<ChoicePresentation>,
    existing: Query<Entity, With<ChoiceVisual>>,
) {
    if presentation.revision == Some(game.state.revision) {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    if let TurnPhase::ResolvingChoices { queue } = &game.state.phase
        && let Some(choice) = queue.first()
    {
        match choice {
            MandatoryChoice::Promote { .. } => {
                let color = match game.state.active_player {
                    Player::North => Color::srgb(0.9, 0.96, 1.0),
                    Player::South => Color::srgb(1.0, 0.78, 0.3),
                };
                commands.spawn((
                    Text2d::new("♕      ♖      ♗      ♘"),
                    TextFont {
                        font: FontSource::Handle(font.0.clone()),
                        font_size: FontSize::Px(25.0),
                        ..default()
                    },
                    TextColor(color),
                    TextLayout::justify(Justify::Center),
                    Transform::from_xyz(
                        0.0,
                        -(f32::from(geometry.board.height) * geometry.tile_size / 2.0) - 56.0,
                        CHOICE_Z,
                    ),
                    Name::new("promotion choice glyphs"),
                    ChessFontText,
                    ChoiceVisual,
                ));
            }
            MandatoryChoice::PlacePawn { legal_squares, .. } => {
                for at in legal_squares {
                    if let Some(world) = geometry.board_to_world(*at) {
                        commands.spawn((
                            Text2d::new("◎"),
                            TextFont {
                                font_size: FontSize::Px(25.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.2, 1.0, 0.65)),
                            TextLayout::justify(Justify::Center),
                            Transform::from_translation(world.extend(CHOICE_Z)),
                            Name::new(format!("legal Pawn placement {at:?}")),
                            ChoiceVisual,
                        ));
                    }
                }
            }
        }
    }
    presentation.revision = Some(game.state.revision);
}

fn choice_description(state: &MatchState) -> Option<String> {
    let TurnPhase::ResolvingChoices { queue } = &state.phase else {
        return None;
    };
    let current = queue.first()?;
    let heading = format!("Mandatory choice 1 of {} remaining", queue.len());
    Some(match current {
        MandatoryChoice::Promote { pawn, .. } => format!(
            "{heading}: promote Pawn {pawn:?}\n[1] ♕ Queen   [2] ♖ Rook   [3] ♗ Bishop   [4] ♘ Knight"
        ),
        MandatoryChoice::PlacePawn {
            settlement_index,
            legal_squares,
        } => format!(
            "{heading}: place produced Pawn for settlement {settlement_index}\n{} legal adjacent squares · arrows cycle · Enter confirms",
            legal_squares.len()
        ),
    })
}

fn clock_description(state: &MatchState) -> Option<String> {
    state.clocks.map(|clocks| {
        format!(
            "Clocks — North {} · South {}",
            format_clock(clocks.north_millis),
            format_clock(clocks.south_millis)
        )
    })
}

fn format_clock(millis: u64) -> String {
    let total_seconds = millis.div_ceil(1_000);
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
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
        let controls = if let Some(choice) = choice_description(&game.state) {
            choice
        } else {
            let hold = match hold_availability(&game.scenario, &game.state) {
                HoldAvailability::Available => "H: Hold (available)",
                HoldAvailability::Disabled(reason) => reason,
            };
            format!("Arrow keys: focus · Enter: select/move · Esc: leave board · {hold}")
        };
        let mut lines = vec![controls];
        if !interaction.status.is_empty() {
            lines.push(interaction.status.clone());
        }
        if let Some(clocks) = clock_description(&game.state) {
            lines.push(clocks);
        }
        text.0 = lines.join("\n");
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

    #[test]
    fn promotion_choice_names_all_four_glyph_controls_and_queue_position() {
        let mut game = game();
        game.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![
                MandatoryChoice::Promote {
                    pawn: PieceId(9),
                    site_index: 0,
                },
                MandatoryChoice::PlacePawn {
                    settlement_index: 1,
                    legal_squares: [Coord::new(2, 2)].into_iter().collect(),
                },
            ],
        };
        let description = choice_description(&game.state).unwrap();
        assert!(description.contains("choice 1 of 2"));
        for label in ["♕ Queen", "♖ Rook", "♗ Bishop", "♘ Knight"] {
            assert!(description.contains(label));
        }
    }

    #[test]
    fn pawn_choice_focus_cycles_only_reducer_provided_squares() {
        let choices = [Coord::new(2, 2), Coord::new(3, 2), Coord::new(4, 2)];
        assert_eq!(
            cycle_choice_focus(&choices, Some(Coord::new(2, 2)), CycleDirection::Previous),
            Some(Coord::new(4, 2))
        );
        assert_eq!(
            cycle_choice_focus(&choices, Some(Coord::new(4, 2)), CycleDirection::Next),
            Some(Coord::new(2, 2))
        );
        assert_eq!(cycle_choice_focus(&[], None, CycleDirection::Next), None);
    }

    #[test]
    fn pawn_choice_spawns_exactly_the_reducer_provided_highlights() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_plugins(crate::rendering::BoardRenderingPlugin)
            .add_plugins(LocalInteractionPlugin);
        app.update();
        let legal_squares: std::collections::BTreeSet<_> =
            [Coord::new(2, 2), Coord::new(3, 2)].into_iter().collect();
        {
            let mut game = app.world_mut().resource_mut::<DisplayedGame>();
            game.state.revision += 1;
            game.state.phase = TurnPhase::ResolvingChoices {
                queue: vec![MandatoryChoice::PlacePawn {
                    settlement_index: 0,
                    legal_squares: legal_squares.clone(),
                }],
            };
        }
        app.update();
        let geometry = *app.world().resource::<BoardGeometry>();
        let mut query = app
            .world_mut()
            .query_filtered::<&Transform, With<ChoiceVisual>>();
        let highlighted: std::collections::BTreeSet<_> = query
            .iter(app.world())
            .filter_map(|transform| geometry.world_to_board(transform.translation.truncate()))
            .collect();
        assert_eq!(highlighted, legal_squares);
    }

    #[test]
    fn active_clocks_have_a_stable_visible_choice_label() {
        let mut game = game();
        game.state.clocks = Some(crownline_core::state::ClockState {
            north_millis: 61_001,
            south_millis: 59_000,
            increment_millis: 2_000,
        });
        assert_eq!(
            clock_description(&game.state).as_deref(),
            Some("Clocks — North 1:02 · South 0:59")
        );
    }
}
