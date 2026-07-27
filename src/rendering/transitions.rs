use std::collections::VecDeque;

use bevy::prelude::*;
use crownline_core::{
    rules::{Transition, TransitionEvent},
    scenario::{PieceKind, Player},
    state::{Action, MatchState, PieceId},
};

use crate::{ChessFontText, config::ClientSettings};

use super::{
    ChessPieceFont, DisplayedGame, FogPresentation, piece_glyph, piece_glyph_vertical_offset,
    player_piece_style,
};

const MOVE_SECONDS: f32 = 0.18;
const GHOST_SECONDS: f32 = 0.16;

#[derive(Debug, Clone, Copy, Component)]
pub(super) struct PiecePresentation {
    pub(super) id: PieceId,
}

#[derive(Component)]
pub(super) struct PieceTween {
    from: Vec2,
    elapsed: f32,
    duration: f32,
}

#[derive(Component)]
pub(super) struct PresentationGhost {
    remaining: f32,
}

#[derive(Debug, Clone, Copy)]
struct MotionRequest {
    id: PieceId,
    offset: Vec2,
}

#[derive(Debug, Clone, Copy)]
struct RetirementRequest {
    id: PieceId,
    kind: PieceKind,
    owner: Player,
    position: Vec3,
}

#[derive(Resource, Default)]
pub(super) struct PresentationMotionQueue {
    movements: Vec<MotionRequest>,
    retirements: Vec<RetirementRequest>,
}

impl PresentationMotionQueue {
    pub(super) fn move_piece(&mut self, id: PieceId, offset: Vec2) {
        self.movements.push(MotionRequest { id, offset });
    }

    pub(super) fn retire(&mut self, id: PieceId, kind: PieceKind, owner: Player, position: Vec3) {
        self.retirements.push(RetirementRequest {
            id,
            kind,
            owner,
            position,
        });
    }
}

#[derive(Resource, Default)]
pub struct PresentationPlayback {
    pub fast_forward: bool,
}

#[derive(Resource, Default)]
pub struct TransitionEventQueue {
    events: VecDeque<TransitionEvent>,
    local_records: VecDeque<LocalTransitionRecord>,
    guided_records: VecDeque<LocalTransitionRecord>,
    local_discontinuity: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalTransitionRecord {
    pub(crate) action: Option<Action>,
    pub(crate) state: MatchState,
    pub(crate) events: Vec<TransitionEvent>,
}

impl TransitionEventQueue {
    pub fn push_transition(&mut self, transition: &Transition) {
        self.events.extend(transition.events.iter().cloned());
    }

    pub(crate) fn push_local_action(&mut self, action: &Action, transition: &Transition) {
        self.push_transition(transition);
        let record = LocalTransitionRecord {
            action: Some(action.clone()),
            state: transition.state.clone(),
            events: transition.events.clone(),
        };
        self.guided_records.push_back(record.clone());
        self.local_records.push_back(record);
    }

    pub(crate) fn push_local_clock(&mut self, transition: &Transition) {
        self.push_transition(transition);
        let record = LocalTransitionRecord {
            action: None,
            state: transition.state.clone(),
            events: transition.events.clone(),
        };
        self.guided_records.push_back(record.clone());
        self.local_records.push_back(record);
    }

    pub(crate) fn drain_local_records(
        &mut self,
    ) -> impl Iterator<Item = LocalTransitionRecord> + '_ {
        self.local_records.drain(..)
    }

    pub(crate) fn drain_guided_records(
        &mut self,
    ) -> impl Iterator<Item = LocalTransitionRecord> + '_ {
        self.guided_records.drain(..)
    }

    pub(crate) fn mark_local_discontinuity(&mut self) {
        self.clear();
        self.local_discontinuity = true;
    }

    pub(crate) fn take_local_discontinuity(&mut self) -> bool {
        std::mem::take(&mut self.local_discontinuity)
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.local_records.clear();
        self.guided_records.clear();
    }
}

