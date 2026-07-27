use std::time::Instant;

use crownline_core::{
    Action, MatchState, ScenarioDefinition, Transition, TransitionError, TransitionEvent,
    apply_action, is_in_check, state::TurnPhase,
};

use crate::{
    ActionKey, Cancellation, EvaluationError, Evaluator, MAX_HEURISTIC_SCORE, MoveOrderer,
    SearchError, SearchLimits, SearchPolicy, SearchRequest, SearchResult, StopReason,
    legal_search_actions, maximizing_after, terminal_score,
};

const MAX_SEARCH_DEPTH: u16 = 64;
const NEG_INFINITY: i32 = -crate::MATE_SCORE - 1;
const POS_INFINITY: i32 = crate::MATE_SCORE + 1;

#[derive(Debug, Default)]
pub struct StableMoveOrderer;

impl MoveOrderer for StableMoveOrderer {
    fn order(
        &self,
        scenario: &ScenarioDefinition,
        state: &MatchState,
        actions: &mut [Action],
        prior_pv: Option<&Action>,
    ) {
        actions.sort_by_key(|action| {
            (
                tactical_priority(scenario, state, action, prior_pv),
                ActionKey::from_action(action),
            )
        });
    }
}

/// Returns whether an action belongs to the deliberately bounded quiescence set.
///
/// # Errors
///
/// Returns a canonical rules error if the supplied action is not applicable.
pub fn is_noisy_action(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    action: &Action,
) -> Result<bool, TransitionError> {
    let transition = apply_action(scenario, state, action)?;
    Ok(is_noisy_transition(scenario, state, action, &transition))
}

fn tactical_priority(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    action: &Action,
    prior_pv: Option<&Action>,
) -> u8 {
    if prior_pv == Some(action) {
        return 0;
    }
    let Ok(transition) = apply_action(scenario, state, action) else {
        return u8::MAX;
    };
    if transition.state.outcome.is_some() {
        return 1;
    }
    if matches!(state.phase, TurnPhase::ResolvingChoices { .. }) {
        return 2;
    }
    if matches!(action, Action::ChoosePromotion { .. }) {
        return 3;
    }
    if has_event(&transition, |event| {
        matches!(event, TransitionEvent::PieceCaptured { .. })
    }) {
        return 4;
    }
    if gives_check(scenario, state, &transition) {
        return 5;
    }
    if has_realm_event(&transition) {
        return 6;
    }
    7
}

fn is_noisy_transition(
    scenario: &ScenarioDefinition,
    before: &MatchState,
    action: &Action,
    transition: &Transition,
) -> bool {
    matches!(before.phase, TurnPhase::ResolvingChoices { .. })
        || matches!(action, Action::ChoosePromotion { .. })
        || has_event(transition, |event| {
            matches!(event, TransitionEvent::PieceCaptured { .. })
        })
        || transition.state.outcome.is_some()
        || gives_check(scenario, before, transition)
        || has_realm_event(transition)
}

fn gives_check(
    scenario: &ScenarioDefinition,
    before: &MatchState,
    transition: &Transition,
) -> bool {
    let opponent = before.active_player.opponent();
    is_in_check(scenario, &transition.state, opponent).unwrap_or(false)
}

fn has_realm_event(transition: &Transition) -> bool {
    has_event(transition, |event| {
        matches!(
            event,
            TransitionEvent::SettlementContinuityInterrupted { .. }
                | TransitionEvent::SettlementClaimed { .. }
                | TransitionEvent::SettlementContested { .. }
                | TransitionEvent::SettlementTransferCancelled { .. }
                | TransitionEvent::SettlementTransferred { .. }
                | TransitionEvent::SettlementDevelopmentAdvanced { .. }
                | TransitionEvent::SettlementDevelopmentReset { .. }
                | TransitionEvent::SettlementEstablished { .. }
                | TransitionEvent::SettlementProductionAdvanced { .. }
                | TransitionEvent::SettlementProductionReset { .. }
                | TransitionEvent::PawnProduced { .. }
                | TransitionEvent::PawnPlacementReady { .. }
                | TransitionEvent::PromotionCandidateStarted { .. }
                | TransitionEvent::PromotionCandidateAdvanced { .. }
                | TransitionEvent::PromotionCandidateCancelled { .. }
                | TransitionEvent::PromotionReady { .. }
        )
    })
}

