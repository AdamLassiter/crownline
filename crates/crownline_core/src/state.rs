use std::collections::{BTreeMap, BTreeSet};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::scenario::{Coord, PieceKind, Player, PromotionUnlockRules, ScenarioDefinition};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplorationState {
    pub north: BTreeSet<Coord>,
    pub south: BTreeSet<Coord>,
}

impl ExplorationState {
    #[must_use]
    pub const fn explored(&self, player: Player) -> &BTreeSet<Coord> {
        match player {
            Player::North => &self.north,
            Player::South => &self.south,
        }
    }

    fn explored_mut(&mut self, player: Player) -> &mut BTreeSet<Coord> {
        match player {
            Player::North => &mut self.north,
            Player::South => &mut self.south,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityState {
    Undiscovered,
    Explored,
    Visible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibilityCacheKey {
    scenario_id: String,
    board: crate::scenario::BoardSize,
    fog: Option<crate::scenario::FogRules>,
    revision: u64,
}

/// Non-canonical derived current vision, invalidated at scenario or revision boundaries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibilityCache {
    key: Option<VisibilityCacheKey>,
    north: BTreeSet<Coord>,
    south: BTreeSet<Coord>,
}

impl VisibilityCache {
    /// Returns one seat's derived visibility, recomputing both seats only when
    /// scenario geometry, fog configuration, or canonical revision changes.
    ///
    /// # Errors
    ///
    /// Returns scenario, identity, or canonical-state validation errors.
    pub fn visible<'a>(
        &'a mut self,
        scenario: &ScenarioDefinition,
        state: &MatchState,
        player: Player,
    ) -> Result<&'a BTreeSet<Coord>, TransitionError> {
        let key = VisibilityCacheKey {
            scenario_id: scenario.id.clone(),
            board: scenario.board,
            fog: scenario.rules.fog,
            revision: state.revision,
        };
        if self.key.as_ref() != Some(&key) {
            scenario
                .validate()
                .map_err(TransitionError::InvalidScenario)?;
            if state.scenario_id != scenario.id {
                return Err(TransitionError::ScenarioMismatch {
                    expected: state.scenario_id.clone(),
                    actual: scenario.id.clone(),
                });
            }
            state.validate_invariants()?;
            self.north = visible_coordinates_unchecked(scenario, state, Player::North);
            self.south = visible_coordinates_unchecked(scenario, state, Player::South);
            self.key = Some(key);
        }
        Ok(match player {
            Player::North => &self.north,
            Player::South => &self.south,
        })
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PieceId(pub u32);

impl<'de> Deserialize<'de> for PieceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PieceIdVisitor;

        impl<'de> Visitor<'de> for PieceIdVisitor {
            type Value = PieceId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a numeric piece ID or numeric JSON object key")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                u32::try_from(value)
                    .map(PieceId)
                    .map_err(|_| E::custom("piece ID exceeds u32"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                value
                    .parse::<u32>()
                    .map(PieceId)
                    .map_err(|_| E::custom("piece ID key is not a u32"))
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let value = sequence
                    .next_element::<u32>()?
                    .ok_or_else(|| serde::de::Error::custom("piece ID newtype is empty"))?;
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom(
                        "piece ID newtype contains more than one value",
                    ));
                }
                Ok(PieceId(value))
            }
        }

        deserializer.deserialize_any(PieceIdVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceOrigin {
    Deployed,
    Settlement { settlement_index: u16 },
    Promoted { from: PieceId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Piece {
    pub id: PieceId,
    pub owner: Player,
    pub kind: PieceKind,
    pub at: Coord,
    pub origin: PieceOrigin,
    pub has_moved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnPassantState {
    pub pawn: PieceId,
    pub capture_destination: Coord,
    pub expires_for: Player,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementState {
    pub site_index: u16,
    pub owner: Option<Player>,
    pub founder: Option<PieceId>,
    pub establishment_progress: u8,
    pub established: bool,
    pub production_progress: u8,
    pub produced_pawn: Option<PieceId>,
    pub cycle_interrupted: bool,
    pub completed_cycle_continuous: bool,
    pub transfer_candidate: Option<PieceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionKind {
    Queen,
    Rook,
    Bishop,
    Knight,
}

impl PromotionKind {
    pub const RECRUITMENT_ORDER: [Self; 4] = [Self::Knight, Self::Bishop, Self::Rook, Self::Queen];
}

/// Settlement counts contributing to one player's current promotion-control score.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealmControlScore {
    pub owned_settlements: u32,
    pub governed_settlements: u32,
    pub established_settlements: u32,
}

impl RealmControlScore {
    /// Returns ownership + governance + twice establishment.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.owned_settlements + self.governed_settlements + self.established_settlements * 2
    }
}

/// Immutable promotion choices captured for one owner-turn batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionEligibility {
    pub control: RealmControlScore,
    pub allowed_kinds: BTreeSet<PromotionKind>,
}

impl Default for PromotionEligibility {
    fn default() -> Self {
        Self::from_control(
            RealmControlScore::default(),
            PromotionUnlockRules::default(),
        )
    }
}

impl PromotionEligibility {
    #[must_use]
    pub fn from_control(control: RealmControlScore, unlocks: PromotionUnlockRules) -> Self {
        let total = control.total();
        let mut allowed_kinds = BTreeSet::from([PromotionKind::Knight]);
        if total >= unlocks.bishop {
            allowed_kinds.insert(PromotionKind::Bishop);
        }
        if total >= unlocks.rook {
            allowed_kinds.insert(PromotionKind::Rook);
        }
        if total >= unlocks.queen {
            allowed_kinds.insert(PromotionKind::Queen);
        }
        Self {
            control,
            allowed_kinds,
        }
    }

    #[must_use]
    pub fn allows(&self, kind: PromotionKind) -> bool {
        self.allowed_kinds.contains(&kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MandatoryChoice {
    Promote {
        pawn: PieceId,
        site_index: u16,
        #[serde(default)]
        eligibility: PromotionEligibility,
    },
    PlacePawn {
        settlement_index: u16,
        legal_squares: BTreeSet<Coord>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    ResolvingChoices { queue: Vec<MandatoryChoice> },
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockState {
    pub north_millis: u64,
    pub south_millis: u64,
    pub increment_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeReason {
    Checkmate,
    Timeout,
    Resignation,
    AgreedDraw,
    ThreefoldRepetition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchOutcome {
    pub winner: Option<Player>,
    pub reason: OutcomeReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchState {
    pub scenario_id: String,
    pub revision: u64,
    pub turn_number: u64,
    pub active_player: Player,
    pub phase: TurnPhase,
    pub pieces: BTreeMap<PieceId, Piece>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exploration: Option<ExplorationState>,
    pub settlements: Vec<SettlementState>,
    pub en_passant: Option<EnPassantState>,
    pub available_castling_routes: BTreeSet<String>,
    pub promotion_candidates: BTreeMap<PieceId, u8>,
    pub outstanding_draw_offer: Option<Player>,
    pub clocks: Option<ClockState>,
    pub repetition_counts: BTreeMap<String, u8>,
    pub outcome: Option<MatchOutcome>,
    pub next_piece_id: u32,
}

impl MatchState {
    /// Constructs the deterministic initial state for a validated scenario.
    ///
    /// # Errors
    ///
    /// Returns validation, capacity, or canonical-state invariant errors.
    pub fn from_scenario(scenario: &ScenarioDefinition) -> Result<Self, TransitionError> {
        scenario
            .validate()
            .map_err(TransitionError::InvalidScenario)?;

        if let Some(guided) = &scenario.guided {
            let mut state = guided.start.state.clone();
            state.validate_invariants()?;
            if state.repetition_counts.is_empty() {
                let key = state.repetition_key()?;
                state.repetition_counts.insert(key, 1);
            }
            return Ok(state);
        }

        let mut deployments = scenario.deployments.clone();
        deployments.sort_by_key(|piece| (piece.player, piece.at, piece.kind));
        let pieces = deployments
            .into_iter()
            .enumerate()
            .map(|(index, deployment)| {
                let id = PieceId(u32::try_from(index).map_err(|_| TransitionError::TooManyPieces)?);
                Ok((
                    id,
                    Piece {
                        id,
                        owner: deployment.player,
                        kind: deployment.kind,
                        at: deployment.at,
                        origin: PieceOrigin::Deployed,
                        has_moved: false,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, TransitionError>>()?;
        let next_piece_id = u32::try_from(scenario.deployments.len())
            .map_err(|_| TransitionError::TooManyPieces)?;

        let mut state = Self {
            scenario_id: scenario.id.clone(),
            revision: 0,
            turn_number: 1,
            active_player: Player::South,
            phase: TurnPhase::Command,
            pieces,
            exploration: scenario.rules.fog.map(|_| ExplorationState::default()),
            settlements: (0..scenario.settlements.len())
                .map(|index| {
                    Ok(SettlementState {
                        site_index: u16::try_from(index)
                            .map_err(|_| TransitionError::TooManySites)?,
                        owner: None,
                        founder: None,
                        establishment_progress: 0,
                        established: false,
                        production_progress: 0,
                        produced_pawn: None,
                        cycle_interrupted: false,
                        completed_cycle_continuous: false,
                        transfer_candidate: None,
                    })
                })
                .collect::<Result<Vec<_>, TransitionError>>()?,
            en_passant: None,
            available_castling_routes: scenario
                .castling_routes
                .iter()
                .map(|route| route.id.clone())
                .collect(),
            promotion_candidates: BTreeMap::new(),
            outstanding_draw_offer: None,
            clocks: None,
            repetition_counts: BTreeMap::new(),
            outcome: None,
            next_piece_id,
        };
        update_exploration_unchecked(scenario, &mut state);
        state.validate_invariants()?;
        let key = state.repetition_key()?;
        state.repetition_counts.insert(key, 1);
        Ok(state)
    }

    /// Computes a hash of the complete persisted canonical state.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical serialization fails.
    pub fn canonical_hash(&self) -> Result<String, TransitionError> {
        let bytes = serde_json::to_vec(self).map_err(TransitionError::Serialize)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    /// Computes the repetition identity, intentionally excluding clocks,
    /// revisions, presentation state, and the repetition counter itself.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical serialization fails.
    pub fn repetition_key(&self) -> Result<String, TransitionError> {
        #[derive(Serialize)]
        struct RepetitionView<'a> {
            active_player: Player,
            phase: &'a TurnPhase,
            pieces: &'a BTreeMap<PieceId, Piece>,
            settlements: &'a [SettlementState],
            en_passant: &'a Option<EnPassantState>,
            castling: &'a BTreeSet<String>,
            candidates: &'a BTreeMap<PieceId, u8>,
        }

        let view = RepetitionView {
            active_player: self.active_player,
            phase: &self.phase,
            pieces: &self.pieces,
            settlements: &self.settlements,
            en_passant: &self.en_passant,
            castling: &self.available_castling_routes,
            candidates: &self.promotion_candidates,
        };
        let bytes = serde_json::to_vec(&view).map_err(TransitionError::Serialize)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    /// Applies a match-level action without mutating the source state.
    ///
    /// # Errors
    ///
    /// Returns an error when the action is unauthorized, incompatible with the
    /// current phase, or belongs to board rules not yet routed through this API.
    pub fn apply_non_board_action(&self, action: &Action) -> Result<Self, TransitionError> {
        if self.outcome.is_some() {
            return Err(TransitionError::MatchFinished);
        }
        let mut next = self.clone();
        match *action {
            Action::Resign { player } => {
                if player != self.active_player {
                    return Err(TransitionError::WrongPlayer {
                        expected: self.active_player,
                        actual: player,
                    });
                }
                next.outcome = Some(MatchOutcome {
                    winner: Some(player.opponent()),
                    reason: OutcomeReason::Resignation,
                });
            }
            Action::OfferDraw { player } => {
                if player != self.active_player {
                    return Err(TransitionError::WrongPlayer {
                        expected: self.active_player,
                        actual: player,
                    });
                }
                if self.outstanding_draw_offer.is_some() {
                    return Err(TransitionError::DrawOfferAlreadyPending);
                }
                next.outstanding_draw_offer = Some(player);
            }
            Action::RespondToDraw { player, accept } => {
                let offering = self
                    .outstanding_draw_offer
                    .ok_or(TransitionError::NoDrawOffer)?;
                if player == offering {
                    return Err(TransitionError::WrongDrawResponder);
                }
                next.outstanding_draw_offer = None;
                if accept {
                    next.outcome = Some(MatchOutcome {
                        winner: None,
                        reason: OutcomeReason::AgreedDraw,
                    });
                }
            }
            _ => return Err(TransitionError::BoardRulesNotImplemented),
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(TransitionError::RevisionOverflow)?;
        next.validate_invariants()?;
        Ok(next)
    }

    /// Verifies internal references, occupancy, identity, and royal invariants.
    ///
    /// # Errors
    ///
    /// Returns the first canonical-state invariant violation.
    pub fn validate_invariants(&self) -> Result<(), TransitionError> {
        let mut occupied = BTreeSet::new();
        let mut kings = BTreeMap::from([(Player::North, 0_u8), (Player::South, 0_u8)]);
        for (id, piece) in &self.pieces {
            if *id != piece.id {
                return Err(TransitionError::MismatchedPieceId(*id));
            }
            if !occupied.insert(piece.at) {
                return Err(TransitionError::DuplicateOccupancy(piece.at));
            }
            if piece.kind == PieceKind::King {
                *kings.entry(piece.owner).or_default() += 1;
            }
        }
        for player in Player::ALL {
            if kings[&player] != 1 {
                return Err(TransitionError::InvalidKingCount {
                    player,
                    found: kings[&player],
                });
            }
        }
        for settlement in &self.settlements {
            for id in [
                settlement.founder,
                settlement.produced_pawn,
                settlement.transfer_candidate,
            ]
            .into_iter()
            .flatten()
            {
                if !self.pieces.contains_key(&id) {
                    return Err(TransitionError::DanglingPieceReference(id));
                }
            }
        }
        Ok(())
    }
}

/// Derives one seat's current visible coordinates from living friendly pieces.
///
/// Version-1 fog uses board-clipped Chebyshev distance and no blockers. Perfect-
/// information scenarios return every board coordinate.
///
/// # Errors
///
/// Returns scenario, identity, or canonical-state validation errors.
pub fn visible_coordinates(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
) -> Result<BTreeSet<Coord>, TransitionError> {
    scenario
        .validate()
        .map_err(TransitionError::InvalidScenario)?;
    if state.scenario_id != scenario.id {
        return Err(TransitionError::ScenarioMismatch {
            expected: state.scenario_id.clone(),
            actual: scenario.id.clone(),
        });
    }
    state.validate_invariants()?;
    Ok(visible_coordinates_unchecked(scenario, state, player))
}

/// Classifies one coordinate for a seat using durable exploration and derived vision.
///
/// # Errors
///
/// Returns scenario, state, identity, exploration, or coordinate errors.
pub fn visibility_at(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
    at: Coord,
) -> Result<VisibilityState, TransitionError> {
    if !at.is_within(scenario.board) {
        return Err(TransitionError::CoordinateOutOfBounds(at));
    }
    validate_exploration(scenario, state)?;
    if scenario.rules.fog.is_none() {
        return Ok(VisibilityState::Visible);
    }
    if visible_coordinates_unchecked(scenario, state, player).contains(&at) {
        return Ok(VisibilityState::Visible);
    }
    Ok(
        if state
            .exploration
            .as_ref()
            .is_some_and(|knowledge| knowledge.explored(player).contains(&at))
        {
            VisibilityState::Explored
        } else {
            VisibilityState::Undiscovered
        },
    )
}

/// Unions current vision into each seat's durable exploration knowledge.
///
/// Perfect-information states retain `None`, keeping their legacy serialized
/// representation. This function is also the single reconstruction primitive
/// used by creation and persistence migrations.
///
/// # Errors
///
/// Returns scenario, identity, or canonical-state validation errors.
pub fn update_exploration(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
) -> Result<(), TransitionError> {
    scenario
        .validate()
        .map_err(TransitionError::InvalidScenario)?;
    if state.scenario_id != scenario.id {
        return Err(TransitionError::ScenarioMismatch {
            expected: state.scenario_id.clone(),
            actual: scenario.id.clone(),
        });
    }
    state.validate_invariants()?;
    update_exploration_unchecked(scenario, state);
    Ok(())
}

/// Validates that durable exploration matches the scenario mode and contains
/// current vision for both seats.
///
/// # Errors
///
/// Returns typed errors for mode mismatch, out-of-board knowledge, or a missing
/// current-visible coordinate.
pub fn validate_exploration(
    scenario: &ScenarioDefinition,
    state: &MatchState,
) -> Result<(), TransitionError> {
    scenario
        .validate()
        .map_err(TransitionError::InvalidScenario)?;
    validate_exploration_unchecked(scenario, state)
}

pub(crate) fn validate_exploration_unchecked(
    scenario: &ScenarioDefinition,
    state: &MatchState,
) -> Result<(), TransitionError> {
    if state.scenario_id != scenario.id {
        return Err(TransitionError::ScenarioMismatch {
            expected: state.scenario_id.clone(),
            actual: scenario.id.clone(),
        });
    }
    state.validate_invariants()?;
    match (scenario.rules.fog, &state.exploration) {
        (None, None) => Ok(()),
        (None, Some(_)) | (Some(_), None) => Err(TransitionError::ExplorationModeMismatch),
        (Some(_), Some(knowledge)) => {
            for player in Player::ALL {
                for &at in knowledge.explored(player) {
                    if !at.is_within(scenario.board) {
                        return Err(TransitionError::ExploredCoordinateOutOfBounds { player, at });
                    }
                }
                if let Some(at) = visible_coordinates_unchecked(scenario, state, player)
                    .difference(knowledge.explored(player))
                    .next()
                    .copied()
                {
                    return Err(TransitionError::VisibleCoordinateNotExplored { player, at });
                }
            }
            Ok(())
        }
    }
}

fn update_exploration_unchecked(scenario: &ScenarioDefinition, state: &mut MatchState) {
    let Some(_) = scenario.rules.fog else {
        state.exploration = None;
        return;
    };
    let visible = Player::ALL.map(|player| {
        (
            player,
            visible_coordinates_unchecked(scenario, state, player),
        )
    });
    let knowledge = state
        .exploration
        .get_or_insert_with(ExplorationState::default);
    for (player, coordinates) in visible {
        knowledge.explored_mut(player).extend(coordinates);
    }
}

fn visible_coordinates_unchecked(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
) -> BTreeSet<Coord> {
    let Some(fog) = scenario.rules.fog else {
        return (0..scenario.board.width)
            .flat_map(|x| (0..scenario.board.height).map(move |y| Coord::new(x, y)))
            .collect();
    };
    let friendly: Vec<_> = state
        .pieces
        .values()
        .filter(|piece| piece.owner == player)
        .map(|piece| piece.at)
        .collect();
    (0..scenario.board.width)
        .flat_map(|x| (0..scenario.board.height).map(move |y| Coord::new(x, y)))
        .filter(|at| {
            friendly.iter().any(|piece| {
                u32::from(piece.x.abs_diff(at.x)).max(u32::from(piece.y.abs_diff(at.y)))
                    <= fog.vision_radius
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Move {
        player: Player,
        piece: PieceId,
        to: Coord,
    },
    Hold {
        player: Player,
    },
    ChoosePromotion {
        player: Player,
        pawn: PieceId,
        promote_to: PromotionKind,
    },
    PlacePawn {
        player: Player,
        settlement_index: u16,
        at: Coord,
    },
    Resign {
        player: Player,
    },
    OfferDraw {
        player: Player,
    },
    RespondToDraw {
        player: Player,
        accept: bool,
    },
}

#[derive(Debug, Error)]
pub enum TransitionError {
    #[error("scenario is invalid: {0:?}")]
    InvalidScenario(Vec<crate::scenario::ScenarioError>),
    #[error("scenario contains too many pieces")]
    TooManyPieces,
    #[error("scenario contains too many sites")]
    TooManySites,
    #[error("failed to serialize canonical state: {0}")]
    Serialize(serde_json::Error),
    #[error("match is already finished")]
    MatchFinished,
    #[error("state belongs to scenario {expected:?}, not {actual:?}")]
    ScenarioMismatch { expected: String, actual: String },
    #[error("expected action from {expected:?}, got {actual:?}")]
    WrongPlayer { expected: Player, actual: Player },
    #[error("a draw offer is already pending")]
    DrawOfferAlreadyPending,
    #[error("there is no draw offer to answer")]
    NoDrawOffer,
    #[error("the player who offered a draw cannot answer it")]
    WrongDrawResponder,
    #[error("board action rules are not implemented yet")]
    BoardRulesNotImplemented,
    #[error("action is not valid during the current turn phase")]
    WrongTurnPhase,
    #[error("action does not resolve the next mandatory choice")]
    ChoiceDoesNotMatch,
    #[error("piece {0:?} is not an eligible promotion Pawn")]
    InvalidPromotionPawn(PieceId),
    #[error(
        "promotion to {requested:?} requires control score {required_score}, but this batch froze at {control_score}"
    )]
    PromotionKindLocked {
        requested: PromotionKind,
        control_score: u32,
        required_score: u32,
    },
    #[error("queued promotion eligibility does not match the scenario unlock rules")]
    InvalidPromotionEligibility,
    #[error("promotion would leave the active King in check")]
    PromotionLeavesKingInCheck,
    #[error("promotion candidate progress overflowed")]
    PromotionProgressOverflow,
    #[error("{at:?} is not a legal placement for settlement {settlement_index}")]
    IllegalPawnPlacement { settlement_index: u16, at: Coord },
    #[error("settlement {0} is missing")]
    MissingSettlement(u16),
    #[error("settlement {0} cannot produce a Pawn for this action")]
    SettlementCannotProduce(u16),
    #[error("piece {piece:?} cannot legally move to {to:?}")]
    IllegalMove { piece: PieceId, to: Coord },
    #[error("coordinate {0:?} is outside the board")]
    CoordinateOutOfBounds(Coord),
    #[error("piece {0:?} is missing")]
    MissingPiece(PieceId),
    #[error("{0:?} has no King")]
    MissingKing(Player),
    #[error("Kings end through checkmate and cannot be captured")]
    CannotCaptureKing,
    #[error("castling route is no longer valid")]
    InvalidCastlingRoute,
    #[error("Hold is illegal while the active King is in check")]
    CannotHoldInCheck,
    #[error("clock base must be between 1 and 180 minutes, got {0}")]
    InvalidClockBase(u16),
    #[error("clock increment must be between 0 and 60 seconds, got {0}")]
    InvalidClockIncrement(u8),
    #[error("clocks have already started")]
    ClocksAlreadyStarted,
    #[error("clocks must be configured before the match starts")]
    ClocksMustStartWithMatch,
    #[error("clock value overflowed")]
    ClockOverflow,
    #[error("match revision overflowed")]
    RevisionOverflow,
    #[error("turn number overflowed")]
    TurnOverflow,
    #[error("piece identity counter overflowed")]
    PieceIdOverflow,
    #[error("piece map key {0:?} does not match the piece id")]
    MismatchedPieceId(PieceId),
    #[error("multiple pieces occupy {0:?}")]
    DuplicateOccupancy(Coord),
    #[error("{player:?} must have one King, found {found}")]
    InvalidKingCount { player: Player, found: u8 },
    #[error("canonical state references missing piece {0:?}")]
    DanglingPieceReference(PieceId),
    #[error("canonical exploration presence does not match the scenario fog mode")]
    ExplorationModeMismatch,
    #[error("{player:?} exploration contains out-of-board coordinate {at:?}")]
    ExploredCoordinateOutOfBounds { player: Player, at: Coord },
    #[error("{player:?} current-visible coordinate {at:?} is missing from exploration")]
    VisibleCoordinateNotExplored { player: Player, at: Coord },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::scenario::{
        BoardSize, Deployment, FOG_RULES_SCHEMA_VERSION, FogRules, SCENARIO_SCHEMA_VERSION,
        ScenarioMetadata, ScenarioRules,
    };

    use super::*;

    #[test]
    fn piece_id_accepts_ron_newtype_and_rejects_malformed_sequences() {
        let encoded = ron::to_string(&PieceId(17)).unwrap();
        assert_eq!(ron::from_str::<PieceId>(&encoded).unwrap(), PieceId(17));
        assert!(ron::from_str::<PieceId>("()").is_err());
        assert!(ron::from_str::<PieceId>("(17, 18)").is_err());
        assert_eq!(serde_json::from_str::<PieceId>("17").unwrap(), PieceId(17));
    }

    fn scenario() -> ScenarioDefinition {
        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "state-test".to_owned(),
            metadata: ScenarioMetadata {
                name: "State test".to_owned(),
                description: String::new(),
                expected_minutes: (30, 45),
                is_default: false,
            },
            board: BoardSize {
                width: 16,
                height: 16,
            },
            terrain: BTreeMap::new(),
            edges: BTreeMap::new(),
            deployments: vec![
                Deployment {
                    player: Player::South,
                    kind: PieceKind::King,
                    at: Coord::new(4, 14),
                },
                Deployment {
                    player: Player::North,
                    kind: PieceKind::King,
                    at: Coord::new(4, 1),
                },
            ],
            settlements: vec![],
            promotion_sites: vec![],
            keeps: vec![],
            fortifications: vec![],
            castling_routes: vec![],
            rules: ScenarioRules {
                army_setup: crate::scenario::ArmySetup::Custom,
                ..ScenarioRules::default()
            },
            guided: None,
        }
    }

    #[test]
    fn promotion_eligibility_unlocks_cumulatively_at_exact_boundaries() {
        let rules = PromotionUnlockRules::default();
        let kinds_at = |score| {
            PromotionEligibility::from_control(
                RealmControlScore {
                    owned_settlements: score,
                    ..RealmControlScore::default()
                },
                rules,
            )
            .allowed_kinds
        };

        assert_eq!(kinds_at(0), BTreeSet::from([PromotionKind::Knight]));
        assert_eq!(kinds_at(1), BTreeSet::from([PromotionKind::Knight]));
        assert_eq!(
            kinds_at(2),
            BTreeSet::from([PromotionKind::Knight, PromotionKind::Bishop])
        );
        assert_eq!(kinds_at(3), kinds_at(2));
        assert_eq!(
            kinds_at(4),
            BTreeSet::from([
                PromotionKind::Knight,
                PromotionKind::Bishop,
                PromotionKind::Rook,
            ])
        );
        assert_eq!(kinds_at(7), kinds_at(4));
        assert_eq!(
            kinds_at(8),
            BTreeSet::from(PromotionKind::RECRUITMENT_ORDER)
        );
    }

    #[test]
    fn construction_and_hash_are_deterministic() {
        let first = MatchState::from_scenario(&scenario()).expect("valid state");
        let second = MatchState::from_scenario(&scenario()).expect("valid state");
        assert_eq!(first, second);
        assert_eq!(
            first.canonical_hash().unwrap(),
            second.canonical_hash().unwrap()
        );
        assert!(first.exploration.is_none());
        assert!(
            !serde_json::to_string(&first)
                .unwrap()
                .contains("exploration")
        );
    }

    #[test]
    fn fog_creation_and_moves_update_independent_monotonic_knowledge() {
        let mut scenario = scenario();
        scenario.rules.fog = Some(FogRules {
            schema_version: FOG_RULES_SCHEMA_VERSION,
            vision_radius: 1,
        });
        let state = MatchState::from_scenario(&scenario).unwrap();
        let knowledge = state.exploration.as_ref().unwrap();
        let north_before = knowledge.north.clone();
        assert_eq!(knowledge.north.len(), 9);
        assert_eq!(knowledge.south.len(), 9);
        assert_eq!(
            visibility_at(&scenario, &state, Player::South, Coord::new(3, 15)).unwrap(),
            VisibilityState::Visible
        );
        assert_eq!(
            visibility_at(&scenario, &state, Player::South, Coord::new(0, 0)).unwrap(),
            VisibilityState::Undiscovered
        );

        let king = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South)
            .unwrap()
            .id;
        let moved = crate::rules::apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: king,
                to: Coord::new(4, 13),
            },
        )
        .unwrap()
        .state;
        let moved_knowledge = moved.exploration.as_ref().unwrap();
        assert_eq!(&moved_knowledge.north, &north_before);
        assert!(moved_knowledge.south.is_superset(&knowledge.south));
        assert_eq!(moved_knowledge.south.len(), 12);
        assert_eq!(
            visibility_at(&scenario, &moved, Player::South, Coord::new(3, 15)).unwrap(),
            VisibilityState::Explored
        );
        assert_eq!(
            visibility_at(&scenario, &moved, Player::South, Coord::new(3, 12)).unwrap(),
            VisibilityState::Visible
        );
        validate_exploration(&scenario, &moved).unwrap();

        let mut cache = VisibilityCache::default();
        assert!(
            cache
                .visible(&scenario, &state, Player::South)
                .unwrap()
                .contains(&Coord::new(3, 15))
        );
        assert_eq!(cache.key.as_ref().unwrap().revision, 0);
        assert!(
            !cache
                .visible(&scenario, &moved, Player::South)
                .unwrap()
                .contains(&Coord::new(3, 15))
        );
        assert_eq!(cache.key.as_ref().unwrap().revision, 1);

        let rematch = MatchState::from_scenario(&scenario).unwrap();
        assert_eq!(rematch.exploration, state.exploration);
        assert_ne!(rematch.exploration, moved.exploration);
    }

    #[test]
    fn exploration_changes_canonical_hash_but_not_repetition_identity() {
        let mut scenario = scenario();
        scenario.rules.fog = Some(FogRules {
            schema_version: FOG_RULES_SCHEMA_VERSION,
            vision_radius: 1,
        });
        let state = MatchState::from_scenario(&scenario).unwrap();
        let mut learned = state.clone();
        learned
            .exploration
            .as_mut()
            .unwrap()
            .south
            .insert(Coord::new(0, 0));
        assert_ne!(
            state.canonical_hash().unwrap(),
            learned.canonical_hash().unwrap()
        );
        assert_eq!(
            state.repetition_key().unwrap(),
            learned.repetition_key().unwrap()
        );
    }

    #[test]
    fn exploration_validation_rejects_mode_bounds_and_missing_current_vision() {
        let mut fog = scenario();
        fog.rules.fog = Some(FogRules {
            schema_version: FOG_RULES_SCHEMA_VERSION,
            vision_radius: 1,
        });
        let state = MatchState::from_scenario(&fog).unwrap();

        let mut missing = state.clone();
        missing.exploration.as_mut().unwrap().south.clear();
        assert!(matches!(
            validate_exploration(&fog, &missing),
            Err(TransitionError::VisibleCoordinateNotExplored {
                player: Player::South,
                ..
            })
        ));

        let mut outside = state.clone();
        outside
            .exploration
            .as_mut()
            .unwrap()
            .north
            .insert(Coord::new(16, 0));
        assert!(matches!(
            validate_exploration(&fog, &outside),
            Err(TransitionError::ExploredCoordinateOutOfBounds {
                player: Player::North,
                at: Coord { x: 16, y: 0 }
            })
        ));

        let mut perfect = MatchState::from_scenario(&scenario()).unwrap();
        perfect.exploration = Some(ExplorationState::default());
        assert!(matches!(
            validate_exploration(&scenario(), &perfect),
            Err(TransitionError::ExplorationModeMismatch)
        ));
    }

    #[test]
    fn populated_state_round_trips_piece_ids_through_json_object_keys() {
        let state = MatchState::from_scenario(&scenario()).expect("valid state");
        let bytes = serde_json::to_vec(&state).unwrap();
        let decoded: MatchState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, state);
        assert_eq!(
            decoded.canonical_hash().unwrap(),
            state.canonical_hash().unwrap()
        );
    }

    #[test]
    fn repetition_identity_covers_gameplay_state_but_excludes_clocks_and_revision() {
        let state = MatchState::from_scenario(&scenario()).unwrap();
        let key = state.repetition_key().unwrap();

        let mut metadata_only = state.clone();
        metadata_only.revision = 42;
        metadata_only.turn_number = 99;
        metadata_only.clocks = Some(ClockState {
            north_millis: 1,
            south_millis: 2,
            increment_millis: 3,
        });
        assert_eq!(metadata_only.repetition_key().unwrap(), key);

        let first_piece = *state.pieces.keys().next().unwrap();
        let mut variants = Vec::new();

        let mut changed = state.clone();
        changed.active_player = changed.active_player.opponent();
        variants.push(changed);

        let mut changed = state.clone();
        changed.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn: first_piece,
                site_index: 0,
                eligibility: PromotionEligibility::default(),
            }],
        };
        variants.push(changed);

        let mut changed = state.clone();
        changed.pieces.get_mut(&first_piece).unwrap().origin =
            PieceOrigin::Promoted { from: PieceId(99) };
        variants.push(changed);

        let mut changed = state.clone();
        changed.pieces.get_mut(&first_piece).unwrap().has_moved = true;
        variants.push(changed);

        let mut changed = state.clone();
        changed.en_passant = Some(EnPassantState {
            pawn: first_piece,
            capture_destination: Coord::new(4, 2),
            expires_for: Player::South,
        });
        variants.push(changed);

        let mut changed = state.clone();
        changed.available_castling_routes.insert("route".to_owned());
        variants.push(changed);

        let mut changed = state.clone();
        changed.settlements.push(SettlementState {
            site_index: 0,
            owner: Some(Player::South),
            founder: Some(first_piece),
            establishment_progress: 1,
            established: false,
            production_progress: 0,
            produced_pawn: None,
            cycle_interrupted: false,
            completed_cycle_continuous: true,
            transfer_candidate: None,
        });
        variants.push(changed);

        let mut changed = state.clone();
        changed.promotion_candidates.insert(first_piece, 1);
        variants.push(changed);

        for changed in variants {
            assert_ne!(changed.repetition_key().unwrap(), key);
        }
    }

    #[test]
    fn invalid_action_is_transactional() {
        let state = MatchState::from_scenario(&scenario()).expect("valid state");
        let before = state.clone();
        let result = state.apply_non_board_action(&Action::Hold {
            player: Player::South,
        });
        assert!(matches!(
            result,
            Err(TransitionError::BoardRulesNotImplemented)
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn draw_offer_can_be_accepted_by_opponent() {
        let state = MatchState::from_scenario(&scenario()).expect("valid state");
        let state = state
            .apply_non_board_action(&Action::OfferDraw {
                player: Player::South,
            })
            .unwrap();
        let state = state
            .apply_non_board_action(&Action::RespondToDraw {
                player: Player::North,
                accept: true,
            })
            .unwrap();
        assert_eq!(
            state.outcome,
            Some(MatchOutcome {
                winner: None,
                reason: OutcomeReason::AgreedDraw
            })
        );
    }
}
