use bevy::prelude::*;
use crownline_core::{
    Action, apply_timed_action, is_in_check, legal_mandatory_choice_actions, legal_moves,
    scenario::{BoardSize, Coord},
    state::{MandatoryChoice, MatchState, PieceId, PromotionEligibility, PromotionKind, TurnPhase},
};

use crate::{
    lifecycle::{ClientFlow, LocalSetup},
    online_connection::{OnlineActionIntent, OnlineIntentOutbox},
    panels::PanelSurface,
    rendering::{
        DisplayedGame, FogPresentation, HoveredBoardSquare, LocalTransitionEventQueue,
        LocalTransitionNoticeLog, OverlaySelection, PointerCapture, coordinates::BoardGeometry,
    },
    ui_layout::SIDE_REGION_PERCENT,
};

const FOCUS_Z: f32 = 6.0;
const CHOICE_Z: f32 = 5.8;

#[derive(Debug, Resource, Default)]
pub(crate) struct BoardInteraction {
    keyboard_focus: Option<Coord>,
    observed_revision: Option<u64>,
    status: String,
    submitting: bool,
}

impl BoardInteraction {
    pub(crate) fn resolve_online(&mut self, status: &str) {
        self.submitting = false;
        status.clone_into(&mut self.status);
    }

    pub(crate) fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }
}

#[derive(Component)]
struct KeyboardFocusVisual;

#[derive(Component)]
struct InteractionHelpText;

#[derive(Component)]
struct PromotionChoiceRow;

#[derive(Component, Clone, Copy)]
struct PromotionChoiceButton(PromotionKind);

#[derive(Component)]
struct PromotionChoiceButtonText(PromotionKind);

#[derive(Resource, Default)]
struct PromotionPointerIntent(Option<PromotionKind>);

type FocusAffordanceQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut Visibility),
    (With<KeyboardFocusVisual>, Without<InteractionHelpText>),
>;

type HelpAffordanceQuery<'w, 's> =
    Query<'w, 's, &'static mut Text, (With<InteractionHelpText>, Without<KeyboardFocusVisual>)>;

#[derive(Component)]
struct ChoiceVisual;

#[derive(Resource, Default)]
struct ChoicePresentation {
    key: Option<ChoicePresentationKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChoicePresentationKey {
    scenario_id: String,
    revision: u64,
    phase: TurnPhase,
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
            .init_resource::<PromotionPointerIntent>()
            .add_systems(Startup, spawn_interaction_affordances)
            .add_systems(
                Update,
                (
                    collect_promotion_pointer_intent,
                    handle_board_input,
                    sync_choice_affordances,
                    sync_interaction_affordances,
                    sync_promotion_choice_buttons,
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
        Text::new("Arrow keys: focus board - Enter: select/move - Esc: leave board - H: Hold"),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.91, 0.95)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            left: percent(SIDE_REGION_PERCENT),
            right: percent(SIDE_REGION_PERCENT),
            bottom: px(46),
            padding: UiRect::axes(px(6), px(3)),
            ..default()
        },
        GlobalZIndex(10),
        Name::new("local interaction help"),
        InteractionHelpText,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(SIDE_REGION_PERCENT),
                right: percent(SIDE_REGION_PERCENT),
                bottom: px(7),
                height: px(34),
                column_gap: px(4),
                display: Display::None,
                ..default()
            },
            GlobalZIndex(12),
            Name::new("promotion choice buttons"),
            PromotionChoiceRow,
        ))
        .with_children(|row| {
            for (kind, key) in [
                (PromotionKind::Queen, "1"),
                (PromotionKind::Rook, "2"),
                (PromotionKind::Bishop, "3"),
                (PromotionKind::Knight, "4"),
            ] {
                row.spawn((
                    Button,
                    Node {
                        width: percent(25),
                        height: percent(100),
                        padding: UiRect::axes(px(3), px(2)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.12, 0.2, 0.24)),
                    PanelSurface,
                    PromotionChoiceButton(kind),
                    Name::new(format!("promotion {kind:?} button")),
                    children![(
                        Text::new(format!("[{key}] {kind:?}")),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.9, 0.92, 0.96)),
                        TextLayout::justify(Justify::Center),
                        PromotionChoiceButtonText(kind),
                    )],
                ));
            }
        });
}

