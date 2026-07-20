use std::collections::BTreeMap;

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

    let next = match *action {
        Action::Move { player, piece, to } => apply_move(scenario, state, player, piece, to),
        Action::Hold { player } => apply_hold(scenario, state, player),
        Action::Resign { .. } | Action::OfferDraw { .. } | Action::RespondToDraw { .. } => {
            state.apply_non_board_action(action)
        }
        Action::ChoosePromotion {
            player,
            pawn,
            promote_to,
        } => apply_promotion_choice(state, player, pawn, promote_to),
        Action::PlacePawn {
            player,
            settlement_index,
            at,
        } => apply_pawn_placement(state, player, settlement_index, at),
    }?;
    let events = transition_events(state, &next, action);
    Ok(Transition {
        state: next,
        events,
    })
}

fn apply_promotion_choice(
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
    if before.active_player != after.active_player {
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
                    rook.owner == king.owner && rook.kind == PieceKind::Rook && !rook.has_moved
                })
            })
            .filter(|route| {
                route
                    .king_path
                    .iter()
                    .chain([&route.king_destination, &route.rook_destination])
                    .all(|at| {
                        *at == route.rook_start
                            || *at == king.at
                            || (!self.occupancy.contains_key(at)
                                && self.terrain(*at) != TileTerrain::Mountain)
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
        let dx = from.x.abs_diff(to.x);
        let dy = from.y.abs_diff(to.y);
        if dx <= 1 && dy <= 1 && dx + dy > 0 {
            if dx == 1 && dy == 1 {
                let horizontal = Coord::new(to.x, from.y);
                let vertical = Coord::new(from.x, to.y);
                return self.can_cross_edge(piece, Edge::new(from, horizontal))
                    && self.can_cross_edge(piece, Edge::new(from, vertical))
                    && self.can_cross_edge(piece, Edge::new(horizontal, to))
                    && self.can_cross_edge(piece, Edge::new(vertical, to));
            }
            return self.can_cross_edge(piece, Edge::new(from, to));
        }
        true
    }

    fn can_cross_edge(&self, piece: &Piece, edge: Edge) -> bool {
        match self.scenario.edges.get(&edge) {
            None | Some(EdgeKind::Bridge | EdgeKind::Ford | EdgeKind::Gate) => true,
            Some(EdgeKind::River) => false,
            Some(EdgeKind::Wall) => {
                piece.kind == PieceKind::Rook
                    && self.scenario.fortifications.iter().any(|fortification| {
                        fortification.owner == piece.owner
                            && fortification.tower == piece.at
                            && fortification.projected_wall == edge
                    })
            }
        }
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
    use std::collections::BTreeMap;

    use crate::{
        scenario::{
            BoardSize, CastlingRoute, Deployment, Edge, EdgeKind, SCENARIO_SCHEMA_VERSION,
            ScenarioMetadata, ScenarioRules, SettlementSite, TileTerrain,
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
        assert_eq!(state.pieces[&king].at, Coord::new(6, 7));
        assert!(
            state
                .pieces
                .values()
                .any(|piece| piece.kind == PieceKind::Rook && piece.at == Coord::new(5, 7))
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
        assert_eq!(
            transition.events,
            vec![TransitionEvent::PawnProduced {
                settlement_index: 0,
                pawn,
                at: first_at,
            }]
        );
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