fn has_event(transition: &Transition, predicate: impl Fn(&TransitionEvent) -> bool) -> bool {
    transition.events.iter().any(predicate)
}

#[derive(Debug, Default)]
pub struct AlphaBetaSearch;

impl SearchPolicy for AlphaBetaSearch {
    fn search(&self, request: SearchRequest<'_>) -> Result<SearchResult, SearchError> {
        if request.limits.max_depth == 0
            || request.limits.max_depth > MAX_SEARCH_DEPTH
            || request.limits.max_nodes == 0
            || (request.limits.max_quiescence_depth > 0 && request.limits.max_quiescence_nodes == 0)
        {
            return Err(SearchError::InvalidLimits);
        }
        let root_actions = legal_search_actions(request.scenario, request.state)?;
        if root_actions.is_empty() {
            let score = request
                .state
                .outcome
                .map_or(0, |outcome| terminal_score(outcome.winner, request.root, 0));
            return Ok(SearchResult {
                action: None,
                score,
                completed_depth: 0,
                principal_variation: Vec::new(),
                nodes: 0,
                quiescence_nodes: 0,
                cutoffs: 0,
                stop_reason: StopReason::NoLegalAction,
                tie_break: None,
            });
        }

        let mut context = SearchContext {
            scenario: request.scenario,
            root: request.root,
            evaluator: request.evaluator,
            orderer: request.orderer,
            limits: request.limits,
            cancellation: request.cancellation,
            nodes: 0,
            quiescence_nodes: 0,
            cutoffs: 0,
        };
        let mut completed: Option<NodeResult> = None;
        let mut completed_depth = 0;
        let mut stopped = None;
        for depth in 1..=request.limits.max_depth {
            let prior = completed
                .as_ref()
                .map_or(&[][..], |result| result.pv.as_slice());
            match context.root(request.state, depth, prior) {
                Ok(result) => {
                    completed = Some(result);
                    completed_depth = depth;
                }
                Err(ExploreError::Stopped(reason)) => {
                    stopped = Some(reason);
                    break;
                }
                Err(ExploreError::Fatal(error)) => return Err(error),
            }
        }
        let reason = stopped.unwrap_or(StopReason::DepthLimit);
        let Some(completed) = completed else {
            return Ok(SearchResult {
                action: None,
                score: 0,
                completed_depth: 0,
                principal_variation: Vec::new(),
                nodes: context.nodes,
                quiescence_nodes: context.quiescence_nodes,
                cutoffs: context.cutoffs,
                stop_reason: reason,
                tie_break: None,
            });
        };
        let action = completed.pv.first().cloned();
        Ok(SearchResult {
            tie_break: action.as_ref().and_then(ActionKey::from_action),
            action,
            score: completed.score,
            completed_depth,
            principal_variation: completed.pv,
            nodes: context.nodes,
            quiescence_nodes: context.quiescence_nodes,
            cutoffs: context.cutoffs,
            stop_reason: reason,
        })
    }
}

struct SearchContext<'a> {
    scenario: &'a ScenarioDefinition,
    root: crownline_core::scenario::Player,
    evaluator: &'a dyn Evaluator,
    orderer: &'a dyn MoveOrderer,
    limits: SearchLimits,
    cancellation: &'a dyn Cancellation,
    nodes: u64,
    quiescence_nodes: u64,
    cutoffs: u64,
}