fn collect_promotion_pointer_intent(
    buttons: Query<(&Interaction, &PromotionChoiceButton), Changed<Interaction>>,
    mut intent: ResMut<PromotionPointerIntent>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            intent.0 = Some(button.0);
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]
fn handle_board_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    hovered: Res<HoveredBoardSquare>,
    capture: Res<PointerCapture>,
    mut game: ResMut<DisplayedGame>,
    mut selection: ResMut<OverlaySelection>,
    mut interaction: ResMut<BoardInteraction>,
    mut transitions: ResMut<LocalTransitionEventQueue>,
    mut online_outbox: Option<ResMut<OnlineIntentOutbox>>,
    mut promotion_pointer: ResMut<PromotionPointerIntent>,
    flow: Option<Res<ClientFlow>>,
    setup: Option<Res<LocalSetup>>,
    fog: Res<FogPresentation>,
) {
    let online = flow
        .as_deref()
        .is_some_and(|flow| *flow == ClientFlow::OnlinePlaying);
    if flow
        .as_deref()
        .is_some_and(|flow| !matches!(*flow, ClientFlow::Playing | ClientFlow::OnlinePlaying))
    {
        selection.piece = None;
        return;
    }
    if !online && (fog.blocks_local_input(&game) || fog.confirmed_this_frame()) {
        selection.piece = None;
        interaction.keyboard_focus = None;
        promotion_pointer.0 = None;
        "Private handoff: board input and clocks are paused.".clone_into(&mut interaction.status);
        return;
    }
    if !online
        && setup.as_deref().is_some_and(|setup| {
            setup
                .controller(game.state.active_player)
                .profile()
                .is_some()
        })
    {
        selection.piece = None;
        interaction.keyboard_focus = None;
        promotion_pointer.0 = None;
        return;
    }
    if interaction.observed_revision != Some(game.state.revision) {
        selection.piece = None;
        interaction.observed_revision = Some(game.state.revision);
        if !online || !online_outbox.as_deref().is_some_and(|outbox| outbox.locked) {
            interaction.submitting = false;
        }
    }
    if online && online_outbox.as_deref().is_some_and(|outbox| outbox.locked) {
        selection.piece = None;
        "Command pending authoritative acknowledgement.".clone_into(&mut interaction.status);
        return;
    }

    if let TurnPhase::ResolvingChoices { queue } = &game.state.phase {
        let current_choice = queue.first().cloned().map(|choice| match choice {
            MandatoryChoice::PlacePawn {
                settlement_index,
                legal_squares,
            } => MandatoryChoice::PlacePawn {
                settlement_index,
                legal_squares: fog.view().map_or(legal_squares, |view| {
                    view.placement_intent_candidates(settlement_index)
                }),
            },
            promotion @ MandatoryChoice::Promote { .. } => promotion,
        });
        if let Some(choice) = current_choice {
            handle_mandatory_choice(
                &keys,
                &mouse,
                hovered.0,
                capture.ui_has_pointer,
                promotion_pointer.0.take(),
                choice,
                &mut game,
                &mut selection,
                &mut interaction,
                &mut transitions,
                online.then_some(online_outbox.as_deref_mut()).flatten(),
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
            online.then_some(online_outbox.as_deref_mut()).flatten(),
        );
        return;
    }

    let activated = activated_square(
        &keys,
        &mouse,
        interaction.keyboard_focus,
        hovered.0,
        capture.ui_has_pointer,
    );
    let Some(at) = activated else {
        return;
    };
    interaction.keyboard_focus = Some(at);

    match presented_activation_for(&game, &fog, selection.piece, at) {
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
            online.then_some(online_outbox.as_deref_mut()).flatten(),
        ),
        Activation::Clear => {
            selection.piece = None;
            "Selection cleared.".clone_into(&mut interaction.status);
        }
    }
}

