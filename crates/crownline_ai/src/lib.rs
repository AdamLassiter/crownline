//! Replaceable deterministic search boundaries for Crownlines opponents.
//!
//! This crate consumes the canonical reducer. It is never a rules authority,
//! and `crownline_core` deliberately has no dependency on it.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use crownline_core::{
    Action, MatchState, ScenarioDefinition, TransitionError, apply_action,
    legal_mandatory_choice_actions, legal_moves,
    scenario::{Coord, Player},
    state::{PieceId, PromotionKind, TurnPhase},
};
use thiserror::Error;

mod search;
pub use search::{AlphaBetaSearch, StableMoveOrderer, is_noisy_action};

pub const MATE_SCORE: i32 = 1_000_000;
pub const MAX_HEURISTIC_SCORE: i32 = MATE_SCORE / 2;
pub const DRAW_SCORE: i32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLimits {
    pub max_depth: u16,
    pub max_nodes: u64,
    pub max_quiescence_depth: u16,
    pub max_quiescence_nodes: u64,
    pub deadline: Option<Instant>,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_nodes: 50_000,
            max_quiescence_depth: 2,
            max_quiescence_nodes: 10_000,
            deadline: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Completed,
    DepthLimit,
    NodeLimit,
    QuiescenceNodeLimit,
    Deadline,
    Cancelled,
    NoLegalAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub score: i32,
    pub components: Vec<ScoreComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreComponent {
    pub name: &'static str,
    pub value: i32,
}

pub trait Evaluator: Send + Sync {
    /// Scores a non-terminal state from `root`'s perspective.
    ///
    /// # Errors
    ///
    /// Returns a typed evaluator failure without mutating the state.
    fn evaluate(
        &self,
        scenario: &ScenarioDefinition,
        state: &MatchState,
        root: Player,
    ) -> Result<Evaluation, EvaluationError>;
}

pub trait MoveOrderer: Send + Sync {
    fn order(
        &self,
        scenario: &ScenarioDefinition,
        state: &MatchState,
        actions: &mut [Action],
        prior_pv: Option<&Action>,
    );
}

pub trait SearchPolicy: Send + Sync {
    /// Searches an immutable canonical position using explicit collaborators.
    ///
    /// # Errors
    ///
    /// Returns rules, evaluation, or invalid-limit failures without modifying input.
    fn search(&self, request: SearchRequest<'_>) -> Result<SearchResult, SearchError>;
}

pub struct SearchRequest<'a> {
    pub scenario: &'a ScenarioDefinition,
    pub state: &'a MatchState,
    pub root: Player,
    pub evaluator: &'a dyn Evaluator,
    pub orderer: &'a dyn MoveOrderer,
    pub limits: SearchLimits,
    pub cancellation: &'a dyn Cancellation,
}

pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

#[derive(Debug, Default)]
pub struct CancellationToken(AtomicBool);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Cancellation for CancellationToken {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub action: Option<Action>,
    pub score: i32,
    pub completed_depth: u16,
    pub principal_variation: Vec<Action>,
    pub nodes: u64,
    pub quiescence_nodes: u64,
    pub cutoffs: u64,
    pub stop_reason: StopReason,
    pub tie_break: Option<ActionKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EvaluationError {
    #[error("evaluator failed: {0}")]
    Failed(String),
    #[error("heuristic score {0} exceeds the non-terminal score range")]
    OutOfRange(i32),
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search limits must include a positive depth and node budget")]
    InvalidLimits,
    #[error("rules expansion failed: {0}")]
    Rules(#[from] TransitionError),
    #[error(transparent)]
    Evaluation(#[from] EvaluationError),
}

/// Canonical deterministic identity used as the final action tie-break.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionKey {
    Move(Player, PieceId, Coord),
    Hold(Player),
    Promote(Player, PieceId, PromotionKind),
    PlacePawn(Player, u16, Coord),
}

impl ActionKey {
    pub fn from_action(action: &Action) -> Option<Self> {
        match *action {
            Action::Move { player, piece, to } => Some(Self::Move(player, piece, to)),
            Action::Hold { player } => Some(Self::Hold(player)),
            Action::ChoosePromotion {
                player,
                pawn,
                promote_to,
            } => Some(Self::Promote(player, pawn, promote_to)),
            Action::PlacePawn {
                player,
                settlement_index,
                at,
            } => Some(Self::PlacePawn(player, settlement_index, at)),
            Action::Resign { .. } | Action::OfferDraw { .. } | Action::RespondToDraw { .. } => None,
        }
    }
}

/// Enumerates exactly the reducer actions the search may expand in stable order.
///
/// # Errors
///
/// Returns a core rules error if legal move generation or Hold validation fails.
pub fn legal_search_actions(
    scenario: &ScenarioDefinition,
    state: &MatchState,
) -> Result<Vec<Action>, TransitionError> {
    if state.outcome.is_some() {
        return Ok(Vec::new());
    }
    let mut actions = match state.phase {
        TurnPhase::ResolvingChoices { .. } => legal_mandatory_choice_actions(state),
        TurnPhase::Command => {
            let mut actions: Vec<_> = legal_moves(scenario, state)?
                .into_iter()
                .map(|candidate| Action::Move {
                    player: state.active_player,
                    piece: candidate.piece,
                    to: candidate.to,
                })
                .collect();
            let hold = Action::Hold {
                player: state.active_player,
            };
            if apply_action(scenario, state, &hold).is_ok() {
                actions.push(hold);
            }
            actions
        }
    };
    actions.sort_by_key(ActionKey::from_action);
    Ok(actions)
}

#[must_use]
pub fn maximizing_after(state: &MatchState, root: Player) -> bool {
    state.active_player == root
}

#[must_use]
pub fn terminal_score(winner: Option<Player>, root: Player, ply: u16) -> i32 {
    match winner {
        Some(player) if player == root => MATE_SCORE - i32::from(ply),
        Some(_) => -MATE_SCORE + i32::from(ply),
        None => DRAW_SCORE,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crownline_core::{
        PromotionEligibility,
        state::{MandatoryChoice, MatchOutcome, OutcomeReason, RealmControlScore},
    };

    use super::*;

    fn fixture() -> (ScenarioDefinition, MatchState) {
        let scenario =
            ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        (scenario, state)
    }

    #[test]
    fn command_actions_are_nonempty_stable_and_include_legal_hold() {
        let (scenario, state) = fixture();
        let first = legal_search_actions(&scenario, &state).unwrap();
        assert!(!first.is_empty());
        assert!(first.contains(&Action::Hold {
            player: Player::South
        }));
        assert_eq!(first, legal_search_actions(&scenario, &state).unwrap());
        assert!(
            first.windows(2).all(|pair| {
                ActionKey::from_action(&pair[0]) <= ActionKey::from_action(&pair[1])
            })
        );
    }

    #[test]
    fn frozen_promotion_choice_never_expands_a_locked_kind_or_flips_side() {
        let (scenario, mut state) = fixture();
        let pawn = *state.pieces.keys().next().unwrap();
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn,
                site_index: 0,
                eligibility: PromotionEligibility {
                    control: RealmControlScore::default(),
                    allowed_kinds: BTreeSet::from([PromotionKind::Knight, PromotionKind::Bishop]),
                },
            }],
        };
        let actions = legal_search_actions(&scenario, &state).unwrap();
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().all(|action| !matches!(
            action,
            Action::ChoosePromotion {
                promote_to: PromotionKind::Queen | PromotionKind::Rook,
                ..
            }
        )));
        assert!(maximizing_after(&state, state.active_player));
    }

    #[test]
    fn pawn_placement_uses_only_reducer_squares_and_terminal_has_none() {
        let (scenario, mut state) = fixture();
        let legal_squares = BTreeSet::from([Coord::new(2, 2), Coord::new(3, 3)]);
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::PlacePawn {
                settlement_index: 0,
                legal_squares: legal_squares.clone(),
            }],
        };
        let actions = legal_search_actions(&scenario, &state).unwrap();
        assert_eq!(
            actions
                .iter()
                .filter_map(|action| match action {
                    Action::PlacePawn { at, .. } => Some(*at),
                    _ => None,
                })
                .collect::<BTreeSet<_>>(),
            legal_squares
        );
        state.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        assert!(legal_search_actions(&scenario, &state).unwrap().is_empty());
    }

    #[test]
    fn terminal_scores_prefer_faster_wins_and_slower_losses() {
        assert!(
            terminal_score(Some(Player::South), Player::South, 2)
                > terminal_score(Some(Player::South), Player::South, 5)
        );
        assert!(
            terminal_score(Some(Player::North), Player::South, 5)
                > terminal_score(Some(Player::North), Player::South, 2)
        );
        assert_eq!(terminal_score(None, Player::South, 0), DRAW_SCORE);
    }
}
