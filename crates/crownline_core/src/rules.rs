use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    scenario::{Coord, Edge, EdgeKind, PieceKind, Player, ScenarioDefinition, TileTerrain},
    state::{
        Action, EnPassantState, MandatoryChoice, MatchOutcome, MatchState, OutcomeReason, Piece,
        PieceId, PieceOrigin, PromotionKind, TransitionError, TurnPhase,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveKind {
    Quiet,
    Capture { captured: PieceId },
    EnPassant { captured: PieceId },
    Castle,
    PawnDoubleStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LegalMove {
    pub piece: PieceId,
    pub from: Coord,
    pub to: Coord,
    pub kind: MoveKind,
}

/// One geometric attack, with a path containing both attacker and target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AttackLine {
    pub attacker: PieceId,
    pub path: Vec<Coord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernanceReport {
    pub settlement_index: u16,
    pub owner: Option<Player>,
    pub governors: Vec<AttackLine>,
    pub blocked: Vec<BlockedGovernanceLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedGovernanceLine {
    pub candidate: PieceId,
    pub path: Vec<Coord>,
    pub blocker: GovernanceBlocker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceBlocker {
    Piece { piece: PieceId, at: Coord },
    Terrain { at: Coord, terrain: TileTerrain },
    Edge { edge: Edge, kind: EdgeKind },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub state: MatchState,
    pub events: Vec<TransitionEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionEvent {
    PieceMoved {
        piece: PieceId,
        from: Coord,
        to: Coord,
    },
    PieceCaptured {
        piece: PieceId,
        at: Coord,
    },
    TurnHeld {
        player: Player,
    },
    PiecePromoted {
        pawn: PieceId,
        promoted: PieceId,
        kind: PieceKind,
        at: Coord,
    },
    PawnProduced {
        settlement_index: u16,
        pawn: PieceId,
        at: Coord,
    },
    SettlementContinuityInterrupted {
        settlement_index: u16,
    },
    SettlementCycleStarted {
        settlement_index: u16,
        player: Player,
        previous_continuous: bool,
    },
    SettlementClaimed {
        settlement_index: u16,
        owner: Player,
        founder: PieceId,
    },
    SettlementContested {
        settlement_index: u16,
        candidate: PieceId,
    },
    SettlementTransferCancelled {
        settlement_index: u16,
        candidate: PieceId,
    },
    SettlementTransferred {
        settlement_index: u16,
        previous_owner: Player,
        owner: Player,
        founder: PieceId,
    },
    SettlementDevelopmentAdvanced {
        settlement_index: u16,
        progress: u8,
    },
    SettlementDevelopmentReset {
        settlement_index: u16,
    },
    SettlementEstablished {
        settlement_index: u16,
    },
    SettlementProductionAdvanced {
        settlement_index: u16,
        progress: u8,
    },
    SettlementProductionReset {
        settlement_index: u16,
    },
    PawnPlacementReady {
        settlement_index: u16,
        legal_squares: BTreeSet<Coord>,
    },
    PromotionCandidateStarted {
        pawn: PieceId,
    },
    PromotionCandidateAdvanced {
        pawn: PieceId,
        progress: u8,
    },
    PromotionCandidateCancelled {
        pawn: PieceId,
    },
    PromotionReady {
        pawn: PieceId,
        site_index: u16,
    },
    DrawOffered {
        player: Player,
    },
    DrawAnswered {
        player: Player,
        accepted: bool,
    },
    TurnStarted {
        player: Player,
        turn_number: u64,
    },
    MatchEnded {
        outcome: MatchOutcome,
    },
}

/// Returns every legal board move for the active player in stable order.
///
/// # Errors
///
/// Returns an invariant error when canonical state is internally inconsistent.
pub fn legal_moves(
    scenario: &ScenarioDefinition,
    state: &MatchState,
) -> Result<Vec<LegalMove>, TransitionError> {
    state.validate_invariants()?;
    if state.outcome.is_some() || !matches!(state.phase, TurnPhase::Command) {
        return Ok(Vec::new());
    }

    let board = Board::new(scenario, state);
    let mut moves = Vec::new();
    for piece in state
        .pieces
        .values()
        .filter(|piece| piece.owner == state.active_player)
    {
        for candidate in board.pseudo_legal_moves(piece) {
            if move_preserves_king(scenario, state, candidate)? {
                moves.push(candidate);
            }
        }
    }
    moves.sort_unstable();
    Ok(moves)
}

/// Reports whether `player`'s King is geometrically attacked.
///
/// # Errors
///
/// Returns an invariant error if that player has no King in canonical state.
pub fn is_in_check(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
) -> Result<bool, TransitionError> {
    let king = state
        .pieces
        .values()
        .find(|piece| piece.owner == player && piece.kind == PieceKind::King)
        .ok_or(TransitionError::MissingKing(player))?;
    Ok(Board::new(scenario, state).is_square_attacked(king.at, player.opponent()))
}

/// Returns geometric attackers and their unblocked paths in stable piece order.
///
/// Friendly-occupied targets are included because they are protected even
/// though the attacker cannot land there. King-safety filtering is not applied.
///
/// # Errors
///
/// Returns typed scenario, state, scenario-identity, or target-bound errors.
pub fn attack_lines_on(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    target: Coord,
    by: Player,
) -> Result<Vec<AttackLine>, TransitionError> {
    scenario
        .validate()
        .map_err(TransitionError::InvalidScenario)?;
    state.validate_invariants()?;
    if state.scenario_id != scenario.id {
        return Err(TransitionError::ScenarioMismatch {
            expected: state.scenario_id.clone(),
            actual: scenario.id.clone(),
        });
    }
    if !target.is_within(scenario.board) {
        return Err(TransitionError::CoordinateOutOfBounds(target));
    }
    let board = Board::new(scenario, state);
    Ok(state
        .pieces
        .values()
        .filter(|piece| piece.owner == by)
        .filter_map(|piece| {
            board.attack_path_to(piece, target).map(|path| AttackLine {
                attacker: piece.id,
                path,
            })
        })
        .collect())
}

/// Resolves geometric major-piece governance and blocked candidate lines for
/// one settlement.
///
/// # Errors
///
/// Returns typed scenario, state, identity, or settlement-index errors.
pub fn governance_report(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    settlement_index: u16,
) -> Result<GovernanceReport, TransitionError> {
    scenario
        .validate()
        .map_err(TransitionError::InvalidScenario)?;
    state.validate_invariants()?;
    if state.scenario_id != scenario.id {
        return Err(TransitionError::ScenarioMismatch {
            expected: state.scenario_id.clone(),
            actual: scenario.id.clone(),
        });
    }
    let settlement = state
        .settlements
        .iter()
        .find(|settlement| settlement.site_index == settlement_index)
        .ok_or(TransitionError::MissingSettlement(settlement_index))?;
    let target = scenario
        .settlements
        .get(usize::from(settlement_index))
        .ok_or(TransitionError::MissingSettlement(settlement_index))?
        .at;
    let mut report = GovernanceReport {
        settlement_index,
        owner: settlement.owner,
        governors: Vec::new(),
        blocked: Vec::new(),
    };
    let Some(owner) = settlement.owner else {
        return Ok(report);
    };
    let board = Board::new(scenario, state);
    for piece in state.pieces.values().filter(|piece| {
        piece.owner == owner
            && matches!(
                piece.kind,
                PieceKind::King | PieceKind::Queen | PieceKind::Rook | PieceKind::Bishop
            )
    }) {
        match board.governance_trace(piece, target) {
            Some(GovernanceTrace::Clear(path)) => report.governors.push(AttackLine {
                attacker: piece.id,
                path,
            }),
            Some(GovernanceTrace::Blocked { path, blocker }) => {
                report.blocked.push(BlockedGovernanceLine {
                    candidate: piece.id,
                    path,
                    blocker,
                });
            }
            None => {}
        }
    }
    Ok(report)
}

/// Returns every currently legal adjacent square for a ready settlement Pawn.
///
/// # Errors
///
/// Returns typed scenario, state, identity, settlement, or readiness errors.
pub fn pawn_placement_squares(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    settlement_index: u16,
) -> Result<BTreeSet<Coord>, TransitionError> {
    scenario
        .validate()
        .map_err(TransitionError::InvalidScenario)?;
    state.validate_invariants()?;
    if state.scenario_id != scenario.id {
        return Err(TransitionError::ScenarioMismatch {
            expected: state.scenario_id.clone(),
            actual: scenario.id.clone(),
        });
    }
    let settlement = state
        .settlements
        .iter()
        .find(|settlement| settlement.site_index == settlement_index)
        .ok_or(TransitionError::MissingSettlement(settlement_index))?;
    if !settlement.established
        || settlement.owner.is_none()
        || settlement.produced_pawn.is_some()
        || settlement.production_progress < scenario.rules.production_cycles
    {
        return Err(TransitionError::SettlementCannotProduce(settlement_index));
    }
    Ok(pawn_placement_squares_unchecked(
        scenario,
        state,
        settlement_index,
    ))
}

/// Applies one action transactionally through the canonical rules engine.
///
/// # Errors
///
/// Returns a typed legality or invariant error and leaves the input unchanged.
pub fn apply_action(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    action: &Action,
) -> Result<Transition, TransitionError> {
    scenario
        .validate()
        .map_err(TransitionError::InvalidScenario)?;
    if state.scenario_id != scenario.id {
        return Err(TransitionError::ScenarioMismatch {
            expected: state.scenario_id.clone(),
            actual: scenario.id.clone(),
        });
    }
    if state.outcome.is_some() {
        return Err(TransitionError::MatchFinished);
    }

    let mut next = match *action {
        Action::Move { player, piece, to } => apply_move(scenario, state, player, piece, to),
        Action::Hold { player } => apply_hold(scenario, state, player),
        Action::Resign { .. } | Action::OfferDraw { .. } | Action::RespondToDraw { .. } => {
            state.apply_non_board_action(action)
        }
        Action::ChoosePromotion {
            player,
            pawn,
            promote_to,
        } => apply_promotion_choice(scenario, state, player, pawn, promote_to),
        Action::PlacePawn {
            player,
            settlement_index,
            at,
        } => apply_pawn_placement(state, player, settlement_index, at),
    }?;
    apply_settlement_landing_effect(scenario, &mut next, action)?;
    apply_promotion_landing_effect(scenario, &mut next, action);
    cancel_invalid_transfer_candidates(scenario, &mut next)?;
    cancel_invalid_promotion_candidates(scenario, &mut next);
    if state.active_player != next.active_player && next.outcome.is_none() {
        let active_player = next.active_player;
        resolve_transfer_candidates(scenario, &mut next, active_player)?;
        advance_promotion_candidates(scenario, &mut next, active_player)?;
    }
    latch_settlement_interruptions(scenario, &mut next)?;
    if state.active_player != next.active_player && next.outcome.is_none() {
        let owner = next.active_player;
        complete_owner_cycles(scenario, &mut next, owner);
        sort_mandatory_choices(scenario, &mut next);
        latch_settlement_interruptions(scenario, &mut next)?;
        record_repetition(&mut next)?;
    }
    let events = transition_events(state, &next, action);
    Ok(Transition {
        state: next,
        events,
    })
}

fn apply_promotion_choice(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
    pawn_id: PieceId,
    promote_to: PromotionKind,
) -> Result<MatchState, TransitionError> {
    ensure_choice_actor(state, player)?;
    let queue = match &state.phase {
        TurnPhase::ResolvingChoices { queue } => queue,
        TurnPhase::Command => return Err(TransitionError::WrongTurnPhase),
    };
    if !matches!(queue.first(), Some(MandatoryChoice::Promote { pawn, .. }) if *pawn == pawn_id) {
        return Err(TransitionError::ChoiceDoesNotMatch);
    }
    let pawn = state
        .pieces
        .get(&pawn_id)
        .filter(|piece| piece.owner == player && piece.kind == PieceKind::Pawn)
        .cloned()
        .ok_or(TransitionError::InvalidPromotionPawn(pawn_id))?;

    let mut next = state.clone();
    next.pieces.remove(&pawn_id);
    next.promotion_candidates.remove(&pawn_id);
    clear_piece_references(&mut next, pawn_id);
    let promoted_id = allocate_piece_id(&mut next)?;
    next.pieces.insert(
        promoted_id,
        Piece {
            id: promoted_id,
            owner: player,
            kind: promotion_piece_kind(promote_to),
            at: pawn.at,
            origin: PieceOrigin::Promoted { from: pawn_id },
            has_moved: true,
        },
    );
    complete_first_choice(&mut next)?;
    finish_non_command_transition(&mut next)?;
    next.validate_invariants()?;
    if is_in_check(scenario, &next, player)? {
        return Err(TransitionError::PromotionLeavesKingInCheck);
    }
    Ok(next)
}

fn apply_pawn_placement(
    state: &MatchState,
    player: Player,
    settlement_index: u16,
    at: Coord,
) -> Result<MatchState, TransitionError> {
    ensure_choice_actor(state, player)?;
    let legal_squares = match &state.phase {
        TurnPhase::ResolvingChoices { queue } => match queue.first() {
            Some(MandatoryChoice::PlacePawn {
                settlement_index: pending,
                legal_squares,
            }) if *pending == settlement_index => legal_squares,
            _ => return Err(TransitionError::ChoiceDoesNotMatch),
        },
        TurnPhase::Command => return Err(TransitionError::WrongTurnPhase),
    };
    if !legal_squares.contains(&at) {
        return Err(TransitionError::IllegalPawnPlacement {
            settlement_index,
            at,
        });
    }
    if state.pieces.values().any(|piece| piece.at == at) {
        return Err(TransitionError::DuplicateOccupancy(at));
    }
    let settlement_position = state
        .settlements
        .iter()
        .position(|settlement| settlement.site_index == settlement_index)
        .ok_or(TransitionError::MissingSettlement(settlement_index))?;
    if state.settlements[settlement_position].owner != Some(player)
        || state.settlements[settlement_position]
            .produced_pawn
            .is_some()
    {
        return Err(TransitionError::SettlementCannotProduce(settlement_index));
    }

    let mut next = state.clone();
    let pawn_id = allocate_piece_id(&mut next)?;
    next.pieces.insert(
        pawn_id,
        Piece {
            id: pawn_id,
            owner: player,
            kind: PieceKind::Pawn,
            at,
            origin: PieceOrigin::Settlement { settlement_index },
            has_moved: false,
        },
    );
    next.settlements[settlement_position].produced_pawn = Some(pawn_id);
    next.settlements[settlement_position].production_progress = 0;
    complete_first_choice(&mut next)?;
    finish_non_command_transition(&mut next)?;
    next.validate_invariants()?;
    Ok(next)
}

fn ensure_choice_actor(state: &MatchState, player: Player) -> Result<(), TransitionError> {
    if player != state.active_player {
        return Err(TransitionError::WrongPlayer {
            expected: state.active_player,
            actual: player,
        });
    }
    if state.outcome.is_some() {
        return Err(TransitionError::MatchFinished);
    }
    Ok(())
}

fn complete_first_choice(state: &mut MatchState) -> Result<(), TransitionError> {
    let TurnPhase::ResolvingChoices { queue } = &mut state.phase else {
        return Err(TransitionError::WrongTurnPhase);
    };
    if queue.is_empty() {
        return Err(TransitionError::ChoiceDoesNotMatch);
    }
    queue.remove(0);
    if queue.is_empty() {
        state.phase = TurnPhase::Command;
    }
    Ok(())
}

fn finish_non_command_transition(state: &mut MatchState) -> Result<(), TransitionError> {
    state.revision = state
        .revision
        .checked_add(1)
        .ok_or(TransitionError::RevisionOverflow)?;
    Ok(())
}

fn allocate_piece_id(state: &mut MatchState) -> Result<PieceId, TransitionError> {
    let id = PieceId(state.next_piece_id);
    state.next_piece_id = state
        .next_piece_id
        .checked_add(1)
        .ok_or(TransitionError::PieceIdOverflow)?;
    Ok(id)
}

const fn promotion_piece_kind(kind: PromotionKind) -> PieceKind {
    match kind {
        PromotionKind::Queen => PieceKind::Queen,
        PromotionKind::Rook => PieceKind::Rook,
        PromotionKind::Bishop => PieceKind::Bishop,
        PromotionKind::Knight => PieceKind::Knight,
    }
}

fn transition_events(
    before: &MatchState,
    after: &MatchState,
    action: &Action,
) -> Vec<TransitionEvent> {
    let mut events = match *action {
        Action::Move { piece, .. } => move_transition_events(before, after, piece),
        Action::Hold { player } => vec![TransitionEvent::TurnHeld { player }],
        Action::ChoosePromotion {
            pawn, promote_to, ..
        } => promotion_transition_events(before, after, pawn, promote_to),
        Action::PlacePawn {
            settlement_index,
            at,
            ..
        } => placement_transition_events(after, settlement_index, at),
        Action::OfferDraw { player } => vec![TransitionEvent::DrawOffered { player }],
        Action::RespondToDraw { player, accept } => {
            vec![TransitionEvent::DrawAnswered {
                player,
                accepted: accept,
            }]
        }
        Action::Resign { .. } => Vec::new(),
    };
    events.extend(settlement_change_events(before, after));
    events.extend(promotion_candidate_events(before, after, action));
    events.extend(new_mandatory_choice_events(before, after));
    if before.active_player != after.active_player && after.outcome.is_none() {
        for settlement in after
            .settlements
            .iter()
            .filter(|settlement| settlement.owner == Some(after.active_player))
        {
            events.push(TransitionEvent::SettlementCycleStarted {
                settlement_index: settlement.site_index,
                player: after.active_player,
                previous_continuous: settlement.completed_cycle_continuous,
            });
        }
        events.push(TransitionEvent::TurnStarted {
            player: after.active_player,
            turn_number: after.turn_number,
        });
    }
    if before.outcome.is_none()
        && let Some(outcome) = after.outcome
    {
        events.push(TransitionEvent::MatchEnded { outcome });
    }
    events
}

fn settlement_change_events(before: &MatchState, after: &MatchState) -> Vec<TransitionEvent> {
    let mut events = Vec::new();
    for settlement_after in &after.settlements {
        if let Some(settlement_before) = before
            .settlements
            .iter()
            .find(|settlement| settlement.site_index == settlement_after.site_index)
            && !settlement_before.cycle_interrupted
            && settlement_after.cycle_interrupted
        {
            events.push(TransitionEvent::SettlementContinuityInterrupted {
                settlement_index: settlement_after.site_index,
            });
        }
        if let Some(settlement_before) = before
            .settlements
            .iter()
            .find(|settlement| settlement.site_index == settlement_after.site_index)
        {
            match (settlement_before.owner, settlement_after.owner) {
                (None, Some(owner)) => {
                    if let Some(founder) = settlement_after.founder {
                        events.push(TransitionEvent::SettlementClaimed {
                            settlement_index: settlement_after.site_index,
                            owner,
                            founder,
                        });
                    }
                }
                (Some(previous_owner), Some(owner)) if previous_owner != owner => {
                    if let Some(founder) = settlement_after.founder {
                        events.push(TransitionEvent::SettlementTransferred {
                            settlement_index: settlement_after.site_index,
                            previous_owner,
                            owner,
                            founder,
                        });
                    }
                }
                _ => {}
            }
            match (
                settlement_before.transfer_candidate,
                settlement_after.transfer_candidate,
            ) {
                (None, Some(candidate)) => {
                    events.push(TransitionEvent::SettlementContested {
                        settlement_index: settlement_after.site_index,
                        candidate,
                    });
                }
                (Some(candidate), None) if settlement_before.owner == settlement_after.owner => {
                    events.push(TransitionEvent::SettlementTransferCancelled {
                        settlement_index: settlement_after.site_index,
                        candidate,
                    });
                }
                _ => {}
            }
            if settlement_before.establishment_progress != settlement_after.establishment_progress {
                if settlement_after.establishment_progress == 0 {
                    events.push(TransitionEvent::SettlementDevelopmentReset {
                        settlement_index: settlement_after.site_index,
                    });
                } else {
                    events.push(TransitionEvent::SettlementDevelopmentAdvanced {
                        settlement_index: settlement_after.site_index,
                        progress: settlement_after.establishment_progress,
                    });
                }
            }
            if !settlement_before.established && settlement_after.established {
                events.push(TransitionEvent::SettlementEstablished {
                    settlement_index: settlement_after.site_index,
                });
            }
            if settlement_before.production_progress != settlement_after.production_progress {
                if settlement_after.production_progress == 0 {
                    events.push(TransitionEvent::SettlementProductionReset {
                        settlement_index: settlement_after.site_index,
                    });
                } else {
                    events.push(TransitionEvent::SettlementProductionAdvanced {
                        settlement_index: settlement_after.site_index,
                        progress: settlement_after.production_progress,
                    });
                }
            }
        }
    }
    events
}

fn new_mandatory_choice_events(before: &MatchState, after: &MatchState) -> Vec<TransitionEvent> {
    let before_queue = match &before.phase {
        TurnPhase::ResolvingChoices { queue } => queue.as_slice(),
        TurnPhase::Command => &[],
    };
    let after_queue = match &after.phase {
        TurnPhase::ResolvingChoices { queue } => queue.as_slice(),
        TurnPhase::Command => &[],
    };
    after_queue
        .iter()
        .filter(|choice| !before_queue.contains(choice))
        .map(|choice| match choice {
            MandatoryChoice::PlacePawn {
                settlement_index,
                legal_squares,
            } => TransitionEvent::PawnPlacementReady {
                settlement_index: *settlement_index,
                legal_squares: legal_squares.clone(),
            },
            MandatoryChoice::Promote { pawn, site_index } => TransitionEvent::PromotionReady {
                pawn: *pawn,
                site_index: *site_index,
            },
        })
        .collect()
}

fn promotion_candidate_events(
    before: &MatchState,
    after: &MatchState,
    action: &Action,
) -> Vec<TransitionEvent> {
    let promoted = match action {
        Action::ChoosePromotion { pawn, .. } => Some(*pawn),
        _ => None,
    };
    let mut events = Vec::new();
    for (pawn, progress) in &after.promotion_candidates {
        match before.promotion_candidates.get(pawn) {
            None => events.push(TransitionEvent::PromotionCandidateStarted { pawn: *pawn }),
            Some(previous) if previous != progress => {
                events.push(TransitionEvent::PromotionCandidateAdvanced {
                    pawn: *pawn,
                    progress: *progress,
                });
            }
            Some(_) => {}
        }
    }
    for pawn in before.promotion_candidates.keys() {
        if !after.promotion_candidates.contains_key(pawn) && promoted != Some(*pawn) {
            events.push(TransitionEvent::PromotionCandidateCancelled { pawn: *pawn });
        }
    }
    events
}

fn apply_settlement_landing_effect(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    action: &Action,
) -> Result<(), TransitionError> {
    let Action::Move { piece, .. } = *action else {
        return Ok(());
    };
    let Some(pawn) = state
        .pieces
        .get(&piece)
        .filter(|piece| piece.kind == PieceKind::Pawn)
        .cloned()
    else {
        return Ok(());
    };
    let Some((index, _)) = scenario
        .settlements
        .iter()
        .enumerate()
        .find(|(_, site)| site.at == pawn.at)
    else {
        return Ok(());
    };
    let settlement_index = u16::try_from(index).map_err(|_| TransitionError::TooManySites)?;
    let settlement = state
        .settlements
        .iter_mut()
        .find(|settlement| settlement.site_index == settlement_index)
        .ok_or(TransitionError::MissingSettlement(settlement_index))?;
    match settlement.owner {
        None => {
            settlement.owner = Some(pawn.owner);
            settlement.founder = Some(pawn.id);
            settlement.establishment_progress = 0;
            settlement.established = false;
            settlement.production_progress = 0;
            settlement.produced_pawn = None;
            settlement.cycle_interrupted = false;
            settlement.completed_cycle_continuous = false;
            settlement.transfer_candidate = None;
        }
        Some(owner) if owner != pawn.owner => {
            settlement.transfer_candidate = Some(pawn.id);
            settlement.cycle_interrupted = true;
        }
        Some(_) => {}
    }
    Ok(())
}

fn apply_promotion_landing_effect(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    action: &Action,
) {
    let Action::Move { piece, .. } = *action else {
        return;
    };
    let Some(pawn) = state
        .pieces
        .get(&piece)
        .filter(|piece| piece.kind == PieceKind::Pawn)
    else {
        return;
    };
    if scenario
        .promotion_sites
        .iter()
        .any(|site| site.at == pawn.at)
    {
        state.promotion_candidates.insert(piece, 0);
    }
}

fn cancel_invalid_promotion_candidates(scenario: &ScenarioDefinition, state: &mut MatchState) {
    let pieces = &state.pieces;
    state.promotion_candidates.retain(|pawn, _| {
        pieces.get(pawn).is_some_and(|piece| {
            piece.kind == PieceKind::Pawn
                && scenario
                    .promotion_sites
                    .iter()
                    .any(|site| site.at == piece.at)
        })
    });
}

fn advance_promotion_candidates(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    active_player: Player,
) -> Result<(), TransitionError> {
    let pieces = &state.pieces;
    for (pawn, progress) in &mut state.promotion_candidates {
        if pieces
            .get(pawn)
            .is_some_and(|piece| piece.owner == active_player)
            && *progress < scenario.rules.promotion_cycles
        {
            *progress = progress
                .checked_add(1)
                .ok_or(TransitionError::PromotionProgressOverflow)?;
        }
    }
    let choices: Vec<_> = state
        .promotion_candidates
        .iter()
        .filter(|(_, progress)| **progress >= scenario.rules.promotion_cycles)
        .filter_map(|(pawn, _)| {
            let piece = state.pieces.get(pawn)?;
            if piece.owner != active_player {
                return None;
            }
            let site_index = scenario
                .promotion_sites
                .iter()
                .position(|site| site.at == piece.at)
                .and_then(|index| u16::try_from(index).ok())?;
            Some(MandatoryChoice::Promote {
                pawn: *pawn,
                site_index,
            })
        })
        .collect();
    append_mandatory_choices(state, choices);
    Ok(())
}

fn append_mandatory_choices(state: &mut MatchState, choices: Vec<MandatoryChoice>) {
    if choices.is_empty() {
        return;
    }
    match &mut state.phase {
        TurnPhase::ResolvingChoices { queue } => queue.extend(choices),
        TurnPhase::Command => {
            state.phase = TurnPhase::ResolvingChoices { queue: choices };
        }
    }
}

fn sort_mandatory_choices(scenario: &ScenarioDefinition, state: &mut MatchState) {
    let TurnPhase::ResolvingChoices { queue } = &mut state.phase else {
        return;
    };
    queue.sort_by_key(|choice| match choice {
        MandatoryChoice::Promote { pawn, site_index } => (
            scenario
                .promotion_sites
                .get(usize::from(*site_index))
                .map_or(Coord::new(u16::MAX, u16::MAX), |site| site.at),
            0_u8,
            pawn.0,
        ),
        MandatoryChoice::PlacePawn {
            settlement_index, ..
        } => (
            scenario
                .settlements
                .get(usize::from(*settlement_index))
                .map_or(Coord::new(u16::MAX, u16::MAX), |site| site.at),
            1,
            u32::from(*settlement_index),
        ),
    });
}

fn cancel_invalid_transfer_candidates(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
) -> Result<(), TransitionError> {
    let pieces = &state.pieces;
    for settlement in &mut state.settlements {
        let Some(candidate) = settlement.transfer_candidate else {
            continue;
        };
        let site = scenario
            .settlements
            .get(usize::from(settlement.site_index))
            .ok_or(TransitionError::MissingSettlement(settlement.site_index))?;
        let valid = pieces.get(&candidate).is_some_and(|piece| {
            piece.kind == PieceKind::Pawn
                && piece.at == site.at
                && settlement.owner.is_some_and(|owner| owner != piece.owner)
        });
        if !valid {
            settlement.transfer_candidate = None;
        }
    }
    Ok(())
}

fn resolve_transfer_candidates(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    active_player: Player,
) -> Result<(), TransitionError> {
    let pieces = &state.pieces;
    for settlement in &mut state.settlements {
        let Some(candidate) = settlement.transfer_candidate else {
            continue;
        };
        let site = scenario
            .settlements
            .get(usize::from(settlement.site_index))
            .ok_or(TransitionError::MissingSettlement(settlement.site_index))?;
        let survives = pieces.get(&candidate).is_some_and(|piece| {
            piece.owner == active_player && piece.kind == PieceKind::Pawn && piece.at == site.at
        });
        if survives {
            settlement.owner = Some(active_player);
            settlement.founder = Some(candidate);
            settlement.transfer_candidate = None;
            settlement.cycle_interrupted = true;
            settlement.completed_cycle_continuous = false;
        }
    }
    Ok(())
}

fn latch_settlement_interruptions(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
) -> Result<(), TransitionError> {
    let indices: Vec<_> = state
        .settlements
        .iter()
        .filter(|settlement| settlement.owner.is_some() && !settlement.cycle_interrupted)
        .map(|settlement| settlement.site_index)
        .collect();
    for index in indices {
        if !settlement_cycle_requirements_met(scenario, state, index)?
            && let Some(settlement) = state
                .settlements
                .iter_mut()
                .find(|settlement| settlement.site_index == index)
        {
            settlement.cycle_interrupted = true;
        }
    }
    Ok(())
}

fn settlement_cycle_requirements_met(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    settlement_index: u16,
) -> Result<bool, TransitionError> {
    let settlement = state
        .settlements
        .iter()
        .find(|settlement| settlement.site_index == settlement_index)
        .ok_or(TransitionError::MissingSettlement(settlement_index))?;
    let Some(owner) = settlement.owner else {
        return Ok(false);
    };
    let founder_present = settlement.founder.is_some_and(|founder| {
        state
            .pieces
            .get(&founder)
            .is_some_and(|piece| piece.owner == owner && piece.kind == PieceKind::Pawn)
    });
    let site = scenario
        .settlements
        .get(usize::from(settlement_index))
        .ok_or(TransitionError::MissingSettlement(settlement_index))?;
    let enemy_occupies = state
        .pieces
        .values()
        .any(|piece| piece.at == site.at && piece.owner != owner);
    let governed = !governance_report(scenario, state, settlement_index)?
        .governors
        .is_empty();
    Ok(founder_present && !enemy_occupies && governed)
}

fn complete_owner_cycles(scenario: &ScenarioDefinition, state: &mut MatchState, owner: Player) {
    let previously_established: BTreeSet<_> = state
        .settlements
        .iter()
        .filter(|settlement| settlement.established)
        .map(|settlement| settlement.site_index)
        .collect();
    for settlement in &mut state.settlements {
        if settlement.owner == Some(owner) {
            let continuous = !settlement.cycle_interrupted;
            settlement.completed_cycle_continuous = continuous;
            if !settlement.established {
                if continuous {
                    settlement.establishment_progress = settlement
                        .establishment_progress
                        .saturating_add(1)
                        .min(scenario.rules.establishment_cycles);
                    if settlement.establishment_progress == scenario.rules.establishment_cycles {
                        settlement.established = true;
                    }
                } else if scenario.rules.development_resets_when_interrupted {
                    settlement.establishment_progress = 0;
                }
            }
            if previously_established.contains(&settlement.site_index)
                && settlement.produced_pawn.is_none()
                && settlement.production_progress < scenario.rules.production_cycles
            {
                if continuous {
                    settlement.production_progress = settlement
                        .production_progress
                        .saturating_add(1)
                        .min(scenario.rules.production_cycles);
                } else if scenario.rules.development_resets_when_interrupted {
                    settlement.production_progress = 0;
                }
            }
            settlement.cycle_interrupted = false;
        }
    }

    let choices: Vec<_> = state
        .settlements
        .iter()
        .filter(|settlement| {
            settlement.owner == Some(owner)
                && settlement.established
                && settlement.produced_pawn.is_none()
                && settlement.production_progress >= scenario.rules.production_cycles
        })
        .filter_map(|settlement| {
            let legal_squares =
                pawn_placement_squares_unchecked(scenario, state, settlement.site_index);
            (!legal_squares.is_empty()).then_some(MandatoryChoice::PlacePawn {
                settlement_index: settlement.site_index,
                legal_squares,
            })
        })
        .collect();
    append_mandatory_choices(state, choices);
}

fn pawn_placement_squares_unchecked(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    settlement_index: u16,
) -> BTreeSet<Coord> {
    let Some(settlement) = state
        .settlements
        .iter()
        .find(|settlement| settlement.site_index == settlement_index)
    else {
        return BTreeSet::new();
    };
    let (Some(owner), Some(site)) = (
        settlement.owner,
        scenario.settlements.get(usize::from(settlement_index)),
    ) else {
        return BTreeSet::new();
    };
    let board = Board::new(scenario, state);
    let source = Piece {
        id: PieceId(u32::MAX),
        owner,
        kind: PieceKind::Pawn,
        at: site.at,
        origin: PieceOrigin::Deployed,
        has_moved: true,
    };
    KING_OFFSETS
        .into_iter()
        .filter_map(|offset| board.step(&source, offset))
        .filter(|at| !board.occupancy.contains_key(at))
        .collect()
}

fn move_transition_events(
    before: &MatchState,
    after: &MatchState,
    moving_piece: PieceId,
) -> Vec<TransitionEvent> {
    let mut events = Vec::new();
    for (id, piece_before) in &before.pieces {
        match after.pieces.get(id) {
            Some(piece_after) if piece_after.at != piece_before.at => {
                events.push(TransitionEvent::PieceMoved {
                    piece: *id,
                    from: piece_before.at,
                    to: piece_after.at,
                });
            }
            None if *id != moving_piece => events.push(TransitionEvent::PieceCaptured {
                piece: *id,
                at: piece_before.at,
            }),
            _ => {}
        }
    }
    events
}

fn promotion_transition_events(
    before: &MatchState,
    after: &MatchState,
    pawn: PieceId,
    promote_to: PromotionKind,
) -> Vec<TransitionEvent> {
    let Some(pawn_before) = before.pieces.get(&pawn) else {
        return Vec::new();
    };
    after
        .pieces
        .values()
        .find(|piece| piece.origin == (PieceOrigin::Promoted { from: pawn }))
        .map(|piece| {
            vec![TransitionEvent::PiecePromoted {
                pawn,
                promoted: piece.id,
                kind: promotion_piece_kind(promote_to),
                at: pawn_before.at,
            }]
        })
        .unwrap_or_default()
}

fn placement_transition_events(
    after: &MatchState,
    settlement_index: u16,
    at: Coord,
) -> Vec<TransitionEvent> {
    after
        .pieces
        .values()
        .find(|piece| {
            piece.at == at
                && piece.origin == (PieceOrigin::Settlement { settlement_index })
                && piece.kind == PieceKind::Pawn
        })
        .map(|piece| {
            vec![TransitionEvent::PawnProduced {
                settlement_index,
                pawn: piece.id,
                at,
            }]
        })
        .unwrap_or_default()
}

fn apply_move(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
    piece_id: PieceId,
    to: Coord,
) -> Result<MatchState, TransitionError> {
    ensure_command_actor(state, player)?;
    let candidate = legal_moves(scenario, state)?
        .into_iter()
        .find(|candidate| candidate.piece == piece_id && candidate.to == to)
        .ok_or(TransitionError::IllegalMove {
            piece: piece_id,
            to,
        })?;

    let mut next = state.clone();
    apply_move_unchecked(scenario, &mut next, candidate)?;
    finish_command(scenario, &mut next, player)?;
    Ok(next)
}

fn apply_hold(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
) -> Result<MatchState, TransitionError> {
    ensure_command_actor(state, player)?;
    if is_in_check(scenario, state, player)? {
        return Err(TransitionError::CannotHoldInCheck);
    }
    let mut next = state.clone();
    next.en_passant = None;
    finish_command(scenario, &mut next, player)?;
    Ok(next)
}

fn ensure_command_actor(state: &MatchState, player: Player) -> Result<(), TransitionError> {
    if player != state.active_player {
        return Err(TransitionError::WrongPlayer {
            expected: state.active_player,
            actual: player,
        });
    }
    if !matches!(state.phase, TurnPhase::Command) {
        return Err(TransitionError::WrongTurnPhase);
    }
    Ok(())
}

fn finish_command(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    player: Player,
) -> Result<(), TransitionError> {
    if state
        .outstanding_draw_offer
        .is_some_and(|offering| offering != player)
    {
        state.outstanding_draw_offer = None;
    }
    state.active_player = player.opponent();
    state.turn_number = state
        .turn_number
        .checked_add(1)
        .ok_or(TransitionError::TurnOverflow)?;
    state.revision = state
        .revision
        .checked_add(1)
        .ok_or(TransitionError::RevisionOverflow)?;
    state.phase = TurnPhase::Command;
    state.validate_invariants()?;

    if is_in_check(scenario, state, state.active_player)?
        && legal_moves(scenario, state)?.is_empty()
    {
        state.outcome = Some(MatchOutcome {
            winner: Some(player),
            reason: OutcomeReason::Checkmate,
        });
        return Ok(());
    }

    Ok(())
}

fn record_repetition(state: &mut MatchState) -> Result<(), TransitionError> {
    let repetition_key = state.repetition_key()?;
    let count = state.repetition_counts.entry(repetition_key).or_default();
    *count = count.saturating_add(1);
    if *count >= 3 {
        state.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::ThreefoldRepetition,
        });
    }
    Ok(())
}

fn move_preserves_king(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    candidate: LegalMove,
) -> Result<bool, TransitionError> {
    let mut simulated = state.clone();
    apply_move_unchecked(scenario, &mut simulated, candidate)?;
    is_in_check(scenario, &simulated, state.active_player).map(|check| !check)
}

fn apply_move_unchecked(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    candidate: LegalMove,
) -> Result<(), TransitionError> {
    let moving = state
        .pieces
        .get(&candidate.piece)
        .cloned()
        .ok_or(TransitionError::MissingPiece(candidate.piece))?;
    state.en_passant = None;

    match candidate.kind {
        MoveKind::Capture { captured } | MoveKind::EnPassant { captured } => {
            let captured_piece = state
                .pieces
                .get(&captured)
                .cloned()
                .ok_or(TransitionError::MissingPiece(captured))?;
            if captured_piece.kind == PieceKind::King {
                return Err(TransitionError::CannotCaptureKing);
            }
            state.pieces.remove(&captured);
            clear_piece_references(state, captured);
            remove_castling_right_for_rook(
                scenario,
                state,
                captured_piece.owner,
                captured_piece.at,
            );
        }
        MoveKind::Castle => apply_castling_rook(scenario, state, &moving, candidate.to)?,
        MoveKind::Quiet | MoveKind::PawnDoubleStep => {}
    }

    let piece = state
        .pieces
        .get_mut(&candidate.piece)
        .ok_or(TransitionError::MissingPiece(candidate.piece))?;
    piece.at = candidate.to;
    piece.has_moved = true;

    if moving.kind == PieceKind::King {
        state.available_castling_routes.retain(|route_id| {
            scenario
                .castling_routes
                .iter()
                .find(|route| route.id == *route_id)
                .is_none_or(|route| route.player != moving.owner)
        });
    } else if moving.kind == PieceKind::Rook {
        remove_castling_right_for_rook(scenario, state, moving.owner, moving.at);
    }

    if candidate.kind == MoveKind::PawnDoubleStep {
        let direction = scenario.rules.pawn_forward_y[&moving.owner];
        let capture_y =
            offset_axis(moving.at.y, direction).ok_or(TransitionError::IllegalMove {
                piece: moving.id,
                to: candidate.to,
            })?;
        state.en_passant = Some(EnPassantState {
            pawn: moving.id,
            capture_destination: Coord::new(moving.at.x, capture_y),
            expires_for: moving.owner.opponent(),
        });
    }
    Ok(())
}

fn apply_castling_rook(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    king: &Piece,
    destination: Coord,
) -> Result<(), TransitionError> {
    let route = scenario
        .castling_routes
        .iter()
        .find(|route| {
            route.player == king.owner
                && route.king_start == king.at
                && route.king_destination == destination
                && state.available_castling_routes.contains(&route.id)
        })
        .ok_or(TransitionError::InvalidCastlingRoute)?;
    let rook = state
        .pieces
        .values_mut()
        .find(|piece| {
            piece.owner == king.owner
                && piece.kind == PieceKind::Rook
                && piece.at == route.rook_start
                && !piece.has_moved
        })
        .ok_or(TransitionError::InvalidCastlingRoute)?;
    rook.at = route.rook_destination;
    rook.has_moved = true;
    Ok(())
}

fn clear_piece_references(state: &mut MatchState, captured: PieceId) {
    state.promotion_candidates.remove(&captured);
    for settlement in &mut state.settlements {
        if settlement.founder == Some(captured) {
            settlement.founder = None;
            settlement.cycle_interrupted = true;
        }
        if settlement.produced_pawn == Some(captured) {
            settlement.produced_pawn = None;
        }
        if settlement.transfer_candidate == Some(captured) {
            settlement.transfer_candidate = None;
        }
    }
}

fn remove_castling_right_for_rook(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    owner: Player,
    at: Coord,
) {
    state.available_castling_routes.retain(|route_id| {
        scenario
            .castling_routes
            .iter()
            .find(|route| route.id == *route_id)
            .is_none_or(|route| route.player != owner || route.rook_start != at)
    });
}

struct Board<'a> {
    scenario: &'a ScenarioDefinition,
    state: &'a MatchState,
    occupancy: BTreeMap<Coord, &'a Piece>,
}

enum GovernanceTrace {
    Clear(Vec<Coord>),
    Blocked {
        path: Vec<Coord>,
        blocker: GovernanceBlocker,
    },
}

impl<'a> Board<'a> {
    fn new(scenario: &'a ScenarioDefinition, state: &'a MatchState) -> Self {
        Self {
            scenario,
            state,
            occupancy: state
                .pieces
                .values()
                .map(|piece| (piece.at, piece))
                .collect(),
        }
    }

    fn pseudo_legal_moves(&self, piece: &Piece) -> Vec<LegalMove> {
        let mut moves = match piece.kind {
            PieceKind::Queen => self.slider_moves(piece, &QUEEN_DIRECTIONS),
            PieceKind::Rook => self.slider_moves(piece, &ROOK_DIRECTIONS),
            PieceKind::Bishop => self.slider_moves(piece, &BISHOP_DIRECTIONS),
            PieceKind::Knight => self.jump_moves(piece, &KNIGHT_OFFSETS),
            PieceKind::King => self.king_moves(piece),
            PieceKind::Pawn => self.pawn_moves(piece),
        };
        moves.sort_unstable();
        moves
    }

    fn attacks_from(&self, piece: &Piece) -> Vec<Coord> {
        match piece.kind {
            PieceKind::Queen => self.slider_attacks(piece, &QUEEN_DIRECTIONS),
            PieceKind::Rook => self.slider_attacks(piece, &ROOK_DIRECTIONS),
            PieceKind::Bishop => self.slider_attacks(piece, &BISHOP_DIRECTIONS),
            PieceKind::Knight => self.jump_attacks(piece, &KNIGHT_OFFSETS),
            PieceKind::King => KING_OFFSETS
                .into_iter()
                .filter_map(|offset| self.step(piece, offset))
                .collect(),
            PieceKind::Pawn => {
                let direction = self.scenario.rules.pawn_forward_y[&piece.owner];
                [(-1, direction), (1, direction)]
                    .into_iter()
                    .filter_map(|offset| self.step(piece, offset))
                    .collect()
            }
        }
    }

    fn attack_path_to(&self, piece: &Piece, target: Coord) -> Option<Vec<Coord>> {
        let slider_directions = match piece.kind {
            PieceKind::Queen => Some(QUEEN_DIRECTIONS.as_slice()),
            PieceKind::Rook => Some(ROOK_DIRECTIONS.as_slice()),
            PieceKind::Bishop => Some(BISHOP_DIRECTIONS.as_slice()),
            PieceKind::Knight | PieceKind::King | PieceKind::Pawn => None,
        };
        if let Some(directions) = slider_directions {
            for &direction in directions {
                let mut current = piece.at;
                let mut path = vec![piece.at];
                while let Some(next) = offset_coord(current, direction) {
                    if !next.is_within(self.scenario.board)
                        || self.terrain(next) == TileTerrain::Mountain
                        || !self.can_cross(piece, current, next)
                    {
                        break;
                    }
                    path.push(next);
                    if next == target {
                        return Some(path);
                    }
                    if self.occupancy.contains_key(&next)
                        || self.terrain(next) == TileTerrain::Forest
                    {
                        break;
                    }
                    current = next;
                }
            }
            return None;
        }

        let attacks = match piece.kind {
            PieceKind::Knight => self.jump_attacks(piece, &KNIGHT_OFFSETS),
            PieceKind::King => KING_OFFSETS
                .into_iter()
                .filter_map(|offset| self.step(piece, offset))
                .collect(),
            PieceKind::Pawn => {
                let direction = self.scenario.rules.pawn_forward_y[&piece.owner];
                [(-1, direction), (1, direction)]
                    .into_iter()
                    .filter_map(|offset| self.step(piece, offset))
                    .collect()
            }
            PieceKind::Queen | PieceKind::Rook | PieceKind::Bishop => unreachable!(),
        };
        attacks.contains(&target).then(|| vec![piece.at, target])
    }

    fn governance_trace(&self, piece: &Piece, target: Coord) -> Option<GovernanceTrace> {
        let direction = aligned_direction(piece, target)?;
        let mut current = piece.at;
        let mut path = vec![piece.at];
        loop {
            let next = offset_coord(current, direction)?;
            if !next.is_within(self.scenario.board) {
                return None;
            }
            if self.terrain(next) == TileTerrain::Mountain {
                path.push(next);
                return Some(GovernanceTrace::Blocked {
                    path,
                    blocker: GovernanceBlocker::Terrain {
                        at: next,
                        terrain: TileTerrain::Mountain,
                    },
                });
            }
            if let Some((edge, kind)) = self.first_blocking_edge(piece, current, next) {
                return Some(GovernanceTrace::Blocked {
                    path,
                    blocker: GovernanceBlocker::Edge { edge, kind },
                });
            }
            path.push(next);
            if next == target {
                return Some(GovernanceTrace::Clear(path));
            }
            if let Some(blocker) = self.occupancy.get(&next) {
                return Some(GovernanceTrace::Blocked {
                    path,
                    blocker: GovernanceBlocker::Piece {
                        piece: blocker.id,
                        at: next,
                    },
                });
            }
            if self.terrain(next) == TileTerrain::Forest {
                return Some(GovernanceTrace::Blocked {
                    path,
                    blocker: GovernanceBlocker::Terrain {
                        at: next,
                        terrain: TileTerrain::Forest,
                    },
                });
            }
            current = next;
        }
    }

    fn is_square_attacked(&self, at: Coord, by: Player) -> bool {
        self.state
            .pieces
            .values()
            .filter(|piece| piece.owner == by)
            .any(|piece| self.attacks_from(piece).contains(&at))
    }

    fn slider_moves(&self, piece: &Piece, directions: &[(i8, i8)]) -> Vec<LegalMove> {
        let mut moves = Vec::new();
        for &direction in directions {
            for target in self.ray(piece, direction) {
                if let Some(occupant) = self.occupancy.get(&target) {
                    if occupant.owner != piece.owner && occupant.kind != PieceKind::King {
                        moves.push(Self::move_to(
                            piece,
                            target,
                            MoveKind::Capture {
                                captured: occupant.id,
                            },
                        ));
                    }
                    break;
                }
                moves.push(Self::move_to(piece, target, MoveKind::Quiet));
                if self.terrain(target) == TileTerrain::Forest {
                    break;
                }
            }
        }
        moves
    }

    fn slider_attacks(&self, piece: &Piece, directions: &[(i8, i8)]) -> Vec<Coord> {
        let mut attacks = Vec::new();
        for &direction in directions {
            for target in self.ray(piece, direction) {
                attacks.push(target);
                if self.occupancy.contains_key(&target)
                    || self.terrain(target) == TileTerrain::Forest
                {
                    break;
                }
            }
        }
        attacks
    }

    fn ray(&self, piece: &Piece, direction: (i8, i8)) -> Vec<Coord> {
        let mut targets = Vec::new();
        let mut current = piece.at;
        while let Some(next) = offset_coord(current, direction) {
            if !next.is_within(self.scenario.board)
                || self.terrain(next) == TileTerrain::Mountain
                || !self.can_cross(piece, current, next)
            {
                break;
            }
            targets.push(next);
            if self.occupancy.contains_key(&next) || self.terrain(next) == TileTerrain::Forest {
                break;
            }
            current = next;
        }
        targets
    }

    fn jump_moves(&self, piece: &Piece, offsets: &[(i8, i8)]) -> Vec<LegalMove> {
        offsets
            .iter()
            .filter_map(|&offset| self.jump_destination(piece.at, offset))
            .filter_map(|to| match self.occupancy.get(&to) {
                None => Some(Self::move_to(piece, to, MoveKind::Quiet)),
                Some(target) if target.owner != piece.owner && target.kind != PieceKind::King => {
                    Some(Self::move_to(
                        piece,
                        to,
                        MoveKind::Capture {
                            captured: target.id,
                        },
                    ))
                }
                Some(_) => None,
            })
            .collect()
    }

    fn jump_attacks(&self, piece: &Piece, offsets: &[(i8, i8)]) -> Vec<Coord> {
        offsets
            .iter()
            .filter_map(|&offset| self.jump_destination(piece.at, offset))
            .collect()
    }

    fn king_moves(&self, piece: &Piece) -> Vec<LegalMove> {
        let mut moves = KING_OFFSETS
            .iter()
            .filter_map(|&offset| self.step(piece, offset))
            .filter_map(|to| match self.occupancy.get(&to) {
                None => Some(Self::move_to(piece, to, MoveKind::Quiet)),
                Some(target) if target.owner != piece.owner && target.kind != PieceKind::King => {
                    Some(Self::move_to(
                        piece,
                        to,
                        MoveKind::Capture {
                            captured: target.id,
                        },
                    ))
                }
                Some(_) => None,
            })
            .collect::<Vec<_>>();
        moves.extend(self.castling_moves(piece));
        moves
    }

    fn castling_moves(&self, king: &Piece) -> Vec<LegalMove> {
        if king.has_moved || self.is_square_attacked(king.at, king.owner.opponent()) {
            return Vec::new();
        }
        self.scenario
            .castling_routes
            .iter()
            .filter(|route| {
                route.player == king.owner
                    && route.king_start == king.at
                    && self.state.available_castling_routes.contains(&route.id)
            })
            .filter(|route| {
                self.occupancy.get(&route.rook_start).is_some_and(|rook| {
                    rook.owner == king.owner
                        && rook.kind == PieceKind::Rook
                        && !rook.has_moved
                        && self.castling_king_route_clear(king, route)
                        && self.castling_rook_route_clear(rook, route)
                })
            })
            .filter(|route| {
                route
                    .king_path
                    .iter()
                    .chain([&route.king_destination])
                    .all(|at| !self.is_square_attacked(*at, king.owner.opponent()))
            })
            .map(|route| Self::move_to(king, route.king_destination, MoveKind::Castle))
            .collect()
    }

    fn castling_king_route_clear(
        &self,
        king: &Piece,
        route: &crate::scenario::CastlingRoute,
    ) -> bool {
        let mut previous = king.at;
        for &at in &route.king_path {
            if self.terrain(at) == TileTerrain::Mountain
                || !self.can_cross(king, previous, at)
                || (at != route.rook_start && self.occupancy.contains_key(&at))
            {
                return false;
            }
            previous = at;
        }
        true
    }

    fn castling_rook_route_clear(
        &self,
        rook: &Piece,
        route: &crate::scenario::CastlingRoute,
    ) -> bool {
        let direction = match (
            route.rook_start.x.cmp(&route.rook_destination.x),
            route.rook_start.y.cmp(&route.rook_destination.y),
        ) {
            (std::cmp::Ordering::Less, std::cmp::Ordering::Equal) => (1, 0),
            (std::cmp::Ordering::Greater, std::cmp::Ordering::Equal) => (-1, 0),
            (std::cmp::Ordering::Equal, std::cmp::Ordering::Less) => (0, 1),
            (std::cmp::Ordering::Equal, std::cmp::Ordering::Greater) => (0, -1),
            _ => return false,
        };
        let mut current = route.rook_start;
        loop {
            let Some(next) = offset_coord(current, direction) else {
                return false;
            };
            if self.terrain(next) == TileTerrain::Mountain
                || !self.can_cross(rook, current, next)
                || (next != route.king_start && self.occupancy.contains_key(&next))
            {
                return false;
            }
            if next == route.rook_destination {
                return true;
            }
            if self.terrain(next) == TileTerrain::Forest {
                return false;
            }
            current = next;
        }
    }

    fn pawn_moves(&self, pawn: &Piece) -> Vec<LegalMove> {
        let direction = self.scenario.rules.pawn_forward_y[&pawn.owner];
        let mut moves = Vec::new();
        if let Some(one) = self.step(pawn, (0, direction))
            && !self.occupancy.contains_key(&one)
        {
            moves.push(Self::move_to(pawn, one, MoveKind::Quiet));
            if self.scenario.rules.allow_pawn_double_step
                && !pawn.has_moved
                && matches!(pawn.origin, crate::state::PieceOrigin::Deployed)
                && let Some(two) = offset_coord(pawn.at, (0, direction.saturating_mul(2)))
                && two.is_within(self.scenario.board)
                && self.terrain(two) != TileTerrain::Mountain
                && !self.occupancy.contains_key(&two)
                && self.can_cross(pawn, one, two)
            {
                moves.push(Self::move_to(pawn, two, MoveKind::PawnDoubleStep));
            }
        }
        for dx in [-1, 1] {
            if let Some(to) = self.step(pawn, (dx, direction)) {
                if let Some(target) = self.occupancy.get(&to) {
                    if target.owner != pawn.owner && target.kind != PieceKind::King {
                        moves.push(Self::move_to(
                            pawn,
                            to,
                            MoveKind::Capture {
                                captured: target.id,
                            },
                        ));
                    }
                } else if self.scenario.rules.allow_en_passant
                    && let Some(en_passant) = self.state.en_passant
                    && en_passant.expires_for == pawn.owner
                    && en_passant.capture_destination == to
                    && self
                        .state
                        .pieces
                        .get(&en_passant.pawn)
                        .is_some_and(|target| {
                            target.owner != pawn.owner && target.kind == PieceKind::Pawn
                        })
                {
                    moves.push(Self::move_to(
                        pawn,
                        to,
                        MoveKind::EnPassant {
                            captured: en_passant.pawn,
                        },
                    ));
                }
            }
        }
        moves
    }

    fn move_to(piece: &Piece, to: Coord, kind: MoveKind) -> LegalMove {
        LegalMove {
            piece: piece.id,
            from: piece.at,
            to,
            kind,
        }
    }

    fn jump_destination(&self, from: Coord, offset: (i8, i8)) -> Option<Coord> {
        let to = offset_coord(from, offset)?;
        (to.is_within(self.scenario.board) && self.terrain(to) != TileTerrain::Mountain)
            .then_some(to)
    }

    fn step(&self, piece: &Piece, offset: (i8, i8)) -> Option<Coord> {
        let to = offset_coord(piece.at, offset)?;
        if !to.is_within(self.scenario.board) || self.terrain(to) == TileTerrain::Mountain {
            return None;
        }
        self.can_cross(piece, piece.at, to).then_some(to)
    }

    fn terrain(&self, at: Coord) -> TileTerrain {
        self.scenario
            .terrain
            .get(&at)
            .copied()
            .unwrap_or(TileTerrain::Open)
    }

    fn can_cross(&self, piece: &Piece, from: Coord, to: Coord) -> bool {
        self.first_blocking_edge(piece, from, to).is_none()
    }

    fn first_blocking_edge(
        &self,
        piece: &Piece,
        from: Coord,
        to: Coord,
    ) -> Option<(Edge, EdgeKind)> {
        let dx = from.x.abs_diff(to.x);
        let dy = from.y.abs_diff(to.y);
        if dx <= 1 && dy <= 1 && dx + dy > 0 {
            if dx == 1 && dy == 1 {
                let horizontal = Coord::new(to.x, from.y);
                let vertical = Coord::new(from.x, to.y);
                return [
                    Edge::new(from, horizontal),
                    Edge::new(from, vertical),
                    Edge::new(horizontal, to),
                    Edge::new(vertical, to),
                ]
                .into_iter()
                .find_map(|edge| self.blocking_edge(piece, edge));
            }
            return self.blocking_edge(piece, Edge::new(from, to));
        }
        None
    }

    fn blocking_edge(&self, piece: &Piece, edge: Edge) -> Option<(Edge, EdgeKind)> {
        match self.scenario.edges.get(&edge) {
            None | Some(EdgeKind::Bridge | EdgeKind::Ford | EdgeKind::Gate) => None,
            Some(EdgeKind::River) => Some((edge, EdgeKind::River)),
            Some(EdgeKind::Wall) => {
                let projected = piece.kind == PieceKind::Rook
                    && self.scenario.fortifications.iter().any(|fortification| {
                        fortification.owner == piece.owner
                            && fortification.tower == piece.at
                            && fortification.projected_wall == edge
                    });
                (!projected).then_some((edge, EdgeKind::Wall))
            }
        }
    }
}

fn aligned_direction(piece: &Piece, target: Coord) -> Option<(i8, i8)> {
    let dx = i32::from(target.x) - i32::from(piece.at.x);
    let dy = i32::from(target.y) - i32::from(piece.at.y);
    let direction = (dx.signum() as i8, dy.signum() as i8);
    match piece.kind {
        PieceKind::King if dx.abs() <= 1 && dy.abs() <= 1 && (dx != 0 || dy != 0) => {
            Some(direction)
        }
        PieceKind::Queen if dx == 0 || dy == 0 || dx.abs() == dy.abs() => Some(direction),
        PieceKind::Rook if (dx == 0) ^ (dy == 0) => Some(direction),
        PieceKind::Bishop if dx.abs() == dy.abs() && dx != 0 => Some(direction),
        PieceKind::King
        | PieceKind::Queen
        | PieceKind::Rook
        | PieceKind::Bishop
        | PieceKind::Knight
        | PieceKind::Pawn => None,
    }
}

fn offset_coord(coord: Coord, offset: (i8, i8)) -> Option<Coord> {
    Some(Coord::new(
        offset_axis(coord.x, offset.0)?,
        offset_axis(coord.y, offset.1)?,
    ))
}

fn offset_axis(value: u16, offset: i8) -> Option<u16> {
    if offset.is_negative() {
        value.checked_sub(u16::from(offset.unsigned_abs()))
    } else {
        value.checked_add(u16::from(offset.unsigned_abs()))
    }
}

const ROOK_DIRECTIONS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRECTIONS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const QUEEN_DIRECTIONS: [(i8, i8); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];
const KING_OFFSETS: [(i8, i8); 8] = QUEEN_DIRECTIONS;
const KNIGHT_OFFSETS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (-1, 2),
    (-2, 1),
    (1, -2),
    (2, -1),
    (-1, -2),
    (-2, -1),
];

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::{
        scenario::{
            BoardSize, CastlingRoute, Deployment, Edge, EdgeKind, Fortification, KeepDefinition,
            PromotionSite, SCENARIO_SCHEMA_VERSION, ScenarioMetadata, ScenarioRules,
            SettlementSite, TileTerrain,
        },
        state::PieceOrigin,
    };

    use super::*;

    fn deployment(player: Player, kind: PieceKind, x: u16, y: u16) -> Deployment {
        Deployment {
            player,
            kind,
            at: Coord::new(x, y),
        }
    }

    fn scenario_with(mut deployments: Vec<Deployment>) -> ScenarioDefinition {
        if !deployments
            .iter()
            .any(|piece| piece.player == Player::South && piece.kind == PieceKind::King)
        {
            deployments.push(deployment(Player::South, PieceKind::King, 4, 7));
        }
        if !deployments
            .iter()
            .any(|piece| piece.player == Player::North && piece.kind == PieceKind::King)
        {
            deployments.push(deployment(Player::North, PieceKind::King, 4, 0));
        }
        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "rules-test".to_owned(),
            metadata: ScenarioMetadata {
                name: "Rules test".to_owned(),
                description: String::new(),
                expected_minutes: (1, 2),
            },
            board: BoardSize {
                width: 8,
                height: 8,
            },
            terrain: BTreeMap::new(),
            edges: BTreeMap::new(),
            deployments,
            settlements: Vec::new(),
            promotion_sites: Vec::new(),
            keeps: Vec::new(),
            fortifications: Vec::new(),
            castling_routes: Vec::new(),
            rules: ScenarioRules {
                army_setup: crate::scenario::ArmySetup::Custom,
                ..ScenarioRules::default()
            },
        }
    }

    fn piece_id_at(state: &MatchState, at: Coord) -> PieceId {
        state
            .pieces
            .values()
            .find(|piece| piece.at == at)
            .map(|piece| piece.id)
            .expect("fixture piece exists")
    }

    fn assert_piece_has_no_move_to(
        scenario: &ScenarioDefinition,
        state: &MatchState,
        piece: PieceId,
        target: Coord,
    ) {
        assert!(
            !legal_moves(scenario, state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == piece && mv.to == target)
        );
    }

    fn add_south_fortification(scenario: &mut ScenarioDefinition, tower: Coord, outside: Coord) {
        let wall = Edge::new(tower, outside);
        scenario.edges.insert(wall, EdgeKind::Wall);
        scenario.fortifications.push(Fortification {
            id: "south-tower".to_owned(),
            owner: Player::South,
            tower,
            projected_wall: wall,
        });
        scenario.keeps.push(KeepDefinition {
            id: "south-keep".to_owned(),
            owner: Player::South,
            tiles: BTreeSet::from([tower]),
            gates: BTreeSet::new(),
            fortification_ids: BTreeSet::from(["south-tower".to_owned()]),
        });
    }

    fn add_settlement(scenario: &mut ScenarioDefinition, at: Coord) {
        scenario.settlements.push(SettlementSite {
            id: "governance-test".to_owned(),
            at,
        });
    }

    #[test]
    fn attack_lines_include_friendly_blocker_but_stop_behind_it() {
        let scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Rook, 0, 7),
            deployment(Player::South, PieceKind::Pawn, 0, 5),
        ]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, Coord::new(0, 7));

        assert_eq!(
            attack_lines_on(&scenario, &state, Coord::new(0, 5), Player::South).unwrap(),
            vec![AttackLine {
                attacker: rook,
                path: vec![Coord::new(0, 7), Coord::new(0, 6), Coord::new(0, 5)],
            }]
        );
        assert!(
            attack_lines_on(&scenario, &state, Coord::new(0, 4), Player::South)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn attack_queries_cover_corner_jumps_edges_and_both_pawn_orientations() {
        let scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Knight, 0, 0),
            deployment(Player::South, PieceKind::Pawn, 2, 6),
            deployment(Player::North, PieceKind::Pawn, 5, 1),
        ]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let knight = piece_id_at(&state, Coord::new(0, 0));
        let south_pawn = piece_id_at(&state, Coord::new(2, 6));
        let north_pawn = piece_id_at(&state, Coord::new(5, 1));

        for target in [Coord::new(1, 2), Coord::new(2, 1)] {
            assert!(
                attack_lines_on(&scenario, &state, target, Player::South)
                    .unwrap()
                    .iter()
                    .any(|attack| attack.attacker == knight)
            );
        }
        assert!(
            attack_lines_on(&scenario, &state, Coord::new(1, 5), Player::South)
                .unwrap()
                .iter()
                .any(|attack| attack.attacker == south_pawn)
        );
        assert!(
            attack_lines_on(&scenario, &state, Coord::new(4, 2), Player::North)
                .unwrap()
                .iter()
                .any(|attack| attack.attacker == north_pawn)
        );
        assert!(matches!(
            attack_lines_on(&scenario, &state, Coord::new(8, 0), Player::South),
            Err(TransitionError::CoordinateOutOfBounds(Coord { x: 8, y: 0 }))
        ));
    }

    #[test]
    fn governance_includes_founder_endpoint_pins_and_stable_multiple_governors() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Bishop, 0, 0),
            deployment(Player::South, PieceKind::Rook, 3, 7),
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Knight, 2, 5),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let founder = piece_id_at(&state, target);
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(founder);

        let report = governance_report(&scenario, &state, 0).unwrap();
        assert_eq!(report.governors.len(), 2);
        assert!(report.governors.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            report
                .governors
                .iter()
                .all(|line| line.path.last() == Some(&target))
        );
        assert!(report.governors.iter().all(|line| line.attacker != founder));

        let pinned_target = Coord::new(4, 4);
        let mut pinned_scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::King, 7, 6),
            deployment(Player::South, PieceKind::Rook, 4, 6),
            deployment(Player::North, PieceKind::Rook, 0, 6),
        ]);
        add_settlement(&mut pinned_scenario, pinned_target);
        let mut pinned_state = MatchState::from_scenario(&pinned_scenario).unwrap();
        pinned_state.settlements[0].owner = Some(Player::South);
        let pinned_rook = piece_id_at(&pinned_state, Coord::new(4, 6));
        assert!(
            governance_report(&pinned_scenario, &pinned_state, 0)
                .unwrap()
                .governors
                .iter()
                .any(|line| line.attacker == pinned_rook)
        );
    }

    #[test]
    fn governance_reports_piece_terrain_and_edge_blockers() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Bishop, 0, 0),
            deployment(Player::South, PieceKind::Rook, 3, 7),
            deployment(Player::South, PieceKind::Pawn, 3, 5),
            deployment(Player::South, PieceKind::Queen, 7, 3),
            deployment(Player::South, PieceKind::Bishop, 6, 0),
        ]);
        add_settlement(&mut scenario, target);
        scenario
            .terrain
            .insert(Coord::new(1, 1), TileTerrain::Forest);
        scenario
            .terrain
            .insert(Coord::new(5, 3), TileTerrain::Mountain);
        scenario.edges.insert(
            Edge::new(Coord::new(6, 0), Coord::new(5, 0)),
            EdgeKind::River,
        );
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.settlements[0].owner = Some(Player::South);

        let report = governance_report(&scenario, &state, 0).unwrap();
        assert!(report.governors.is_empty());
        assert!(report.blocked.iter().any(|line| matches!(
            line.blocker,
            GovernanceBlocker::Piece { at, .. } if at == Coord::new(3, 5)
        )));
        assert!(report.blocked.iter().any(|line| matches!(
            line.blocker,
            GovernanceBlocker::Terrain {
                at,
                terrain: TileTerrain::Forest
            } if at == Coord::new(1, 1)
        )));
        assert!(report.blocked.iter().any(|line| matches!(
            line.blocker,
            GovernanceBlocker::Terrain {
                at,
                terrain: TileTerrain::Mountain
            } if at == Coord::new(5, 3)
        )));
        assert!(report.blocked.iter().any(|line| matches!(
            line.blocker,
            GovernanceBlocker::Edge {
                kind: EdgeKind::River,
                ..
            }
        )));
    }

    #[test]
    fn accepted_actions_latch_broken_settlement_continuity() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));
        let rook = piece_id_at(&state, Coord::new(3, 7));

        let transition = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: rook,
                to: Coord::new(2, 7),
            },
        )
        .unwrap();

        assert!(transition.state.settlements[0].cycle_interrupted);
        assert!(
            transition
                .events
                .contains(&TransitionEvent::SettlementContinuityInterrupted {
                    settlement_index: 0,
                })
        );
    }

    #[test]
    fn owner_turn_boundary_snapshots_and_resets_continuity() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));
        state.active_player = Player::North;

        let continuous = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert!(continuous.state.settlements[0].completed_cycle_continuous);
        assert!(!continuous.state.settlements[0].cycle_interrupted);
        assert!(
            continuous
                .events
                .contains(&TransitionEvent::SettlementCycleStarted {
                    settlement_index: 0,
                    player: Player::South,
                    previous_continuous: true,
                })
        );

        state.settlements[0].cycle_interrupted = true;
        let interrupted = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert!(!interrupted.state.settlements[0].completed_cycle_continuous);
        assert!(!interrupted.state.settlements[0].cycle_interrupted);

        let save =
            crate::persistence::SaveEnvelope::new("continuity-test", interrupted.state.clone())
                .unwrap();
        let loaded = crate::persistence::SaveReader::new()
            .read(&save.to_json().unwrap())
            .unwrap();
        assert_eq!(loaded.state, interrupted.state);
    }

    #[test]
    fn continuous_owner_cycle_advances_to_configured_establishment_threshold() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        scenario.rules.establishment_cycles = 2;
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.active_player = Player::North;
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));
        state.settlements[0].establishment_progress = 1;

        let transition = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();

        assert_eq!(transition.state.settlements[0].establishment_progress, 2);
        assert!(transition.state.settlements[0].established);
        assert!(
            transition
                .events
                .contains(&TransitionEvent::SettlementDevelopmentAdvanced {
                    settlement_index: 0,
                    progress: 2,
                })
        );
        assert!(
            transition
                .events
                .contains(&TransitionEvent::SettlementEstablished {
                    settlement_index: 0,
                })
        );
    }

    #[test]
    fn repetition_records_final_post_realm_state() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.active_player = Player::North;
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));

        let transition = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(transition.state.settlements[0].establishment_progress, 1);
        let final_key = transition.state.repetition_key().unwrap();
        assert_eq!(transition.state.repetition_counts[&final_key], 1);
        let mut transient = transition.state.clone();
        transient.settlements[0].establishment_progress = 0;
        transient.settlements[0].completed_cycle_continuous = false;
        let transient_key = transient.repetition_key().unwrap();
        assert!(
            !transition
                .state
                .repetition_counts
                .contains_key(&transient_key)
        );
    }

    #[test]
    fn interrupted_development_pauses_or_resets_by_scenario_policy() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.active_player = Player::North;
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));
        state.settlements[0].establishment_progress = 1;
        state.settlements[0].cycle_interrupted = true;

        let paused = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(paused.state.settlements[0].establishment_progress, 1);
        assert!(!paused.state.settlements[0].established);

        scenario.rules.development_resets_when_interrupted = true;
        let reset = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(reset.state.settlements[0].establishment_progress, 0);
        assert!(
            reset
                .events
                .contains(&TransitionEvent::SettlementDevelopmentReset {
                    settlement_index: 0,
                })
        );
    }

    #[test]
    fn establishment_persists_after_founder_moves_away() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let founder = piece_id_at(&state, target);
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(founder);
        state.settlements[0].establishment_progress = scenario.rules.establishment_cycles;
        state.settlements[0].established = true;

        let transition = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: founder,
                to: Coord::new(3, 2),
            },
        )
        .unwrap();

        assert!(transition.state.settlements[0].established);
        assert_eq!(
            transition.state.settlements[0].establishment_progress,
            scenario.rules.establishment_cycles
        );
    }

    #[test]
    fn established_settlement_queues_and_places_a_produced_pawn() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.active_player = Player::North;
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));
        state.settlements[0].established = true;
        state.settlements[0].establishment_progress = scenario.rules.establishment_cycles;
        state.settlements[0].production_progress = scenario.rules.production_cycles - 1;

        let ready = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(
            ready.state.settlements[0].production_progress,
            scenario.rules.production_cycles
        );
        let legal_squares = pawn_placement_squares(&scenario, &ready.state, 0).unwrap();
        assert!(legal_squares.contains(&Coord::new(2, 2)));
        assert_eq!(
            ready.state.phase,
            TurnPhase::ResolvingChoices {
                queue: vec![MandatoryChoice::PlacePawn {
                    settlement_index: 0,
                    legal_squares: legal_squares.clone(),
                }]
            }
        );
        assert!(
            ready
                .events
                .contains(&TransitionEvent::SettlementProductionAdvanced {
                    settlement_index: 0,
                    progress: scenario.rules.production_cycles,
                })
        );
        assert!(ready.events.contains(&TransitionEvent::PawnPlacementReady {
            settlement_index: 0,
            legal_squares: legal_squares.clone(),
        }));

        let produced = PieceId(ready.state.next_piece_id);
        let placed = apply_action(
            &scenario,
            &ready.state,
            &Action::PlacePawn {
                player: Player::South,
                settlement_index: 0,
                at: Coord::new(2, 2),
            },
        )
        .unwrap();
        assert_eq!(
            placed.state.pieces[&produced].origin,
            PieceOrigin::Settlement {
                settlement_index: 0
            }
        );
        assert_eq!(placed.state.settlements[0].produced_pawn, Some(produced));
        assert_eq!(placed.state.settlements[0].production_progress, 0);
        assert_eq!(placed.state.phase, TurnPhase::Command);
        assert_piece_has_no_move_to(&scenario, &placed.state, produced, Coord::new(2, 0));

        let mut capacity = placed.state.clone();
        capacity.active_player = Player::North;
        let next_cycle = apply_action(
            &scenario,
            &capacity,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(next_cycle.state.settlements[0].production_progress, 0);
        assert!(matches!(next_cycle.state.phase, TurnPhase::Command));

        let mut promoting = placed.state;
        promoting.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn: produced,
                site_index: 0,
            }],
        };
        let promoted = apply_action(
            &scenario,
            &promoting,
            &Action::ChoosePromotion {
                player: Player::South,
                pawn: produced,
                promote_to: PromotionKind::Queen,
            },
        )
        .unwrap();
        assert_eq!(promoted.state.settlements[0].produced_pawn, None);
    }

    #[test]
    fn production_readiness_persists_until_a_legal_square_opens() {
        let target = Coord::new(3, 3);
        let mut deployments = vec![
            deployment(Player::South, PieceKind::King, 4, 4),
            deployment(Player::South, PieceKind::Pawn, 3, 3),
        ];
        for at in [
            Coord::new(2, 2),
            Coord::new(3, 2),
            Coord::new(4, 2),
            Coord::new(2, 3),
            Coord::new(4, 3),
            Coord::new(2, 4),
            Coord::new(3, 4),
        ] {
            deployments.push(deployment(Player::South, PieceKind::Pawn, at.x, at.y));
        }
        let mut scenario = scenario_with(deployments);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.active_player = Player::North;
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));
        state.settlements[0].established = true;
        state.settlements[0].establishment_progress = scenario.rules.establishment_cycles;
        state.settlements[0].production_progress = scenario.rules.production_cycles - 1;

        let blocked = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(
            blocked.state.settlements[0].production_progress,
            scenario.rules.production_cycles
        );
        assert!(matches!(blocked.state.phase, TurnPhase::Command));

        let mut opened = blocked.state;
        let occupant = piece_id_at(&opened, Coord::new(2, 3));
        opened.pieces.remove(&occupant);
        opened.active_player = Player::North;
        let ready = apply_action(
            &scenario,
            &opened,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(
            ready.state.settlements[0].production_progress,
            scenario.rules.production_cycles
        );
        assert!(matches!(
            ready.state.phase,
            TurnPhase::ResolvingChoices { .. }
        ));
    }

    #[test]
    fn pawn_placement_respects_mountains_and_edge_barriers() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        scenario
            .terrain
            .insert(Coord::new(2, 2), TileTerrain::Mountain);
        scenario.edges.insert(
            Edge::new(Coord::new(3, 3), Coord::new(3, 2)),
            EdgeKind::Wall,
        );
        scenario.edges.insert(
            Edge::new(Coord::new(3, 3), Coord::new(4, 3)),
            EdgeKind::River,
        );
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(piece_id_at(&state, target));
        state.settlements[0].established = true;
        state.settlements[0].establishment_progress = scenario.rules.establishment_cycles;
        state.settlements[0].production_progress = scenario.rules.production_cycles;

        let squares = pawn_placement_squares(&scenario, &state, 0).unwrap();
        assert!(!squares.contains(&Coord::new(2, 2)));
        assert!(!squares.contains(&Coord::new(3, 2)));
        assert!(!squares.contains(&Coord::new(4, 3)));
        assert!(squares.contains(&Coord::new(2, 3)));
    }

    #[test]
    fn surviving_promotion_candidate_queues_and_replaces_stable_identity() {
        let site = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Pawn, 3, 4)]);
        scenario.promotion_sites.push(PromotionSite {
            id: "court".to_owned(),
            at: site,
        });
        scenario.settlements.push(SettlementSite {
            id: "capacity-source".to_owned(),
            at: Coord::new(6, 6),
        });
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = piece_id_at(&state, Coord::new(3, 4));
        state.pieces.get_mut(&pawn).unwrap().origin = PieceOrigin::Settlement {
            settlement_index: 0,
        };
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].established = true;
        state.settlements[0].produced_pawn = Some(pawn);

        let candidate = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: pawn,
                to: site,
            },
        )
        .unwrap();
        assert_eq!(candidate.state.promotion_candidates[&pawn], 0);
        assert!(
            candidate
                .events
                .contains(&TransitionEvent::PromotionCandidateStarted { pawn })
        );
        assert!(matches!(candidate.state.phase, TurnPhase::Command));

        let ready = apply_action(
            &scenario,
            &candidate.state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(ready.state.promotion_candidates[&pawn], 1);
        assert_eq!(
            ready.state.phase,
            TurnPhase::ResolvingChoices {
                queue: vec![MandatoryChoice::Promote {
                    pawn,
                    site_index: 0,
                }]
            }
        );
        assert!(ready.events.contains(&TransitionEvent::PromotionReady {
            pawn,
            site_index: 0,
        }));

        let promoted_id = PieceId(ready.state.next_piece_id);
        let promoted = apply_action(
            &scenario,
            &ready.state,
            &Action::ChoosePromotion {
                player: Player::South,
                pawn,
                promote_to: PromotionKind::Bishop,
            },
        )
        .unwrap();
        assert!(!promoted.state.pieces.contains_key(&pawn));
        assert_eq!(promoted.state.pieces[&promoted_id].owner, Player::South);
        assert_eq!(promoted.state.pieces[&promoted_id].at, site);
        assert_eq!(promoted.state.pieces[&promoted_id].kind, PieceKind::Bishop);
        assert_eq!(promoted.state.settlements[0].produced_pawn, None);
    }

    #[test]
    fn promotion_candidacy_cancels_on_departure_or_capture() {
        let site = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 4),
            deployment(Player::North, PieceKind::Rook, 3, 0),
        ]);
        scenario.promotion_sites.push(PromotionSite {
            id: "court".to_owned(),
            at: site,
        });
        let state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = piece_id_at(&state, Coord::new(3, 4));
        let candidate = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: pawn,
                to: site,
            },
        )
        .unwrap()
        .state;

        let mut leaving = candidate.clone();
        leaving.active_player = Player::South;
        let departed = apply_action(
            &scenario,
            &leaving,
            &Action::Move {
                player: Player::South,
                piece: pawn,
                to: Coord::new(3, 2),
            },
        )
        .unwrap();
        assert!(!departed.state.promotion_candidates.contains_key(&pawn));
        assert!(
            departed
                .events
                .contains(&TransitionEvent::PromotionCandidateCancelled { pawn })
        );

        let rook = piece_id_at(&candidate, Coord::new(3, 0));
        let captured = apply_action(
            &scenario,
            &candidate,
            &Action::Move {
                player: Player::North,
                piece: rook,
                to: site,
            },
        )
        .unwrap();
        assert!(!captured.state.pieces.contains_key(&pawn));
        assert!(!captured.state.promotion_candidates.contains_key(&pawn));
        assert!(
            captured
                .events
                .contains(&TransitionEvent::PromotionCandidateCancelled { pawn })
        );
    }

    #[test]
    fn promotion_rejects_a_result_that_leaves_king_in_check() {
        let site = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 3),
            deployment(Player::North, PieceKind::Rook, 0, 7),
        ]);
        scenario.promotion_sites.push(PromotionSite {
            id: "court".to_owned(),
            at: site,
        });
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = piece_id_at(&state, site);
        state.promotion_candidates.insert(pawn, 1);
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn,
                site_index: 0,
            }],
        };
        let before = state.clone();

        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::ChoosePromotion {
                    player: Player::South,
                    pawn,
                    promote_to: PromotionKind::Queen,
                }
            ),
            Err(TransitionError::PromotionLeavesKingInCheck)
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn turn_start_queues_all_choices_in_stable_coordinate_order() {
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 1, 1),
            deployment(Player::South, PieceKind::Pawn, 2, 2),
            deployment(Player::South, PieceKind::Pawn, 5, 5),
            deployment(Player::South, PieceKind::Pawn, 6, 6),
        ]);
        scenario.settlements = vec![
            SettlementSite {
                id: "west-town".to_owned(),
                at: Coord::new(1, 1),
            },
            SettlementSite {
                id: "east-town".to_owned(),
                at: Coord::new(5, 5),
            },
        ];
        scenario.promotion_sites = vec![
            PromotionSite {
                id: "west-court".to_owned(),
                at: Coord::new(2, 2),
            },
            PromotionSite {
                id: "east-court".to_owned(),
                at: Coord::new(6, 6),
            },
        ];
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.active_player = Player::North;
        for (index, at) in [(0, Coord::new(1, 1)), (1, Coord::new(5, 5))] {
            state.settlements[index].owner = Some(Player::South);
            state.settlements[index].founder = Some(piece_id_at(&state, at));
            state.settlements[index].established = true;
            state.settlements[index].establishment_progress = scenario.rules.establishment_cycles;
            state.settlements[index].production_progress = scenario.rules.production_cycles;
        }
        let west_candidate = piece_id_at(&state, Coord::new(2, 2));
        let east_candidate = piece_id_at(&state, Coord::new(6, 6));
        state.promotion_candidates.insert(west_candidate, 0);
        state.promotion_candidates.insert(east_candidate, 0);

        let first = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        let second = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert_eq!(first, second);

        let TurnPhase::ResolvingChoices { queue } = &first.state.phase else {
            panic!("turn start must queue all ready choices");
        };
        assert_eq!(queue.len(), 4);
        assert!(matches!(
            queue[0],
            MandatoryChoice::PlacePawn {
                settlement_index: 0,
                ..
            }
        ));
        assert_eq!(
            queue[1],
            MandatoryChoice::Promote {
                pawn: west_candidate,
                site_index: 0,
            }
        );
        assert!(matches!(
            queue[2],
            MandatoryChoice::PlacePawn {
                settlement_index: 1,
                ..
            }
        ));
        assert_eq!(
            queue[3],
            MandatoryChoice::Promote {
                pawn: east_candidate,
                site_index: 1,
            }
        );
    }

    #[test]
    fn current_turn_choice_cannot_change_completed_cycle_eligibility() {
        let settlement_at = Coord::new(3, 3);
        let promotion_at = Coord::new(2, 2);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 2, 2),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, settlement_at);
        scenario.promotion_sites.push(PromotionSite {
            id: "court".to_owned(),
            at: promotion_at,
        });
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let founder = piece_id_at(&state, promotion_at);
        state.active_player = Player::North;
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(founder);
        state.promotion_candidates.insert(founder, 0);

        let ready = apply_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        assert!(ready.state.settlements[0].completed_cycle_continuous);
        assert_eq!(ready.state.settlements[0].establishment_progress, 1);

        let promoted = apply_action(
            &scenario,
            &ready.state,
            &Action::ChoosePromotion {
                player: Player::South,
                pawn: founder,
                promote_to: PromotionKind::Knight,
            },
        )
        .unwrap();
        assert!(promoted.state.settlements[0].completed_cycle_continuous);
        assert_eq!(promoted.state.settlements[0].establishment_progress, 1);
        assert!(promoted.state.settlements[0].cycle_interrupted);
    }

    #[test]
    fn promotion_turn_start_replays_without_clock_input() {
        use crate::journal::{ActionJournal, AppendOutcome, IdempotencyKey};

        let site = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Pawn, 3, 4)]);
        scenario.promotion_sites.push(PromotionSite {
            id: "court".to_owned(),
            at: site,
        });
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = piece_id_at(&state, Coord::new(3, 4));
        let mut journal = ActionJournal::new("turn-start-test", &scenario).unwrap();
        for (key, action) in [
            (
                IdempotencyKey([1; 16]),
                Action::Move {
                    player: Player::South,
                    piece: pawn,
                    to: site,
                },
            ),
            (
                IdempotencyKey([2; 16]),
                Action::Hold {
                    player: Player::North,
                },
            ),
            (
                IdempotencyKey([3; 16]),
                Action::ChoosePromotion {
                    player: Player::South,
                    pawn,
                    promote_to: PromotionKind::Rook,
                },
            ),
        ] {
            let AppendOutcome::Accepted(transition) =
                journal.append(&scenario, &state, key, &action).unwrap()
            else {
                panic!("unique replay action must be accepted");
            };
            state = transition.state;
        }

        assert_eq!(journal.replay(&scenario).unwrap(), state);
    }

    #[test]
    fn pawn_landing_claims_neutral_settlement_without_removal() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 4),
            deployment(Player::South, PieceKind::Rook, 3, 7),
        ]);
        add_settlement(&mut scenario, target);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = piece_id_at(&state, Coord::new(3, 4));

        let transition = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: pawn,
                to: target,
            },
        )
        .unwrap();

        assert_eq!(transition.state.settlements[0].owner, Some(Player::South));
        assert_eq!(transition.state.settlements[0].founder, Some(pawn));
        assert_eq!(transition.state.pieces[&pawn].at, target);
        assert!(
            transition
                .events
                .contains(&TransitionEvent::SettlementClaimed {
                    settlement_index: 0,
                    owner: Player::South,
                    founder: pawn,
                })
        );
    }

    #[test]
    fn hostile_pawn_contests_then_transfers_only_at_its_owner_boundary() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 4),
            deployment(Player::North, PieceKind::Pawn, 0, 1),
            deployment(Player::North, PieceKind::Rook, 3, 0),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let candidate = piece_id_at(&state, Coord::new(3, 4));
        let old_founder = piece_id_at(&state, Coord::new(0, 1));
        state.settlements[0].owner = Some(Player::North);
        state.settlements[0].founder = Some(old_founder);
        state.settlements[0].establishment_progress = 2;
        state.settlements[0].established = true;
        state.settlements[0].production_progress = 1;

        let contested = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: candidate,
                to: target,
            },
        )
        .unwrap();
        assert_eq!(contested.state.settlements[0].owner, Some(Player::North));
        assert_eq!(
            contested.state.settlements[0].transfer_candidate,
            Some(candidate)
        );
        assert!(contested.state.settlements[0].cycle_interrupted);
        assert!(
            contested
                .events
                .contains(&TransitionEvent::SettlementContested {
                    settlement_index: 0,
                    candidate,
                })
        );

        let transferred = apply_action(
            &scenario,
            &contested.state,
            &Action::Hold {
                player: Player::North,
            },
        )
        .unwrap();
        let settlement = &transferred.state.settlements[0];
        assert_eq!(settlement.owner, Some(Player::South));
        assert_eq!(settlement.founder, Some(candidate));
        assert_eq!(settlement.transfer_candidate, None);
        assert_eq!(settlement.establishment_progress, 2);
        assert!(settlement.established);
        assert_eq!(settlement.production_progress, 1);
        assert!(!settlement.completed_cycle_continuous);
        assert!(
            transferred
                .events
                .contains(&TransitionEvent::SettlementTransferred {
                    settlement_index: 0,
                    previous_owner: Player::North,
                    owner: Player::South,
                    founder: candidate,
                })
        );
    }

    #[test]
    fn capture_cancels_transfer_without_changing_owner() {
        let target = Coord::new(3, 3);
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 4),
            deployment(Player::North, PieceKind::Pawn, 0, 1),
            deployment(Player::North, PieceKind::Rook, 3, 0),
        ]);
        add_settlement(&mut scenario, target);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let candidate = piece_id_at(&state, Coord::new(3, 4));
        state.settlements[0].owner = Some(Player::North);
        state.settlements[0].founder = Some(piece_id_at(&state, Coord::new(0, 1)));
        let contested = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: candidate,
                to: target,
            },
        )
        .unwrap()
        .state;
        let rook = piece_id_at(&contested, Coord::new(3, 0));

        let mut leaving = contested.clone();
        leaving.active_player = Player::South;
        let departed = apply_action(
            &scenario,
            &leaving,
            &Action::Move {
                player: Player::South,
                piece: candidate,
                to: Coord::new(3, 2),
            },
        )
        .unwrap();
        assert_eq!(departed.state.settlements[0].owner, Some(Player::North));
        assert_eq!(departed.state.settlements[0].transfer_candidate, None);
        assert!(
            departed
                .events
                .contains(&TransitionEvent::SettlementTransferCancelled {
                    settlement_index: 0,
                    candidate,
                })
        );

        let defended = apply_action(
            &scenario,
            &contested,
            &Action::Move {
                player: Player::North,
                piece: rook,
                to: target,
            },
        )
        .unwrap();

        assert_eq!(defended.state.settlements[0].owner, Some(Player::North));
        assert_eq!(defended.state.settlements[0].transfer_candidate, None);
        assert!(!defended.state.pieces.contains_key(&candidate));
        assert!(
            defended
                .events
                .contains(&TransitionEvent::SettlementTransferCancelled {
                    settlement_index: 0,
                    candidate,
                })
        );
    }

    #[test]
    fn slider_stops_on_forest_and_piece() {
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Rook, 0, 7)]);
        scenario
            .terrain
            .insert(Coord::new(0, 5), TileTerrain::Forest);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, Coord::new(0, 7));
        let moves = legal_moves(&scenario, &state).unwrap();
        assert!(
            moves
                .iter()
                .any(|mv| mv.piece == rook && mv.to == Coord::new(0, 5))
        );
        assert!(
            !moves
                .iter()
                .any(|mv| mv.piece == rook && mv.to == Coord::new(0, 4))
        );
    }

    #[test]
    fn mountain_blocks_sliders_but_not_knight_jumps() {
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Rook, 0, 7),
            deployment(Player::South, PieceKind::Knight, 1, 7),
        ]);
        scenario
            .terrain
            .insert(Coord::new(0, 6), TileTerrain::Mountain);
        scenario
            .terrain
            .insert(Coord::new(1, 6), TileTerrain::Mountain);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, Coord::new(0, 7));
        let knight = piece_id_at(&state, Coord::new(1, 7));
        let moves = legal_moves(&scenario, &state).unwrap();
        assert!(
            !moves
                .iter()
                .any(|mv| mv.piece == rook && mv.to == Coord::new(0, 6))
        );
        assert!(
            moves
                .iter()
                .any(|mv| mv.piece == knight && mv.to == Coord::new(2, 5))
        );
    }

    #[test]
    fn river_blocks_and_bridge_reopens_a_ray() {
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Rook, 0, 7)]);
        let edge = Edge::new(Coord::new(0, 6), Coord::new(0, 5));
        scenario.edges.insert(edge, EdgeKind::River);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, Coord::new(0, 7));
        assert!(
            !legal_moves(&scenario, &state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == rook && mv.to == Coord::new(0, 5))
        );

        scenario.edges.insert(edge, EdgeKind::Bridge);
        assert!(
            legal_moves(&scenario, &state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == rook && mv.to == Coord::new(0, 5))
        );
    }

    #[test]
    fn orthogonal_edge_kinds_apply_closed_and_open_crossings() {
        let edge = Edge::new(Coord::new(0, 6), Coord::new(0, 5));
        for (kind, expected) in [
            (EdgeKind::River, false),
            (EdgeKind::Wall, false),
            (EdgeKind::Bridge, true),
            (EdgeKind::Ford, true),
            (EdgeKind::Gate, true),
        ] {
            let mut scenario =
                scenario_with(vec![deployment(Player::South, PieceKind::Rook, 0, 7)]);
            scenario.edges.insert(edge, kind);
            let state = MatchState::from_scenario(&scenario).unwrap();
            let rook = piece_id_at(&state, Coord::new(0, 7));
            assert_eq!(
                legal_moves(&scenario, &state)
                    .unwrap()
                    .iter()
                    .any(|mv| mv.piece == rook && mv.to == Coord::new(0, 5)),
                expected,
                "unexpected crossing behavior for {kind:?}"
            );
        }
    }

    #[test]
    fn diagonal_crossing_requires_each_component_edge_to_be_open() {
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Bishop, 1, 6)]);
        scenario.edges.insert(
            Edge::new(Coord::new(1, 6), Coord::new(2, 6)),
            EdgeKind::River,
        );
        let state = MatchState::from_scenario(&scenario).unwrap();
        let bishop = piece_id_at(&state, Coord::new(1, 6));
        assert!(
            !legal_moves(&scenario, &state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == bishop && mv.to == Coord::new(2, 5))
        );

        scenario.edges.insert(
            Edge::new(Coord::new(1, 6), Coord::new(2, 6)),
            EdgeKind::Ford,
        );
        assert!(
            legal_moves(&scenario, &state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == bishop && mv.to == Coord::new(2, 5))
        );
    }

    #[test]
    fn knight_jump_ignores_intervening_edge_barriers() {
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Knight, 1, 6)]);
        scenario.edges.insert(
            Edge::new(Coord::new(1, 6), Coord::new(2, 6)),
            EdgeKind::Wall,
        );
        scenario.edges.insert(
            Edge::new(Coord::new(2, 6), Coord::new(2, 5)),
            EdgeKind::River,
        );
        let state = MatchState::from_scenario(&scenario).unwrap();
        let knight = piece_id_at(&state, Coord::new(1, 6));
        assert!(
            legal_moves(&scenario, &state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == knight && mv.to == Coord::new(2, 4))
        );
    }

    #[test]
    fn friendly_rook_projects_through_only_its_linked_wall() {
        let tower = Coord::new(0, 7);
        let outside = Coord::new(0, 6);
        let target = Coord::new(0, 5);
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Rook, 0, 7)]);
        add_south_fortification(&mut scenario, tower, outside);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, tower);

        assert!(
            legal_moves(&scenario, &state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == rook && mv.to == target)
        );
        assert!(
            attack_lines_on(&scenario, &state, target, Player::South)
                .unwrap()
                .iter()
                .any(|attack| attack.attacker == rook)
        );

        let mut wrong_piece = state.clone();
        wrong_piece.pieces.get_mut(&rook).unwrap().kind = PieceKind::Queen;
        assert!(
            attack_lines_on(&scenario, &wrong_piece, target, Player::South)
                .unwrap()
                .is_empty()
        );

        let mut wrong_owner = state.clone();
        wrong_owner.pieces.get_mut(&rook).unwrap().owner = Player::North;
        assert!(
            attack_lines_on(&scenario, &wrong_owner, target, Player::North)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn projection_disappears_after_leaving_or_capture() {
        let tower = Coord::new(0, 7);
        let outside = Coord::new(0, 6);
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Rook, 0, 7)]);
        add_south_fortification(&mut scenario, tower, outside);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, tower);
        let moved = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: rook,
                to: outside,
            },
        )
        .unwrap()
        .state;
        assert!(
            attack_lines_on(&scenario, &moved, tower, Player::South)
                .unwrap()
                .is_empty()
        );

        let mut captured = state;
        captured.pieces.remove(&rook);
        assert!(
            attack_lines_on(&scenario, &captured, Coord::new(0, 5), Player::South)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn projection_respects_downstream_forest_blockers_and_check() {
        let tower = Coord::new(0, 7);
        let outside = Coord::new(0, 6);
        let mut forest_scenario =
            scenario_with(vec![deployment(Player::South, PieceKind::Rook, 0, 7)]);
        add_south_fortification(&mut forest_scenario, tower, outside);
        forest_scenario
            .terrain
            .insert(Coord::new(0, 5), TileTerrain::Forest);
        let forest_state = MatchState::from_scenario(&forest_scenario).unwrap();
        assert_eq!(
            attack_lines_on(
                &forest_scenario,
                &forest_state,
                Coord::new(0, 5),
                Player::South
            )
            .unwrap()
            .len(),
            1
        );
        assert!(
            attack_lines_on(
                &forest_scenario,
                &forest_state,
                Coord::new(0, 4),
                Player::South
            )
            .unwrap()
            .is_empty()
        );

        let mut blocker_scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Rook, 0, 7),
            deployment(Player::South, PieceKind::Pawn, 0, 5),
        ]);
        add_south_fortification(&mut blocker_scenario, tower, outside);
        let blocker_state = MatchState::from_scenario(&blocker_scenario).unwrap();
        assert_eq!(
            attack_lines_on(
                &blocker_scenario,
                &blocker_state,
                Coord::new(0, 5),
                Player::South
            )
            .unwrap()
            .len(),
            1
        );
        assert!(
            attack_lines_on(
                &blocker_scenario,
                &blocker_state,
                Coord::new(0, 4),
                Player::South
            )
            .unwrap()
            .is_empty()
        );

        let mut check_scenario = scenario_with(vec![
            deployment(Player::North, PieceKind::King, 0, 5),
            deployment(Player::South, PieceKind::Rook, 0, 7),
        ]);
        add_south_fortification(&mut check_scenario, tower, outside);
        let check_state = MatchState::from_scenario(&check_scenario).unwrap();
        assert!(is_in_check(&check_scenario, &check_state, Player::North).unwrap());
    }

    #[test]
    fn pinned_piece_cannot_expose_king() {
        let scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Rook, 4, 6),
            deployment(Player::North, PieceKind::Rook, 4, 1),
        ]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, Coord::new(4, 6));
        let moves = legal_moves(&scenario, &state).unwrap();
        assert!(
            !moves
                .iter()
                .any(|mv| mv.piece == rook && mv.to == Coord::new(5, 6))
        );
        assert!(
            moves
                .iter()
                .any(|mv| mv.piece == rook && mv.to == Coord::new(4, 5))
        );
    }

    #[test]
    fn designated_castling_moves_both_pieces() {
        let mut scenario = scenario_with(vec![deployment(Player::South, PieceKind::Rook, 7, 7)]);
        scenario.castling_routes.push(CastlingRoute {
            id: "south-east".to_owned(),
            player: Player::South,
            king_start: Coord::new(4, 7),
            rook_start: Coord::new(7, 7),
            king_path: vec![Coord::new(5, 7), Coord::new(6, 7)],
            king_destination: Coord::new(6, 7),
            rook_destination: Coord::new(5, 7),
        });
        let state = MatchState::from_scenario(&scenario).unwrap();
        let king = piece_id_at(&state, Coord::new(4, 7));
        let transition = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: king,
                to: Coord::new(6, 7),
            },
        )
        .unwrap();
        let state = transition.state;
        assert_eq!(transition.events.len(), 3);
        assert!(transition.events.contains(&TransitionEvent::PieceMoved {
            piece: king,
            from: Coord::new(4, 7),
            to: Coord::new(6, 7),
        }));
        assert!(transition.events.iter().any(|event| matches!(
            event,
            TransitionEvent::PieceMoved {
                from,
                to,
                ..
            } if *from == Coord::new(7, 7) && *to == Coord::new(5, 7)
        )));
        assert_eq!(state.pieces[&king].at, Coord::new(6, 7));
        assert!(
            state
                .pieces
                .values()
                .any(|piece| piece.kind == PieceKind::Rook && piece.at == Coord::new(5, 7))
        );
    }

    #[test]
    fn castling_checks_attacks_and_complete_rook_occupancy_route() {
        let mut attacked = scenario_with(vec![
            deployment(Player::South, PieceKind::Rook, 7, 7),
            deployment(Player::North, PieceKind::Rook, 5, 0),
        ]);
        attacked.castling_routes.push(CastlingRoute {
            id: "south-east".to_owned(),
            player: Player::South,
            king_start: Coord::new(4, 7),
            rook_start: Coord::new(7, 7),
            king_path: vec![Coord::new(5, 7), Coord::new(6, 7)],
            king_destination: Coord::new(6, 7),
            rook_destination: Coord::new(5, 7),
        });
        let attacked_state = MatchState::from_scenario(&attacked).unwrap();
        let king = piece_id_at(&attacked_state, Coord::new(4, 7));
        assert!(
            !legal_moves(&attacked, &attacked_state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == king && mv.kind == MoveKind::Castle)
        );

        let mut blocked = scenario_with(vec![
            deployment(Player::South, PieceKind::Rook, 7, 7),
            deployment(Player::South, PieceKind::Knight, 6, 7),
        ]);
        blocked.castling_routes.push(CastlingRoute {
            id: "south-rook-long".to_owned(),
            player: Player::South,
            king_start: Coord::new(4, 7),
            rook_start: Coord::new(7, 7),
            king_path: vec![Coord::new(3, 7)],
            king_destination: Coord::new(3, 7),
            rook_destination: Coord::new(4, 7),
        });
        let blocked_state = MatchState::from_scenario(&blocked).unwrap();
        let king = piece_id_at(&blocked_state, Coord::new(4, 7));
        assert!(
            !legal_moves(&blocked, &blocked_state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == king && mv.kind == MoveKind::Castle)
        );

        let mut projected = scenario_with(vec![deployment(Player::South, PieceKind::Rook, 7, 7)]);
        add_south_fortification(&mut projected, Coord::new(7, 7), Coord::new(6, 7));
        projected.castling_routes.push(CastlingRoute {
            id: "south-projected-rook".to_owned(),
            player: Player::South,
            king_start: Coord::new(4, 7),
            rook_start: Coord::new(7, 7),
            king_path: vec![Coord::new(3, 7)],
            king_destination: Coord::new(3, 7),
            rook_destination: Coord::new(4, 7),
        });
        let projected_state = MatchState::from_scenario(&projected).unwrap();
        let king = piece_id_at(&projected_state, Coord::new(4, 7));
        assert!(
            legal_moves(&projected, &projected_state)
                .unwrap()
                .iter()
                .any(|mv| mv.piece == king && mv.kind == MoveKind::Castle)
        );
    }

    #[test]
    fn moving_or_losing_castling_participant_removes_route_right() {
        let mut south = scenario_with(vec![deployment(Player::South, PieceKind::Rook, 7, 7)]);
        south.castling_routes.push(CastlingRoute {
            id: "south-east".to_owned(),
            player: Player::South,
            king_start: Coord::new(4, 7),
            rook_start: Coord::new(7, 7),
            king_path: vec![Coord::new(5, 7), Coord::new(6, 7)],
            king_destination: Coord::new(6, 7),
            rook_destination: Coord::new(5, 7),
        });
        let state = MatchState::from_scenario(&south).unwrap();
        let rook = piece_id_at(&state, Coord::new(7, 7));
        let moved_rook = apply_action(
            &south,
            &state,
            &Action::Move {
                player: Player::South,
                piece: rook,
                to: Coord::new(7, 6),
            },
        )
        .unwrap()
        .state;
        assert!(!moved_rook.available_castling_routes.contains("south-east"));

        let king = piece_id_at(&state, Coord::new(4, 7));
        let moved_king = apply_action(
            &south,
            &state,
            &Action::Move {
                player: Player::South,
                piece: king,
                to: Coord::new(4, 6),
            },
        )
        .unwrap()
        .state;
        assert!(!moved_king.available_castling_routes.contains("south-east"));

        let mut north = scenario_with(vec![
            deployment(Player::North, PieceKind::Rook, 7, 0),
            deployment(Player::South, PieceKind::Bishop, 6, 1),
        ]);
        north.castling_routes.push(CastlingRoute {
            id: "north-east".to_owned(),
            player: Player::North,
            king_start: Coord::new(4, 0),
            rook_start: Coord::new(7, 0),
            king_path: vec![Coord::new(5, 0), Coord::new(6, 0)],
            king_destination: Coord::new(6, 0),
            rook_destination: Coord::new(5, 0),
        });
        let state = MatchState::from_scenario(&north).unwrap();
        let bishop = piece_id_at(&state, Coord::new(6, 1));
        let captured_rook = apply_action(
            &north,
            &state,
            &Action::Move {
                player: Player::South,
                piece: bishop,
                to: Coord::new(7, 0),
            },
        )
        .unwrap()
        .state;
        assert!(
            !captured_rook
                .available_castling_routes
                .contains("north-east")
        );
    }

    #[test]
    fn double_step_enables_one_reply_en_passant() {
        let scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Pawn, 3, 6),
            deployment(Player::North, PieceKind::Pawn, 4, 4),
        ]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let south_pawn = piece_id_at(&state, Coord::new(3, 6));
        assert_eq!(state.pieces[&south_pawn].origin, PieceOrigin::Deployed);
        let state = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: south_pawn,
                to: Coord::new(3, 4),
            },
        )
        .unwrap()
        .state;
        let north_pawn = piece_id_at(&state, Coord::new(4, 4));
        let state = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::North,
                piece: north_pawn,
                to: Coord::new(3, 5),
            },
        )
        .unwrap()
        .state;
        assert_eq!(state.pieces[&north_pawn].at, Coord::new(3, 5));
        assert!(!state.pieces.contains_key(&south_pawn));
    }

    #[test]
    fn hold_is_rejected_in_check() {
        let scenario = scenario_with(vec![deployment(Player::North, PieceKind::Rook, 4, 1)]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        assert!(is_in_check(&scenario, &state, Player::South).unwrap());
        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::Hold {
                    player: Player::South
                }
            ),
            Err(TransitionError::CannotHoldInCheck)
        ));
    }

    #[test]
    fn hold_completes_stalemated_turn_without_changing_occupancy_and_is_journaled() {
        use crate::journal::{ActionJournal, AppendOutcome, IdempotencyKey};

        let scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::King, 0, 7),
            deployment(Player::North, PieceKind::King, 2, 6),
            deployment(Player::North, PieceKind::Queen, 1, 5),
        ]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        assert!(!is_in_check(&scenario, &state, Player::South).unwrap());
        assert!(legal_moves(&scenario, &state).unwrap().is_empty());
        let occupancy = state.pieces.clone();

        let action = Action::Hold {
            player: Player::South,
        };
        let mut journal = ActionJournal::new("command-phase-test", &scenario).unwrap();
        let AppendOutcome::Accepted(transition) = journal
            .append(&scenario, &state, IdempotencyKey([1; 16]), &action)
            .unwrap()
        else {
            panic!("Hold must be accepted exactly once");
        };

        assert_eq!(transition.state.pieces, occupancy);
        assert_eq!(transition.state.active_player, Player::North);
        assert_eq!(transition.state.revision, state.revision + 1);
        assert!(transition.events.contains(&TransitionEvent::TurnHeld {
            player: Player::South,
        }));
        assert_eq!(journal.records.len(), 1);
        assert_eq!(journal.records[0].action, action);
        assert_eq!(journal.replay(&scenario).unwrap(), transition.state);
    }

    #[test]
    fn commands_are_rejected_after_match_termination() {
        let scenario = scenario_with(Vec::new());
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let king = piece_id_at(&state, Coord::new(4, 7));
        state.outcome = Some(MatchOutcome {
            winner: Some(Player::North),
            reason: OutcomeReason::Resignation,
        });
        let before = state.clone();

        for action in [
            Action::Hold {
                player: Player::South,
            },
            Action::Move {
                player: Player::South,
                piece: king,
                to: Coord::new(4, 6),
            },
        ] {
            assert!(matches!(
                apply_action(&scenario, &state, &action),
                Err(TransitionError::MatchFinished)
            ));
        }
        assert_eq!(state, before);
    }

    #[test]
    fn capture_event_preserves_identity_and_cleans_realm_references() {
        let mut scenario = scenario_with(vec![
            deployment(Player::South, PieceKind::Rook, 0, 7),
            deployment(Player::North, PieceKind::Pawn, 0, 5),
        ]);
        scenario.settlements = vec![
            SettlementSite {
                id: "founding".to_owned(),
                at: Coord::new(2, 2),
            },
            SettlementSite {
                id: "producing".to_owned(),
                at: Coord::new(5, 5),
            },
        ];
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let rook = piece_id_at(&state, Coord::new(0, 7));
        let captured = piece_id_at(&state, Coord::new(0, 5));
        state.settlements[0].founder = Some(captured);
        state.settlements[1].produced_pawn = Some(captured);
        state.pieces.get_mut(&captured).unwrap().origin = PieceOrigin::Settlement {
            settlement_index: 1,
        };

        let transition = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: rook,
                to: Coord::new(0, 5),
            },
        )
        .unwrap();

        assert!(!transition.state.pieces.contains_key(&captured));
        assert_eq!(transition.state.settlements[0].founder, None);
        assert!(transition.state.settlements[0].cycle_interrupted);
        assert_eq!(transition.state.settlements[1].produced_pawn, None);
        assert!(transition.events.contains(&TransitionEvent::PieceCaptured {
            piece: captured,
            at: Coord::new(0, 5),
        }));
    }

    #[test]
    fn kings_cannot_be_captured_and_rejection_leaves_source_unchanged() {
        let scenario = scenario_with(vec![deployment(Player::South, PieceKind::Queen, 4, 1)]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let queen = piece_id_at(&state, Coord::new(4, 1));
        let before = state.clone();

        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::Move {
                    player: Player::South,
                    piece: queen,
                    to: Coord::new(4, 0),
                }
            ),
            Err(TransitionError::IllegalMove { .. })
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn an_attacked_inescapable_king_resolves_as_checkmate() {
        let scenario = scenario_with(vec![
            deployment(Player::North, PieceKind::King, 0, 0),
            deployment(Player::South, PieceKind::King, 2, 2),
            deployment(Player::South, PieceKind::Queen, 1, 2),
        ]);
        let state = MatchState::from_scenario(&scenario).unwrap();
        let queen = piece_id_at(&state, Coord::new(1, 2));

        let transition = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: queen,
                to: Coord::new(1, 1),
            },
        )
        .unwrap();

        let outcome = MatchOutcome {
            winner: Some(Player::South),
            reason: OutcomeReason::Checkmate,
        };
        assert_eq!(transition.state.outcome, Some(outcome));
        assert!(
            transition
                .events
                .contains(&TransitionEvent::MatchEnded { outcome })
        );
    }

    #[test]
    fn mandatory_choice_blocks_command_without_mutating_state() {
        let scenario = scenario_with(vec![deployment(Player::South, PieceKind::Pawn, 0, 1)]);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = piece_id_at(&state, Coord::new(0, 1));
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn,
                site_index: 0,
            }],
        };
        let before = state.clone();

        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::Hold {
                    player: Player::South
                }
            ),
            Err(TransitionError::WrongTurnPhase)
        ));
        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::Move {
                    player: Player::South,
                    piece: pawn,
                    to: Coord::new(0, 2),
                }
            ),
            Err(TransitionError::WrongTurnPhase)
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn promotion_replaces_the_pawn_and_reports_stable_id_change() {
        let scenario = scenario_with(vec![deployment(Player::South, PieceKind::Pawn, 0, 1)]);
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = piece_id_at(&state, Coord::new(0, 1));
        let promoted = PieceId(state.next_piece_id);
        state.promotion_candidates.insert(pawn, 2);
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn,
                site_index: 0,
            }],
        };

        let transition = apply_action(
            &scenario,
            &state,
            &Action::ChoosePromotion {
                player: Player::South,
                pawn,
                promote_to: PromotionKind::Knight,
            },
        )
        .unwrap();

        assert!(!transition.state.pieces.contains_key(&pawn));
        assert_eq!(transition.state.pieces[&promoted].kind, PieceKind::Knight);
        assert_eq!(transition.state.phase, TurnPhase::Command);
        assert_eq!(transition.state.revision, state.revision + 1);
        assert_eq!(
            transition.events,
            vec![TransitionEvent::PiecePromoted {
                pawn,
                promoted,
                kind: PieceKind::Knight,
                at: Coord::new(0, 1),
            }]
        );
    }

    #[test]
    fn pawn_placement_consumes_only_the_first_queued_choice() {
        let mut scenario = scenario_with(Vec::new());
        scenario.settlements.push(SettlementSite {
            id: "test-settlement".to_owned(),
            at: Coord::new(2, 2),
        });
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].established = true;
        let first_at = Coord::new(2, 3);
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![
                MandatoryChoice::PlacePawn {
                    settlement_index: 0,
                    legal_squares: [first_at].into_iter().collect(),
                },
                MandatoryChoice::PlacePawn {
                    settlement_index: 0,
                    legal_squares: [Coord::new(3, 2)].into_iter().collect(),
                },
            ],
        };
        let pawn = PieceId(state.next_piece_id);

        let transition = apply_action(
            &scenario,
            &state,
            &Action::PlacePawn {
                player: Player::South,
                settlement_index: 0,
                at: first_at,
            },
        )
        .unwrap();

        assert_eq!(transition.state.pieces[&pawn].at, first_at);
        assert_eq!(
            transition.state.phase,
            TurnPhase::ResolvingChoices {
                queue: vec![MandatoryChoice::PlacePawn {
                    settlement_index: 0,
                    legal_squares: [Coord::new(3, 2)].into_iter().collect(),
                }]
            }
        );
        assert!(transition.events.contains(&TransitionEvent::PawnProduced {
            settlement_index: 0,
            pawn,
            at: first_at,
        }));
    }

    #[test]
    fn choice_errors_distinguish_actor_queue_and_target() {
        let mut scenario = scenario_with(Vec::new());
        scenario.settlements.push(SettlementSite {
            id: "test-settlement".to_owned(),
            at: Coord::new(2, 2),
        });
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.settlements[0].owner = Some(Player::South);
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::PlacePawn {
                settlement_index: 0,
                legal_squares: [Coord::new(2, 3)].into_iter().collect(),
            }],
        };

        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::PlacePawn {
                    player: Player::North,
                    settlement_index: 0,
                    at: Coord::new(2, 3),
                }
            ),
            Err(TransitionError::WrongPlayer { .. })
        ));
        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::ChoosePromotion {
                    player: Player::South,
                    pawn: PieceId(99),
                    promote_to: PromotionKind::Queen,
                }
            ),
            Err(TransitionError::ChoiceDoesNotMatch)
        ));
        assert!(matches!(
            apply_action(
                &scenario,
                &state,
                &Action::PlacePawn {
                    player: Player::South,
                    settlement_index: 0,
                    at: Coord::new(7, 7),
                }
            ),
            Err(TransitionError::IllegalPawnPlacement { .. })
        ));
    }
}