fn activated_square(
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    keyboard_focus: Option<Coord>,
    hovered: Option<Coord>,
    ui_has_pointer: bool,
) -> Option<Coord> {
    if keys.just_pressed(KeyCode::Enter) {
        keyboard_focus
    } else if mouse.just_pressed(MouseButton::Left) && !ui_has_pointer {
        hovered
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_mandatory_choice(
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    hovered: Option<Coord>,
    ui_has_pointer: bool,
    pointer_promotion: Option<PromotionKind>,
    choice: MandatoryChoice,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    interaction: &mut BoardInteraction,
    transitions: &mut LocalTransitionEventQueue,
    mut online_outbox: Option<&mut OnlineIntentOutbox>,
) {
    selection.piece = None;
    if keys.just_pressed(KeyCode::KeyH) {
        "Hold disabled: resolve the mandatory choice first.".clone_into(&mut interaction.status);
        return;
    }
    match choice {
        MandatoryChoice::Promote {
            pawn, eligibility, ..
        } => {
            interaction.keyboard_focus = None;
            let promotion = pointer_promotion.or_else(|| {
                if keys.just_pressed(KeyCode::Digit1) {
                    Some(PromotionKind::Queen)
                } else if keys.just_pressed(KeyCode::Digit2) {
                    Some(PromotionKind::Rook)
                } else if keys.just_pressed(KeyCode::Digit3) {
                    Some(PromotionKind::Bishop)
                } else if keys.just_pressed(KeyCode::Digit4) {
                    Some(PromotionKind::Knight)
                } else {
                    None
                }
            });
            if let Some(promote_to) = promotion {
                if !promotion_is_legal(&game.state, pawn, promote_to) {
                    let required = promotion_required_score(&game.scenario, promote_to);
                    format!(
                        "{:?} is locked for this batch: frozen score {}, requires {required}.",
                        promote_to,
                        eligibility.control.total()
                    )
                    .clone_into(&mut interaction.status);
                    return;
                }
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
                    online_outbox.as_deref_mut(),
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
                        online_outbox,
                    );
                } else {
                    "Choose one of the highlighted legal Pawn squares."
                        .clone_into(&mut interaction.status);
                }
            }
        }
    }
}

fn promotion_is_legal(state: &MatchState, pawn: PieceId, kind: PromotionKind) -> bool {
    legal_mandatory_choice_actions(state).iter().any(|action| {
        matches!(
            action,
            Action::ChoosePromotion {
                pawn: candidate,
                promote_to,
                ..
            } if *candidate == pawn && *promote_to == kind
        )
    })
}