#[derive(Resource, Default)]
pub struct TransitionNoticeLog {
    pub entries: Vec<String>,
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn process_piece_motion_requests(
    mut commands: Commands,
    mut requests: ResMut<PresentationMotionQueue>,
    playback: Res<PresentationPlayback>,
    settings: Option<Res<ClientSettings>>,
    font: Res<ChessPieceFont>,
    presentations: Query<(Entity, &PiecePresentation)>,
    game: Res<DisplayedGame>,
    fog: Res<FogPresentation>,
    ghosts: Query<Entity, With<PresentationGhost>>,
    tweens: Query<Entity, With<PieceTween>>,
) {
    if game.scenario.rules.fog.is_some() {
        requests.movements.clear();
        requests.retirements.clear();
        for entity in &ghosts {
            commands.entity(entity).despawn();
        }
        for entity in &tweens {
            commands
                .entity(entity)
                .remove::<PieceTween>()
                .insert(Transform::default());
        }
        if fog.blocks_local_input(&game) {
            return;
        }
    }
    let skip = playback.fast_forward
        || settings
            .as_deref()
            .is_some_and(|settings| settings.reduced_motion);
    for request in requests.movements.drain(..) {
        let Some((entity, _)) = presentations
            .iter()
            .find(|(_, presentation)| presentation.id == request.id)
        else {
            continue;
        };
        if skip {
            commands.entity(entity).insert(Transform::default());
        } else {
            commands.entity(entity).insert((
                Transform::from_xyz(request.offset.x, request.offset.y, 0.0),
                PieceTween {
                    from: request.offset,
                    elapsed: 0.0,
                    duration: MOVE_SECONDS,
                },
            ));
        }
    }
    for request in requests.retirements.drain(..) {
        if !skip {
            spawn_retirement_ghost(&mut commands, &font.0, request);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn animate_piece_presentations(
    mut commands: Commands,
    time: Option<Res<Time>>,
    mut tweens: Query<(Entity, &mut Transform, &mut PieceTween)>,
    mut ghosts: Query<(Entity, &mut PresentationGhost)>,
) {
    let delta = time.as_deref().map_or(1.0 / 60.0, Time::delta_secs);
    for (entity, mut transform, mut tween) in &mut tweens {
        tween.elapsed += delta;
        let t = (tween.elapsed / tween.duration).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        transform.translation = (tween.from * (1.0 - eased)).extend(0.0);
        if t >= 1.0 {
            transform.translation = Vec3::ZERO;
            commands.entity(entity).remove::<PieceTween>();
        }
    }
    for (entity, mut ghost) in &mut ghosts {
        ghost.remaining -= delta;
        if ghost.remaining <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn process_transition_events(
    mut queue: ResMut<TransitionEventQueue>,
    mut log: ResMut<TransitionNoticeLog>,
    game: Res<DisplayedGame>,
) {
    if game.scenario.rules.fog.is_some() {
        if !queue.events.is_empty() {
            queue.events.clear();
            log.entries
                .push("Action resolved; private view updated.".to_owned());
        }
        return;
    }
    log.entries.extend(
        queue
            .events
            .drain(..)
            .map(|event| event_message(&event, &game.state)),
    );
}

fn spawn_retirement_ghost(
    commands: &mut Commands,
    font: &Handle<Font>,
    request: RetirementRequest,
) {
    let (text, backplate, rotation) = player_piece_style(request.owner);
    commands
        .spawn((
            Transform::from_translation(request.position + Vec3::Z * 0.4),
            Visibility::default(),
            PresentationGhost {
                remaining: GHOST_SECONDS,
            },
            Name::new(format!("retired piece {:?}", request.id)),
        ))
        .with_children(|ghost| {
            ghost.spawn((
                Sprite::from_color(backplate, Vec2::splat(super::PIECE_BACKPLATE_SIZE)),
                Transform::from_rotation(rotation),
            ));
            ghost.spawn((
                Text2d::new(piece_glyph(request.kind)),
                TextFont {
                    font: FontSource::Handle(font.clone()),
                    font_size: FontSize::Px(26.0),
                    ..default()
                },
                TextColor(text),
                TextLayout::justify(Justify::Center),
                Transform::from_xyz(0.0, piece_glyph_vertical_offset(request.kind), 1.0),
                ChessFontText,
            ));
        });
}

#[allow(clippy::too_many_lines)]
fn event_message(event: &TransitionEvent, state: &MatchState) -> String {
    match event {
        TransitionEvent::PieceMoved { piece, from, to } => state.pieces.get(piece).map_or_else(
            || format!("Move {from:?} to {to:?}"),
            |piece| {
                format!(
                    "{:?} {:?} moved from {from:?} to {to:?}",
                    piece.owner, piece.kind
                )
            },
        ),
        TransitionEvent::PieceCaptured { at, .. } => format!("Capture at {at:?}"),
        TransitionEvent::TurnHeld { player } => format!("{player:?} held the turn"),
        TransitionEvent::ClockAdvanced {
            player,
            elapsed_millis,
            remaining_millis,
        } => format!(
            "{player:?} clock advanced by {elapsed_millis} ms; {remaining_millis} ms remain"
        ),
        TransitionEvent::ClockIncrementApplied {
            player,
            increment_millis,
            remaining_millis,
        } => format!(
            "{player:?} received {increment_millis} ms increment; {remaining_millis} ms remain"
        ),
        TransitionEvent::PiecePromoted { kind, at, .. } => {
            format!("Promotion to {kind:?} at {at:?}")
        }
        TransitionEvent::PawnProduced {
            settlement_index,
            at,
            ..
        } => format!("Settlement {settlement_index} produced a Pawn at {at:?}"),
        TransitionEvent::SettlementContinuityInterrupted { settlement_index } => {
            format!("Settlement {settlement_index} continuity interrupted")
        }
        TransitionEvent::SettlementCycleStarted {
            settlement_index,
            player,
            previous_continuous,
        } => format!(
            "Settlement {settlement_index} began {player:?} cycle; previous cycle {}",
            if *previous_continuous {
                "was continuous"
            } else {
                "was interrupted"
            }
        ),
        TransitionEvent::SettlementClaimed {
            settlement_index,
            owner,
            ..
        } => format!("{owner:?} claimed settlement {settlement_index}"),
        TransitionEvent::SettlementContested {
            settlement_index, ..
        } => format!("Settlement {settlement_index} contested by a Pawn"),
        TransitionEvent::SettlementTransferCancelled {
            settlement_index, ..
        } => format!("Settlement {settlement_index} transfer cancelled"),
        TransitionEvent::SettlementTransferred {
            settlement_index,
            owner,
            ..
        } => format!("Settlement {settlement_index} transferred to {owner:?}"),
        TransitionEvent::SettlementDevelopmentAdvanced {
            settlement_index,
            progress,
        } => format!("Settlement {settlement_index} development advanced to {progress}"),
        TransitionEvent::SettlementDevelopmentReset { settlement_index } => {
            format!("Settlement {settlement_index} development reset")
        }
        TransitionEvent::SettlementEstablished { settlement_index } => {
            format!("Settlement {settlement_index} established")
        }
        TransitionEvent::SettlementProductionAdvanced {
            settlement_index,
            progress,
        } => format!("Settlement {settlement_index} production advanced to {progress}"),
        TransitionEvent::SettlementProductionReset { settlement_index } => {
            format!("Settlement {settlement_index} production reset")
        }
        TransitionEvent::PawnPlacementReady {
            settlement_index,
            legal_squares,
        } => format!(
            "Settlement {settlement_index} has {} legal Pawn placements",
            legal_squares.len()
        ),
        TransitionEvent::PromotionCandidateStarted { pawn } => {
            format!(
                "{} started promotion progress",
                pawn_description(state, *pawn)
            )
        }
        TransitionEvent::PromotionCandidateAdvanced { pawn, progress } => format!(
            "{} promotion advanced to {progress}",
            pawn_description(state, *pawn)
        ),
        TransitionEvent::PromotionCandidateCancelled { .. } => {
            "A Pawn lost its promotion opportunity".to_owned()
        }
        TransitionEvent::PromotionReady {
            pawn, site_index, ..
        } => format!(
            "{} is ready to promote at site {site_index}",
            pawn_description(state, *pawn)
        ),
        TransitionEvent::DrawOffered { player } => format!("{player:?} offered a draw"),
        TransitionEvent::DrawAnswered { player, accepted } => format!(
            "{player:?} {} the draw",
            if *accepted { "accepted" } else { "declined" }
        ),
        TransitionEvent::TurnStarted {
            player,
            turn_number,
        } => format!("Turn {turn_number}: {player:?}"),
        TransitionEvent::MatchEnded { outcome } => format!("Match ended: {:?}", outcome.reason),
    }
}

fn pawn_description(state: &MatchState, pawn: PieceId) -> String {
    state.pieces.get(&pawn).map_or_else(
        || "Pawn no longer on board".to_owned(),
        |piece| format!("{:?} Pawn at ({}, {})", piece.owner, piece.at.x, piece.at.y),
    )
}

#[cfg(test)]
mod tests {
    use crownline_core::{
        rules::Transition,
        scenario::{Coord, Player},
        state::{MatchOutcome, MatchState, OutcomeReason},
    };

    use super::*;
    use crate::rendering::{BoardRenderingPlugin, DisplayedGame};

    #[test]
    fn event_queue_preserves_transition_order_for_realm_summaries() {
        let transition = Transition {
            state: state_for_queue_test(),
            events: vec![
                TransitionEvent::SettlementEstablished {
                    settlement_index: 2,
                },
                TransitionEvent::PawnProduced {
                    settlement_index: 2,
                    pawn: PieceId(9),
                    at: Coord::new(3, 4),
                },
                TransitionEvent::TurnStarted {
                    player: Player::North,
                    turn_number: 7,
                },
            ],
        };
        let mut queue = TransitionEventQueue::default();
        queue.push_transition(&transition);
        let messages: Vec<_> = queue
            .events
            .iter()
            .map(|event| event_message(event, &transition.state))
            .collect();
        assert_eq!(messages[0], "Settlement 2 established");
        assert_eq!(
            messages[1],
            "Settlement 2 produced a Pawn at Coord { x: 3, y: 4 }"
        );
        assert_eq!(messages[2], "Turn 7: North");
    }

    #[test]
    fn local_actions_are_mirrored_to_independent_playtest_and_guidance_consumers() {
        let transition = Transition {
            state: state_for_queue_test(),
            events: vec![TransitionEvent::TurnHeld {
                player: Player::South,
            }],
        };
        let action = Action::Hold {
            player: Player::South,
        };
        let mut queue = TransitionEventQueue::default();
        queue.push_local_action(&action, &transition);

        let playtest = queue.drain_local_records().collect::<Vec<_>>();
        assert_eq!(playtest.len(), 1);
        let guided = queue.drain_guided_records().collect::<Vec<_>>();
        assert_eq!(guided.len(), 1);
        assert_eq!(guided[0].action, Some(action));
        assert_eq!(guided[0].events, transition.events);
    }

    #[test]
    fn history_messages_distinguish_hold_draw_promotion_production_and_terminal_events() {
        let events = [
            TransitionEvent::TurnHeld {
                player: Player::South,
            },
            TransitionEvent::DrawOffered {
                player: Player::North,
            },
            TransitionEvent::PiecePromoted {
                pawn: PieceId(1),
                promoted: PieceId(2),
                kind: PieceKind::Queen,
                at: Coord::new(4, 4),
            },
            TransitionEvent::PawnProduced {
                settlement_index: 3,
                pawn: PieceId(4),
                at: Coord::new(5, 5),
            },
            TransitionEvent::MatchEnded {
                outcome: MatchOutcome {
                    winner: None,
                    reason: OutcomeReason::AgreedDraw,
                },
            },
        ];
        let state = state_for_queue_test();
        let messages: Vec<_> = events
            .iter()
            .map(|event| event_message(event, &state))
            .collect();
        for expected in [
            "held",
            "offered a draw",
            "Promotion",
            "produced",
            "Match ended",
        ] {
            assert!(messages.iter().any(|message| message.contains(expected)));
        }
    }

    #[test]
    fn motion_math_reaches_authoritative_parent_without_overshoot() {
        let from = Vec2::new(-64.0, 32.0);
        for t in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let eased = 1.0 - (1.0 - t) * (1.0 - t);
            let offset = from * (1.0 - eased);
            assert!(offset.length() <= from.length());
        }
    }

    #[test]
    fn fast_forward_keeps_canonical_parent_at_destination_and_skips_tween() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let piece_id = app
            .world()
            .resource::<DisplayedGame>()
            .state
            .pieces
            .keys()
            .next()
            .copied()
            .unwrap();
        app.world_mut()
            .resource_mut::<PresentationPlayback>()
            .fast_forward = true;
        app.world_mut()
            .resource_mut::<DisplayedGame>()
            .state
            .pieces
            .get_mut(&piece_id)
            .unwrap()
            .at = Coord::new(0, 10);
        app.update();

        let world = app.world_mut();
        let mut presentations =
            world.query::<(&PiecePresentation, &Transform, Option<&PieceTween>)>();
        let (_, transform, tween) = presentations
            .iter(world)
            .find(|(presentation, _, _)| presentation.id == piece_id)
            .unwrap();
        assert!(transform.translation.length_squared() < f32::EPSILON);
        assert!(tween.is_none());
    }

    #[test]
    fn reduced_motion_skips_interpolation_but_preserves_ordered_feedback() {
        let mut app = App::new();
        app.insert_resource(ClientSettings {
            reduced_motion: true,
            ..ClientSettings::default()
        });
        app.add_plugins(BoardRenderingPlugin);
        app.update();

        let (piece_id, kind, owner, position) = {
            let world = app.world_mut();
            let mut presentations = world.query::<(&PiecePresentation, &Transform)>();
            let (presentation, transform) = presentations.iter(world).next().unwrap();
            let piece = world
                .resource::<DisplayedGame>()
                .state
                .pieces
                .get(&presentation.id)
                .unwrap();
            (
                presentation.id,
                piece.kind,
                piece.owner,
                transform.translation,
            )
        };
        {
            let mut motion = app.world_mut().resource_mut::<PresentationMotionQueue>();
            motion.move_piece(piece_id, Vec2::new(32.0, -16.0));
            motion.retire(piece_id, kind, owner, position);
        }
        let events = vec![
            TransitionEvent::SettlementEstablished {
                settlement_index: 2,
            },
            TransitionEvent::PawnProduced {
                settlement_index: 2,
                pawn: PieceId(9),
                at: Coord::new(3, 4),
            },
            TransitionEvent::TurnStarted {
                player: Player::North,
                turn_number: 7,
            },
        ];
        let state = state_for_queue_test();
        let expected: Vec<_> = events
            .iter()
            .map(|event| event_message(event, &state))
            .collect();
        app.world_mut()
            .resource_mut::<TransitionEventQueue>()
            .push_transition(&Transition {
                state: state_for_queue_test(),
                events,
            });
        app.update();

        let world = app.world_mut();
        let mut presentations =
            world.query::<(&PiecePresentation, &Transform, Option<&PieceTween>)>();
        let (_, transform, tween) = presentations
            .iter(world)
            .find(|(presentation, _, _)| presentation.id == piece_id)
            .unwrap();
        assert!(transform.translation.length_squared() < f32::EPSILON);
        assert!(tween.is_none());
        let mut ghosts = world.query::<&PresentationGhost>();
        assert_eq!(ghosts.iter(world).count(), 0);
        assert_eq!(
            world.resource::<TransitionNoticeLog>().entries,
            expected,
            "reduced motion must retain canonical transition order"
        );
    }

    fn state_for_queue_test() -> MatchState {
        let scenario: crownline_core::ScenarioDefinition =
            ron::from_str(include_str!("../../assets/scenarios/standard.ron")).unwrap();
        MatchState::from_scenario(&scenario).unwrap()
    }
}