impl SearchContext<'_> {
    fn root(
        &mut self,
        state: &MatchState,
        depth: u16,
        prior_pv: &[Action],
    ) -> Result<NodeResult, ExploreError> {
        self.visit_node()?;
        let mut actions = legal_search_actions(self.scenario, state).map_err(SearchError::Rules)?;
        self.orderer
            .order(self.scenario, state, &mut actions, prior_pv.first());
        self.expand(
            state,
            depth,
            0,
            actions,
            prior_pv,
            NEG_INFINITY,
            POS_INFINITY,
        )
    }

    fn alpha_beta(
        &mut self,
        state: &MatchState,
        depth: u16,
        ply: u16,
        alpha: i32,
        beta: i32,
        prior_pv: &[Action],
    ) -> Result<NodeResult, ExploreError> {
        self.visit_node()?;
        if let Some(outcome) = state.outcome {
            return Ok(NodeResult {
                score: terminal_score(outcome.winner, self.root, ply),
                pv: Vec::new(),
            });
        }
        if depth == 0 {
            if self.limits.max_quiescence_depth == 0 {
                return self.evaluate(state);
            }
            return self.quiescence(state, ply, self.limits.max_quiescence_depth, alpha, beta);
        }
        let mut actions = legal_search_actions(self.scenario, state).map_err(SearchError::Rules)?;
        if actions.is_empty() {
            return self.evaluate(state);
        }
        self.orderer
            .order(self.scenario, state, &mut actions, prior_pv.first());
        self.expand(state, depth, ply, actions, prior_pv, alpha, beta)
    }

    #[allow(clippy::too_many_arguments)]
    fn expand(
        &mut self,
        state: &MatchState,
        depth: u16,
        ply: u16,
        actions: Vec<Action>,
        prior_pv: &[Action],
        mut alpha: i32,
        mut beta: i32,
    ) -> Result<NodeResult, ExploreError> {
        let maximizing = maximizing_after(state, self.root);
        let mut best: Option<(ActionKey, NodeResult)> = None;
        for action in actions {
            self.check_stop()?;
            let transition = apply_action(self.scenario, state, &action)
                .map_err(SearchError::Rules)
                .map_err(ExploreError::Fatal)?;
            let child_prior = if prior_pv.first() == Some(&action) {
                &prior_pv[1..]
            } else {
                &[]
            };
            let child = self.alpha_beta(
                &transition.state,
                depth.saturating_sub(1),
                ply.saturating_add(1),
                alpha,
                beta,
                child_prior,
            )?;
            let key = ActionKey::from_action(&action)
                .expect("search action enumeration excludes control actions");
            let replace = best.as_ref().is_none_or(|(best_key, best_result)| {
                if maximizing {
                    child.score > best_result.score
                        || (child.score == best_result.score && key < *best_key)
                } else {
                    child.score < best_result.score
                        || (child.score == best_result.score && key < *best_key)
                }
            });
            if replace {
                let mut pv = Vec::with_capacity(child.pv.len() + 1);
                pv.push(action);
                pv.extend(child.pv);
                best = Some((
                    key,
                    NodeResult {
                        score: child.score,
                        pv,
                    },
                ));
            }
            if maximizing {
                alpha = alpha.max(child.score);
            } else {
                beta = beta.min(child.score);
            }
            if alpha >= beta {
                self.cutoffs = self.cutoffs.saturating_add(1);
                break;
            }
        }
        best.map(|(_, result)| result).ok_or_else(|| {
            ExploreError::Fatal(SearchError::Evaluation(EvaluationError::Failed(
                "search expansion produced no child".to_owned(),
            )))
        })
    }

    fn quiescence(
        &mut self,
        state: &MatchState,
        ply: u16,
        depth: u16,
        mut alpha: i32,
        mut beta: i32,
    ) -> Result<NodeResult, ExploreError> {
        self.visit_quiescence_node()?;
        if let Some(outcome) = state.outcome {
            return Ok(NodeResult {
                score: terminal_score(outcome.winner, self.root, ply),
                pv: Vec::new(),
            });
        }

        let mandatory = matches!(state.phase, TurnPhase::ResolvingChoices { .. });
        let checked = is_in_check(self.scenario, state, state.active_player)
            .map_err(SearchError::Rules)
            .map_err(ExploreError::Fatal)?;
        let forced = mandatory || checked;
        let maximizing = maximizing_after(state, self.root);
        let mut best: Option<(ActionKey, NodeResult)> = None;

        if !forced || depth == 0 {
            let stand_pat = self.evaluate(state)?;
            if depth == 0 {
                return Ok(stand_pat);
            }
            if maximizing {
                alpha = alpha.max(stand_pat.score);
            } else {
                beta = beta.min(stand_pat.score);
            }
            if alpha >= beta {
                self.cutoffs = self.cutoffs.saturating_add(1);
                return Ok(stand_pat);
            }
            best = Some((
                if maximizing {
                    ActionKey::Hold(self.root)
                } else {
                    ActionKey::Hold(self.root.opponent())
                },
                stand_pat,
            ));
        }

        let mut actions = legal_search_actions(self.scenario, state).map_err(SearchError::Rules)?;
        self.orderer.order(self.scenario, state, &mut actions, None);
        for action in actions {
            self.check_external_stop()?;
            let transition = apply_action(self.scenario, state, &action)
                .map_err(SearchError::Rules)
                .map_err(ExploreError::Fatal)?;
            if !forced && !is_noisy_transition(self.scenario, state, &action, &transition) {
                continue;
            }
            let child = self.quiescence(
                &transition.state,
                ply.saturating_add(1),
                depth.saturating_sub(1),
                alpha,
                beta,
            )?;
            let key = ActionKey::from_action(&action)
                .expect("search action enumeration excludes control actions");
            let replace = best.as_ref().is_none_or(|(best_key, best_result)| {
                if maximizing {
                    child.score > best_result.score
                        || (child.score == best_result.score && key < *best_key)
                } else {
                    child.score < best_result.score
                        || (child.score == best_result.score && key < *best_key)
                }
            });
            if replace {
                let mut pv = Vec::with_capacity(child.pv.len() + 1);
                pv.push(action);
                pv.extend(child.pv);
                best = Some((
                    key,
                    NodeResult {
                        score: child.score,
                        pv,
                    },
                ));
            }
            if maximizing {
                alpha = alpha.max(child.score);
            } else {
                beta = beta.min(child.score);
            }
            if alpha >= beta {
                self.cutoffs = self.cutoffs.saturating_add(1);
                break;
            }
        }

        best.map(|(_, result)| result)
            .map_or_else(|| self.evaluate(state), Ok)
    }

    fn evaluate(&self, state: &MatchState) -> Result<NodeResult, ExploreError> {
        let evaluation = self
            .evaluator
            .evaluate(self.scenario, state, self.root)
            .map_err(SearchError::Evaluation)
            .map_err(ExploreError::Fatal)?;
        if !(-MAX_HEURISTIC_SCORE..=MAX_HEURISTIC_SCORE).contains(&evaluation.score) {
            return Err(ExploreError::Fatal(SearchError::Evaluation(
                EvaluationError::OutOfRange(evaluation.score),
            )));
        }
        Ok(NodeResult {
            score: evaluation.score,
            pv: Vec::new(),
        })
    }

    fn visit_node(&mut self) -> Result<(), ExploreError> {
        self.check_external_stop()?;
        if self.nodes >= self.limits.max_nodes {
            return Err(ExploreError::Stopped(StopReason::NodeLimit));
        }
        self.nodes = self.nodes.saturating_add(1);
        Ok(())
    }

    fn visit_quiescence_node(&mut self) -> Result<(), ExploreError> {
        self.check_external_stop()?;
        if self.quiescence_nodes >= self.limits.max_quiescence_nodes {
            return Err(ExploreError::Stopped(StopReason::QuiescenceNodeLimit));
        }
        self.quiescence_nodes = self.quiescence_nodes.saturating_add(1);
        Ok(())
    }

    fn check_stop(&self) -> Result<(), ExploreError> {
        self.check_external_stop()?;
        if self.nodes >= self.limits.max_nodes {
            return Err(ExploreError::Stopped(StopReason::NodeLimit));
        }
        Ok(())
    }

    fn check_external_stop(&self) -> Result<(), ExploreError> {
        if self.cancellation.is_cancelled() {
            return Err(ExploreError::Stopped(StopReason::Cancelled));
        }
        if self
            .limits
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(ExploreError::Stopped(StopReason::Deadline));
        }
        Ok(())
    }
}