const fn promotion_required_score(
    scenario: &crownline_core::ScenarioDefinition,
    kind: PromotionKind,
) -> u32 {
    match kind {
        PromotionKind::Knight => 0,
        PromotionKind::Bishop => scenario.rules.promotion_unlocks.bishop,
        PromotionKind::Rook => scenario.rules.promotion_unlocks.rook,
        PromotionKind::Queen => scenario.rules.promotion_unlocks.queen,
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

fn presented_activation_for(
    game: &DisplayedGame,
    fog: &FogPresentation,
    selected: Option<PieceId>,
    at: Coord,
) -> Activation {
    let Some(view) = fog.view() else {
        return activation_for(game, selected, at);
    };
    if let Some(piece) = view
        .pieces
        .values()
        .find(|piece| piece.at == at && piece.owner == view.seat)
    {
        return Activation::Select(piece.id);
    }
    if let Some(piece) = selected
        && view.intent_candidates(piece).contains(&at)
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
    online_outbox: Option<&mut OnlineIntentOutbox>,
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
            online_outbox,
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
    online_outbox: Option<&mut OnlineIntentOutbox>,
) {
    if interaction.submitting {
        return;
    }
    interaction.submitting = true;
    if let Some(outbox) = online_outbox {
        if outbox.locked {
            return;
        }
        outbox.locked = true;
        outbox.intent = Some(OnlineActionIntent {
            action: action.clone(),
            expected_revision: game.state.revision,
        });
        selection.piece = None;
        "Command pending authoritative acknowledgement.".clone_into(&mut interaction.status);
        return;
    }
    match apply_timed_action(&game.scenario, &game.state, action, 0) {
        Ok(transition) => {
            transitions.push_local_action(action, &transition);
            game.state = transition.state;
            interaction.observed_revision = Some(game.state.revision);
            selection.piece = None;
            "Command accepted.".clone_into(&mut interaction.status);
        }
        Err(error) => {
            interaction.status = if game.scenario.rules.fog.is_some() {
                "Command rejected: illegal intent.".to_owned()
            } else {
                format!("Command rejected: {error}")
            };
        }
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
    mut presentation: ResMut<ChoicePresentation>,
    existing: Query<Entity, With<ChoiceVisual>>,
    fog: Res<FogPresentation>,
) {
    let key = ChoicePresentationKey {
        scenario_id: game.state.scenario_id.clone(),
        revision: game.state.revision,
        phase: game.state.phase.clone(),
    };
    if presentation.key.as_ref() == Some(&key) {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    if let TurnPhase::ResolvingChoices { queue } = &game.state.phase
        && let Some(choice) = queue.first()
    {
        match choice {
            MandatoryChoice::Promote { .. } => {}
            MandatoryChoice::PlacePawn {
                settlement_index,
                legal_squares,
            } => {
                let safe_squares = fog.view().map_or_else(
                    || legal_squares.clone(),
                    |view| view.placement_intent_candidates(*settlement_index),
                );
                for at in safe_squares {
                    if let Some(world) = geometry.board_to_world(at) {
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
    presentation.key = Some(key);
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
fn sync_promotion_choice_buttons(
    game: Res<DisplayedGame>,
    mut row: Query<&mut Node, With<PromotionChoiceRow>>,
    mut buttons: Query<
        (&PromotionChoiceButton, &mut BackgroundColor),
        Without<PromotionChoiceButtonText>,
    >,
    mut labels: Query<(&PromotionChoiceButtonText, &mut Text)>,
) {
    let promotion = match &game.state.phase {
        TurnPhase::ResolvingChoices { queue } => match queue.first() {
            Some(MandatoryChoice::Promote {
                pawn, eligibility, ..
            }) => Some((*pawn, eligibility)),
            _ => None,
        },
        TurnPhase::Command => None,
    };
    let Ok(mut row) = row.single_mut() else {
        return;
    };
    row.display = if promotion.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    let Some((pawn, eligibility)) = promotion else {
        return;
    };
    for (button, mut background) in &mut buttons {
        *background = if promotion_is_legal(&game.state, pawn, button.0) {
            BackgroundColor(Color::srgb(0.08, 0.3, 0.2))
        } else {
            BackgroundColor(Color::srgb(0.26, 0.12, 0.14))
        };
    }
    for (label, mut text) in &mut labels {
        let key = promotion_binding(label.0);
        let required = promotion_required_score(&game.scenario, label.0);
        let state = if eligibility.allows(label.0) {
            if label.0 == PromotionKind::Knight {
                "READY".to_owned()
            } else {
                format!("READY >={required}")
            }
        } else {
            format!("LOCK >={required}")
        };
        text.0 = format!("[{key}] {:?}\n{state}", label.0);
    }
}

const fn promotion_binding(kind: PromotionKind) -> &'static str {
    match kind {
        PromotionKind::Queen => "1",
        PromotionKind::Rook => "2",
        PromotionKind::Bishop => "3",
        PromotionKind::Knight => "4",
    }
}

fn choice_description(
    scenario: &crownline_core::ScenarioDefinition,
    state: &MatchState,
) -> Option<String> {
    let TurnPhase::ResolvingChoices { queue } = &state.phase else {
        return None;
    };
    let current = queue.first()?;
    let heading = format!("Mandatory choice 1 of {} remaining", queue.len());
    Some(match current {
        MandatoryChoice::Promote {
            pawn, eligibility, ..
        } => promotion_choice_description(scenario, state, &heading, *pawn, eligibility),
        MandatoryChoice::PlacePawn {
            settlement_index,
            legal_squares,
        } => format!(
            "{heading}: place produced Pawn for settlement {settlement_index}\n{} legal adjacent squares - arrows cycle - Enter confirms",
            legal_squares.len()
        ),
    })
}

fn promotion_choice_description(
    scenario: &crownline_core::ScenarioDefinition,
    state: &MatchState,
    heading: &str,
    pawn: PieceId,
    eligibility: &PromotionEligibility,
) -> String {
    let control = eligibility.control;
    let score = control.total();
    let next = [
        (PromotionKind::Bishop, "Bishop"),
        (PromotionKind::Rook, "Rook"),
        (PromotionKind::Queen, "Queen"),
    ]
    .into_iter()
    .find(|(kind, _)| !eligibility.allows(*kind))
    .map_or_else(
        || "all recruits unlocked".to_owned(),
        |(kind, name)| {
            let required = promotion_required_score(scenario, kind);
            format!(
                "next {name} at {required} ({} needed)",
                required.saturating_sub(score)
            )
        },
    );
    let available = legal_mandatory_choice_actions(state)
        .into_iter()
        .filter_map(|action| match action {
            Action::ChoosePromotion { promote_to, .. } => Some(format!("{promote_to:?}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ");
    let options = [
        PromotionKind::Queen,
        PromotionKind::Rook,
        PromotionKind::Bishop,
        PromotionKind::Knight,
    ]
    .into_iter()
    .map(|kind| {
        let key = promotion_binding(kind);
        let required = promotion_required_score(scenario, kind);
        let status = if eligibility.allows(kind) {
            "READY".to_owned()
        } else {
            format!("LOCK >={required}")
        };
        format!("[{key}] {kind:?} {status}")
    })
    .collect::<Vec<_>>()
    .join(" | ");
    let pawn = state.pieces.get(&pawn).map_or_else(
        || "Pawn no longer on board".to_owned(),
        |piece| format!("{:?} Pawn at ({}, {})", piece.owner, piece.at.x, piece.at.y),
    );
    format!(
        "{heading}: promote {pawn} - BATCH SNAPSHOT score {score}\nOwned {} + governed {} + established {}x2; {next}. Available: {available}\n{options}",
        control.owned_settlements, control.governed_settlements, control.established_settlements,
    )
}

fn clock_description(state: &MatchState) -> Option<String> {
    state.clocks.map(|clocks| {
        format!(
            "Clocks - North {} - South {}",
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
    history: Res<LocalTransitionNoticeLog>,
    mut focus: FocusAffordanceQuery,
    mut help: HelpAffordanceQuery,
    fog: Res<FogPresentation>,
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
    if let Ok(mut text) = help.single_mut() {
        text.0 = if fog.blocks_local_input(&game) || fog.confirmed_this_frame() {
            "Private handoff: press Enter when the next player is ready. Board input and clocks are paused."
                .to_owned()
        } else if let Some(view) = fog.view() {
            let controls = "Arrow keys: focus - Enter: select/move - Esc: leave board - H: Hold";
            let mut lines = vec![controls.to_owned()];
            if !interaction.status.is_empty() {
                lines.push(interaction.status.clone());
            }
            if let Some(clocks) = view.clocks {
                lines.push(format!(
                    "Clocks - North {} - South {}",
                    format_clock(clocks.north_millis),
                    format_clock(clocks.south_millis)
                ));
            }
            lines.join("\n")
        } else {
            interaction_affordance_text(&game, &interaction, &history)
        };
    }
}

fn interaction_affordance_text(
    game: &DisplayedGame,
    interaction: &BoardInteraction,
    history: &LocalTransitionNoticeLog,
) -> String {
    let controls = if let Some(choice) = choice_description(&game.scenario, &game.state) {
        choice
    } else {
        let hold = match hold_availability(&game.scenario, &game.state) {
            HoldAvailability::Available => "H: Hold (available)",
            HoldAvailability::Disabled(reason) => reason,
        };
        format!("Arrow keys: focus - Enter: select/move - Esc: leave board - {hold}")
    };
    let mut lines = vec![controls];
    if !interaction.status.is_empty() {
        lines.push(interaction.status.clone());
    }
    if let Some(clocks) = clock_description(&game.state) {
        lines.push(clocks);
    }
    if let Some(latest) = history.entries.last() {
        lines.push(format!("Latest: {latest}"));
    }
    lines.join("\n")
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
    fn interaction_log_is_screen_space_in_the_reserved_bottom_region() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_interaction_affordances);
        app.update();
        let world = app.world_mut();
        let entity = world
            .query_filtered::<Entity, With<InteractionHelpText>>()
            .single(world)
            .unwrap();
        let node = world.get::<Node>(entity).unwrap();
        assert_eq!(node.left, percent(SIDE_REGION_PERCENT));
        assert_eq!(node.right, percent(SIDE_REGION_PERCENT));
        assert!(world.get::<Text>(entity).is_some());
        assert!(world.get::<Text2d>(entity).is_none());

        let history = LocalTransitionNoticeLog {
            entries: vec!["South Pawn moved to h12".to_owned()],
        };
        assert!(
            interaction_affordance_text(&game(), &BoardInteraction::default(), &history)
                .contains("Latest: South Pawn moved to h12")
        );
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
            None,
        );
        assert_eq!(game.state.revision, 1);
        assert_eq!(selection.piece, None);
        assert!(!interaction.submitting);
    }

    #[test]
    fn online_submission_locks_one_intent_without_mutating_canonical_state() {
        let mut game = game();
        let original = game.state.clone();
        let legal = legal_moves(&game.scenario, &game.state).unwrap().remove(0);
        let action = Action::Move {
            player: game.state.active_player,
            piece: legal.piece,
            to: legal.to,
        };
        let mut selection = OverlaySelection {
            piece: Some(legal.piece),
        };
        let mut interaction = BoardInteraction::default();
        let mut transitions = LocalTransitionEventQueue::default();
        let mut outbox = OnlineIntentOutbox::default();

        submit_action(
            &action,
            &mut game,
            &mut selection,
            &mut interaction,
            &mut transitions,
            Some(&mut outbox),
        );
        submit_action(
            &Action::Hold {
                player: game.state.active_player,
            },
            &mut game,
            &mut selection,
            &mut interaction,
            &mut transitions,
            Some(&mut outbox),
        );

        assert_eq!(game.state, original);
        assert_eq!(selection.piece, None);
        assert!(interaction.submitting);
        assert!(outbox.locked);
        let intent = outbox.intent.unwrap();
        assert_eq!(intent.action, action);
        assert_eq!(intent.expected_revision, original.revision);
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
    fn ui_captured_pointer_input_never_activates_a_board_command() {
        let keys = ButtonInput::<KeyCode>::default();
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        let hovered = Some(Coord::new(3, 4));

        assert_eq!(
            activated_square(&keys, &mouse, None, hovered, true),
            None,
            "a UI-owned click must not reach board selection or submission"
        );
        assert_eq!(
            activated_square(&keys, &mouse, None, hovered, false),
            hovered,
            "the same uncaptured click remains a board activation"
        );
    }

    #[test]
    fn promotion_choice_names_all_four_controls_and_queue_position() {
        let mut game = game();
        game.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![
                MandatoryChoice::Promote {
                    pawn: PieceId(9),
                    site_index: 0,
                    eligibility: crownline_core::PromotionEligibility::default(),
                },
                MandatoryChoice::PlacePawn {
                    settlement_index: 1,
                    legal_squares: [Coord::new(2, 2)].into_iter().collect(),
                },
            ],
        };
        let description = choice_description(&game.scenario, &game.state).unwrap();
        assert!(description.contains("choice 1 of 2"));
        for label in ["[1] Queen", "[2] Rook", "[3] Bishop", "[4] Knight"] {
            assert!(description.contains(label));
        }
        assert!(description.contains("BATCH SNAPSHOT score 0"));
        assert!(description.contains("Owned 0 + governed 0 + established 0x2"));
        assert!(description.contains("next Bishop at 2 (2 needed)"));
        assert!(description.contains("Available: Knight"));
    }

    #[test]
    fn locked_promotion_key_reports_feedback_without_emitting_action() {
        let mut game = game();
        let pawn = *game.state.pieces.keys().next().unwrap();
        let choice = MandatoryChoice::Promote {
            pawn,
            site_index: 0,
            eligibility: PromotionEligibility::default(),
        };
        game.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![choice.clone()],
        };
        let before = game.state.clone();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Digit1);
        let mouse = ButtonInput::default();
        let mut selection = OverlaySelection::default();
        let mut interaction = BoardInteraction::default();
        let mut transitions = LocalTransitionEventQueue::default();

        handle_mandatory_choice(
            &keys,
            &mouse,
            None,
            false,
            None,
            choice,
            &mut game,
            &mut selection,
            &mut interaction,
            &mut transitions,
            None,
        );

        assert_eq!(game.state, before);
        assert!(interaction.status.contains("Queen is locked"));
        assert!(interaction.status.contains("frozen score 0, requires 8"));
        assert_eq!(transitions.drain_local_records().count(), 0);
    }

    #[test]
    fn promotion_buttons_share_bindings_thresholds_and_reserved_layout() {
        let mut game = game();
        game.scenario.rules.promotion_unlocks = crownline_core::PromotionUnlockRules {
            bishop: 3,
            rook: 6,
            queen: 9,
        };
        let pawn = *game.state.pieces.keys().next().unwrap();
        game.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn,
                site_index: 0,
                eligibility: PromotionEligibility::from_control(
                    crownline_core::RealmControlScore {
                        owned_settlements: 2,
                        governed_settlements: 2,
                        established_settlements: 0,
                    },
                    game.scenario.rules.promotion_unlocks,
                ),
            }],
        };
        let mut app = App::new();
        app.insert_resource(game)
            .add_systems(Startup, spawn_interaction_affordances)
            .add_systems(Update, sync_promotion_choice_buttons);
        app.update();

        let world = app.world_mut();
        let row = world
            .query_filtered::<(&Node, &Name), With<PromotionChoiceRow>>()
            .single(world)
            .unwrap();
        assert_eq!(row.0.left, percent(SIDE_REGION_PERCENT));
        assert_eq!(row.0.right, percent(SIDE_REGION_PERCENT));
        assert_eq!(row.0.bottom, px(7));
        assert_eq!(row.0.height, px(34));
        assert_eq!(row.0.display, Display::Flex);

        let labels = world
            .query::<(&PromotionChoiceButtonText, &Text)>()
            .iter(world)
            .map(|(_, text)| text.0.clone())
            .collect::<Vec<_>>();
        assert!(labels.iter().any(|text| text == "[1] Queen\nLOCK >=9"));
        assert!(labels.iter().any(|text| text == "[2] Rook\nLOCK >=6"));
        assert!(labels.iter().any(|text| text == "[3] Bishop\nREADY >=3"));
        assert!(labels.iter().any(|text| text == "[4] Knight\nREADY"));
    }

    #[test]
    fn choice_projection_key_changes_for_equal_revision_reconnect_state() {
        let mut game = game();
        let command_key = ChoicePresentationKey {
            scenario_id: game.state.scenario_id.clone(),
            revision: game.state.revision,
            phase: game.state.phase.clone(),
        };
        game.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn: game.state.pieces.keys().next().copied().unwrap(),
                site_index: 0,
                eligibility: crownline_core::PromotionEligibility::default(),
            }],
        };
        let restored_choice_key = ChoicePresentationKey {
            scenario_id: game.state.scenario_id.clone(),
            revision: game.state.revision,
            phase: game.state.phase.clone(),
        };

        assert_ne!(command_key, restored_choice_key);
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
            Some("Clocks - North 1:02 - South 0:59")
        );
    }
}
