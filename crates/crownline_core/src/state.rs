use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::scenario::{Coord, PieceKind, Player, ScenarioDefinition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PieceId(pub u32);

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
    pub transfer_candidate: Option<PieceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionKind {
    Queen,
    Rook,
    Bishop,
    Knight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MandatoryChoice {
    Promote {
        pawn: PieceId,
        site_index: u16,
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
    #[error("piece {piece:?} cannot legally move to {to:?}")]
    IllegalMove { piece: PieceId, to: Coord },
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
    #[error("match revision overflowed")]
    RevisionOverflow,
    #[error("turn number overflowed")]
    TurnOverflow,
    #[error("piece map key {0:?} does not match the piece id")]
    MismatchedPieceId(PieceId),
    #[error("multiple pieces occupy {0:?}")]
    DuplicateOccupancy(Coord),
    #[error("{player:?} must have one King, found {found}")]
    InvalidKingCount { player: Player, found: u8 },
    #[error("canonical state references missing piece {0:?}")]
    DanglingPieceReference(PieceId),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::scenario::{
        BoardSize, Deployment, SCENARIO_SCHEMA_VERSION, ScenarioMetadata, ScenarioRules,
    };

    use super::*;

    fn scenario() -> ScenarioDefinition {
        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "state-test".to_owned(),
            metadata: ScenarioMetadata {
                name: "State test".to_owned(),
                description: String::new(),
                expected_minutes: (30, 45),
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
            fortifications: vec![],
            castling_routes: vec![],
            rules: ScenarioRules {
                require_standard_armies: false,
                ..ScenarioRules::default()
            },
        }
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