struct NodeResult {
    score: i32,
    pv: Vec<Action>,
}

#[derive(Debug)]
enum ExploreError {
    Stopped(StopReason),
    Fatal(SearchError),
}

impl From<SearchError> for ExploreError {
    fn from(error: SearchError) -> Self {
        Self::Fatal(error)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crownline_core::{
        MatchState, PromotionEligibility,
        scenario::{PieceKind, Player},
        state::{MandatoryChoice, MatchOutcome, OutcomeReason, RealmControlScore, TurnPhase},
    };

    use super::*;
    use crate::{CancellationToken, Evaluation, ScoreComponent};

    struct CoordinateEvaluator;

    impl Evaluator for CoordinateEvaluator {
        fn evaluate(
            &self,
            _scenario: &ScenarioDefinition,
            state: &MatchState,
            root: Player,
        ) -> Result<Evaluation, EvaluationError> {
            let score = state
                .pieces
                .values()
                .filter(|piece| piece.owner == root)
                .map(|piece| i32::from(piece.at.y))
                .sum();
            Ok(Evaluation {
                score,
                components: vec![ScoreComponent {
                    name: "coordinate",
                    value: score,
                }],
            })
        }
    }

    struct FailingEvaluator;

    impl Evaluator for FailingEvaluator {
        fn evaluate(
            &self,
            _scenario: &ScenarioDefinition,
            _state: &MatchState,
            _root: Player,
        ) -> Result<Evaluation, EvaluationError> {
            Err(EvaluationError::Failed("fixture".to_owned()))
        }
    }

    struct ReverseOrderer;

    impl MoveOrderer for ReverseOrderer {
        fn order(
            &self,
            _scenario: &ScenarioDefinition,
            _state: &MatchState,
            actions: &mut [Action],
            _prior_pv: Option<&Action>,
        ) {
            actions.sort_by_key(ActionKey::from_action);
            actions.reverse();
        }
    }

    struct MaterialEvaluator;

    impl Evaluator for MaterialEvaluator {
        fn evaluate(
            &self,
            _scenario: &ScenarioDefinition,
            state: &MatchState,
            root: Player,
        ) -> Result<Evaluation, EvaluationError> {
            let score = state
                .pieces
                .values()
                .map(|piece| {
                    let value = match piece.kind {
                        PieceKind::King => 0,
                        PieceKind::Queen => 900,
                        PieceKind::Rook => 500,
                        PieceKind::Bishop => 330,
                        PieceKind::Knight => 320,
                        PieceKind::Pawn => 100,
                    };
                    if piece.owner == root { value } else { -value }
                })
                .sum();
            Ok(Evaluation {
                score,
                components: vec![ScoreComponent {
                    name: "material",
                    value: score,
                }],
            })
        }
    }

    fn fixture() -> (ScenarioDefinition, MatchState) {
        let scenario =
            ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        (scenario, state)
    }

    fn capture_fixture() -> (ScenarioDefinition, MatchState, Action, Action) {
        let (scenario, mut state) = fixture();
        let south_king = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        let north_king = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::North && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        let rook = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::Rook)
            .unwrap()
            .id;
        let pawn = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::North && piece.kind == PieceKind::Pawn)
            .unwrap()
            .id;
        let north_rook = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::North && piece.kind == PieceKind::Rook)
            .unwrap()
            .id;
        state
            .pieces
            .retain(|id, _| [south_king, north_king, rook, pawn, north_rook].contains(id));
        state.pieces.get_mut(&south_king).unwrap().at = crate::Coord::new(15, 15);
        state.pieces.get_mut(&north_king).unwrap().at = crate::Coord::new(15, 0);
        state.pieces.get_mut(&rook).unwrap().at = crate::Coord::new(5, 5);
        state.pieces.get_mut(&pawn).unwrap().at = crate::Coord::new(5, 7);
        state.pieces.get_mut(&north_rook).unwrap().at = crate::Coord::new(5, 9);
        state.available_castling_routes.clear();
        let capture = Action::Move {
            player: Player::South,
            piece: rook,
            to: crate::Coord::new(5, 7),
        };
        let quiet = Action::Move {
            player: Player::South,
            piece: rook,
            to: crate::Coord::new(4, 5),
        };
        apply_action(&scenario, &state, &capture).unwrap();
        apply_action(&scenario, &state, &quiet).unwrap();
        (scenario, state, capture, quiet)
    }

    fn request<'a>(
        scenario: &'a ScenarioDefinition,
        state: &'a MatchState,
        evaluator: &'a dyn Evaluator,
        cancellation: &'a dyn Cancellation,
        limits: SearchLimits,
    ) -> SearchRequest<'a> {
        SearchRequest {
            scenario,
            state,
            root: state.active_player,
            evaluator,
            orderer: &StableMoveOrderer,
            limits,
            cancellation,
        }
    }

    #[test]
    fn iterative_search_is_deterministic_and_returns_a_complete_iteration() {
        let (scenario, state) = fixture();
        let before = state.canonical_hash().unwrap();
        let token = CancellationToken::default();
        let limits = SearchLimits {
            max_depth: 2,
            max_nodes: 20_000,
            ..SearchLimits::default()
        };
        let search = AlphaBetaSearch;
        let first = search
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &token,
                limits,
            ))
            .unwrap();
        let second = search
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &token,
                limits,
            ))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.completed_depth, 2);
        assert_eq!(first.action, first.principal_variation.first().cloned());
        assert!(first.cutoffs > 0);
        assert_eq!(state.canonical_hash().unwrap(), before);
    }

    #[test]
    fn tactical_ordering_prioritizes_pv_then_capture_before_quiet_actions() {
        let (scenario, state, capture, quiet) = capture_fixture();
        assert!(is_noisy_action(&scenario, &state, &capture).unwrap());
        assert!(!is_noisy_action(&scenario, &state, &quiet).unwrap());

        let orderer = StableMoveOrderer;
        let mut actions = vec![quiet.clone(), capture.clone()];
        orderer.order(&scenario, &state, &mut actions, None);
        assert_eq!(actions, vec![capture.clone(), quiet.clone()]);
        orderer.order(&scenario, &state, &mut actions, Some(&quiet));
        assert_eq!(actions, vec![quiet, capture]);
    }

    #[test]
    fn quiescence_avoids_a_hanging_capture_seen_as_good_at_plain_horizon() {
        let (scenario, state, capture, _) = capture_fixture();
        let token = CancellationToken::default();
        let plain = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &MaterialEvaluator,
                &token,
                SearchLimits {
                    max_depth: 1,
                    max_quiescence_depth: 0,
                    max_quiescence_nodes: 0,
                    ..SearchLimits::default()
                },
            ))
            .unwrap();
        let tactical = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &MaterialEvaluator,
                &token,
                SearchLimits {
                    max_depth: 1,
                    max_quiescence_depth: 2,
                    max_quiescence_nodes: 1_000,
                    ..SearchLimits::default()
                },
            ))
            .unwrap();
        assert_eq!(plain.action, Some(capture.clone()));
        assert_ne!(tactical.action, Some(capture));
        assert!(tactical.quiescence_nodes > 0);
    }

    #[test]
    fn checked_quiescence_searches_an_evasion_instead_of_standing_pat() {
        let (scenario, mut state, _, _) = capture_fixture();
        let north_rook = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::North && piece.kind == PieceKind::Rook)
            .unwrap()
            .id;
        state.pieces.get_mut(&north_rook).unwrap().at = crate::Coord::new(15, 9);
        assert!(is_in_check(&scenario, &state, Player::South).unwrap());
        let token = CancellationToken::default();
        let mut context = SearchContext {
            scenario: &scenario,
            root: Player::South,
            evaluator: &MaterialEvaluator,
            orderer: &StableMoveOrderer,
            limits: SearchLimits {
                max_quiescence_depth: 1,
                max_quiescence_nodes: 1_000,
                ..SearchLimits::default()
            },
            cancellation: &token,
            nodes: 0,
            quiescence_nodes: 0,
            cutoffs: 0,
        };
        let result = context
            .quiescence(&state, 0, 1, NEG_INFINITY, POS_INFINITY)
            .unwrap();
        assert!(!result.pv.is_empty());
        assert!(context.quiescence_nodes > 1);
    }

    #[test]
    fn ordering_changes_work_but_not_fixed_depth_minimax_result() {
        let (scenario, state) = fixture();
        let token = CancellationToken::default();
        let limits = SearchLimits {
            max_depth: 3,
            max_nodes: 500_000,
            max_quiescence_depth: 0,
            max_quiescence_nodes: 0,
            deadline: None,
        };
        let tactical = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &token,
                limits,
            ))
            .unwrap();
        let reverse = AlphaBetaSearch
            .search(SearchRequest {
                scenario: &scenario,
                state: &state,
                root: state.active_player,
                evaluator: &CoordinateEvaluator,
                orderer: &ReverseOrderer,
                limits,
                cancellation: &token,
            })
            .unwrap();
        assert_eq!(tactical.completed_depth, 3);
        assert_eq!(reverse.completed_depth, 3);
        assert_eq!(tactical.action, reverse.action);
        assert_eq!(tactical.score, reverse.score);
        assert_ne!(tactical.nodes, reverse.nodes);
    }

    #[test]
    fn node_limit_returns_only_the_last_completed_depth() {
        let (scenario, state) = fixture();
        let before = state.canonical_hash().unwrap();
        let token = CancellationToken::default();
        let result = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &token,
                SearchLimits {
                    max_depth: 5,
                    max_nodes: 100,
                    ..SearchLimits::default()
                },
            ))
            .unwrap();
        assert_eq!(result.stop_reason, StopReason::NodeLimit);
        assert_eq!(result.completed_depth, 1);
        assert_eq!(result.principal_variation.len(), 1);
        assert!(result.nodes <= 100);
        assert_eq!(state.canonical_hash().unwrap(), before);
    }

    #[test]
    fn cancellation_before_depth_one_returns_no_partial_root_choice() {
        let (scenario, state) = fixture();
        let before = state.canonical_hash().unwrap();
        let token = CancellationToken::default();
        token.cancel();
        let result = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &token,
                SearchLimits::default(),
            ))
            .unwrap();
        assert_eq!(result.completed_depth, 0);
        assert_eq!(result.action, None);
        assert_eq!(result.stop_reason, StopReason::Cancelled);
        assert_eq!(state.canonical_hash().unwrap(), before);
    }

    #[test]
    fn expired_deadline_returns_no_partial_root_choice() {
        let (scenario, state) = fixture();
        let token = CancellationToken::default();
        let result = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &token,
                SearchLimits {
                    deadline: Some(Instant::now()),
                    ..SearchLimits::default()
                },
            ))
            .unwrap();
        assert_eq!(result.completed_depth, 0);
        assert_eq!(result.action, None);
        assert_eq!(result.stop_reason, StopReason::Deadline);
    }

    #[test]
    fn quiescence_node_budget_is_independent_and_deterministic() {
        let (scenario, state) = fixture();
        let limits = SearchLimits {
            max_depth: 1,
            max_nodes: 10_000,
            max_quiescence_depth: 4,
            max_quiescence_nodes: 1,
            deadline: None,
        };
        let first = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &CancellationToken::default(),
                limits,
            ))
            .unwrap();
        let second = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &CoordinateEvaluator,
                &CancellationToken::default(),
                limits,
            ))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.stop_reason, StopReason::QuiescenceNodeLimit);
        assert_eq!(first.completed_depth, 0);
        assert_eq!(first.quiescence_nodes, 1);
        assert!(first.nodes > first.quiescence_nodes);
    }

    #[test]
    fn terminal_draw_uses_exact_score_without_calling_evaluator() {
        let (scenario, mut state) = fixture();
        state.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        let result = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &FailingEvaluator,
                &CancellationToken::default(),
                SearchLimits::default(),
            ))
            .unwrap();
        assert_eq!(result.score, crate::DRAW_SCORE);
        assert_eq!(result.stop_reason, StopReason::NoLegalAction);
    }

    #[test]
    fn consecutive_mandatory_choices_keep_the_same_maximizing_player() {
        let (scenario, mut state) = fixture();
        let pawns: Vec<_> = state
            .pieces
            .values()
            .filter(|piece| piece.owner == state.active_player && piece.kind == PieceKind::Pawn)
            .take(2)
            .map(|piece| piece.id)
            .collect();
        let eligibility = PromotionEligibility::from_control(
            RealmControlScore {
                owned_settlements: 8,
                governed_settlements: 0,
                established_settlements: 0,
            },
            scenario.rules.promotion_unlocks,
        );
        assert_eq!(
            eligibility.allowed_kinds,
            BTreeSet::from([
                crate::PromotionKind::Queen,
                crate::PromotionKind::Rook,
                crate::PromotionKind::Bishop,
                crate::PromotionKind::Knight,
            ])
        );
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![
                MandatoryChoice::Promote {
                    pawn: pawns[0],
                    site_index: 0,
                    eligibility: eligibility.clone(),
                },
                MandatoryChoice::Promote {
                    pawn: pawns[1],
                    site_index: 0,
                    eligibility,
                },
            ],
        };
        let result = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &MaterialEvaluator,
                &CancellationToken::default(),
                SearchLimits {
                    max_depth: 2,
                    max_nodes: 1_000,
                    ..SearchLimits::default()
                },
            ))
            .unwrap();
        assert_eq!(result.completed_depth, 2);
        assert_eq!(result.principal_variation.len(), 2);
        assert!(result.principal_variation.iter().all(|action| matches!(
            action,
            Action::ChoosePromotion {
                promote_to: crate::PromotionKind::Queen,
                ..
            }
        )));

        let horizon_result = AlphaBetaSearch
            .search(request(
                &scenario,
                &state,
                &MaterialEvaluator,
                &CancellationToken::default(),
                SearchLimits {
                    max_depth: 1,
                    max_nodes: 1_000,
                    ..SearchLimits::default()
                },
            ))
            .unwrap();
        assert!(horizon_result.principal_variation.len() >= 2);
        assert!(
            horizon_result.principal_variation[..2]
                .iter()
                .all(|action| {
                    matches!(
                        action,
                        Action::ChoosePromotion {
                            promote_to: crate::PromotionKind::Queen,
                            ..
                        }
                    )
                })
        );
        assert!(horizon_result.quiescence_nodes > 0);
    }

    #[test]
    fn evaluator_failure_and_search_leave_input_hash_unchanged() {
        let (scenario, state) = fixture();
        let before = state.canonical_hash().unwrap();
        let error = AlphaBetaSearch.search(request(
            &scenario,
            &state,
            &FailingEvaluator,
            &CancellationToken::default(),
            SearchLimits {
                max_depth: 1,
                ..SearchLimits::default()
            },
        ));
        assert!(matches!(error, Err(SearchError::Evaluation(_))));
        assert_eq!(state.canonical_hash().unwrap(), before);
    }
}
