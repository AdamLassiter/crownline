use crownline_core::{
    MatchState, ScenarioDefinition, attack_lines_on, governance_report, is_in_check, legal_moves,
    realm_control_score,
    scenario::{Coord, PieceKind, Player, TileTerrain},
    state::{PromotionKind, TurnPhase},
};
use serde::{Deserialize, Serialize};

use crate::{
    Evaluation, EvaluationError, Evaluator, MAX_HEURISTIC_SCORE, ScoreComponent, terminal_score,
};

pub const EVALUATION_SCHEMA_VERSION: u16 = 1;

/// Versioned integer baseline. Zeroing a weight disables that component without
/// changing search, rules, saves, or the network protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationWeights {
    pub schema_version: u16,
    pub queen: i32,
    pub rook: i32,
    pub bishop: i32,
    pub knight: i32,
    pub pawn: i32,
    pub mobility: i32,
    pub piece_safety: i32,
    pub king_check: i32,
    pub pawn_advancement: i32,
    pub pawn_connection: i32,
    pub promotion_distance: i32,
    pub promotion_candidate: i32,
    pub promotion_tier: i32,
    pub centre_access: i32,
    pub terrain_activity: i32,
    pub settlement_ownership: i32,
    pub governor: i32,
    pub founder_safety: i32,
    pub settlement_continuity: i32,
    pub settlement_development: i32,
    pub settlement_production: i32,
    pub produced_pawn: i32,
    pub transfer_pressure: i32,
}

impl Default for EvaluationWeights {
    fn default() -> Self {
        Self {
            schema_version: EVALUATION_SCHEMA_VERSION,
            queen: 900,
            rook: 500,
            bishop: 330,
            knight: 320,
            pawn: 100,
            mobility: 2,
            piece_safety: 8,
            king_check: 80,
            pawn_advancement: 3,
            pawn_connection: 6,
            promotion_distance: 4,
            promotion_candidate: 20,
            promotion_tier: 25,
            centre_access: 2,
            terrain_activity: 5,
            settlement_ownership: 90,
            governor: 20,
            founder_safety: 15,
            settlement_continuity: 12,
            settlement_development: 12,
            settlement_production: 10,
            produced_pawn: 35,
            transfer_pressure: 25,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BaselineEvaluator {
    pub weights: EvaluationWeights,
}

impl BaselineEvaluator {
    #[must_use]
    pub fn new(weights: EvaluationWeights) -> Self {
        Self { weights }
    }
}

impl Evaluator for BaselineEvaluator {
    fn evaluate(
        &self,
        scenario: &ScenarioDefinition,
        state: &MatchState,
        root: Player,
    ) -> Result<Evaluation, EvaluationError> {
        if self.weights.schema_version != EVALUATION_SCHEMA_VERSION {
            return Err(EvaluationError::Failed(format!(
                "unsupported evaluation schema {}; expected {EVALUATION_SCHEMA_VERSION}",
                self.weights.schema_version
            )));
        }
        if let Some(outcome) = state.outcome {
            let score = terminal_score(outcome.winner, root, 0);
            return Ok(Evaluation {
                score,
                components: vec![ScoreComponent {
                    name: "terminal",
                    value: score,
                }],
            });
        }

        let raw = features(scenario, state, root, &self.weights)?;
        let weights = self.weights;
        let mut components = vec![
            component("material", bounded(raw.material)),
            component("mobility", weighted(raw.mobility, weights.mobility)),
            component(
                "piece_safety",
                weighted(raw.piece_safety, weights.piece_safety),
            ),
            component("king_safety", weighted(raw.king_check, weights.king_check)),
            component(
                "pawn_advancement",
                weighted(raw.pawn_advancement, weights.pawn_advancement),
            ),
            component(
                "pawn_structure",
                weighted(raw.pawn_connection, weights.pawn_connection),
            ),
            component(
                "promotion_distance",
                weighted(raw.promotion_distance, weights.promotion_distance),
            ),
            component(
                "promotion_candidate",
                weighted(raw.promotion_candidate, weights.promotion_candidate),
            ),
            component(
                "promotion_tier",
                weighted(raw.promotion_tier, weights.promotion_tier),
            ),
            component(
                "centre_access",
                weighted(raw.centre_access, weights.centre_access),
            ),
            component(
                "terrain_activity",
                weighted(raw.terrain_activity, weights.terrain_activity),
            ),
            component(
                "settlement_ownership",
                weighted(raw.settlement_ownership, weights.settlement_ownership),
            ),
            component("governance", weighted(raw.governor, weights.governor)),
            component(
                "founder_safety",
                weighted(raw.founder_safety, weights.founder_safety),
            ),
            component(
                "settlement_continuity",
                weighted(raw.settlement_continuity, weights.settlement_continuity),
            ),
            component(
                "settlement_development",
                weighted(raw.settlement_development, weights.settlement_development),
            ),
            component(
                "settlement_production",
                weighted(raw.settlement_production, weights.settlement_production),
            ),
            component(
                "produced_pawn",
                weighted(raw.produced_pawn, weights.produced_pawn),
            ),
            component(
                "transfer_pressure",
                weighted(raw.transfer_pressure, weights.transfer_pressure),
            ),
        ];
        let total = components
            .iter()
            .fold(0_i64, |sum, item| sum.saturating_add(i64::from(item.value)));
        let score = i32::try_from(total.clamp(
            i64::from(-MAX_HEURISTIC_SCORE),
            i64::from(MAX_HEURISTIC_SCORE),
        ))
        .expect("heuristic total is clamped to the i32 score range");
        if total != i64::from(score) {
            let unbounded = i32::try_from(total).expect("bounded components sum within i32");
            components.push(component("score_bound", score - unbounded));
        }
        Ok(Evaluation { score, components })
    }
}

const fn component(name: &'static str, value: i32) -> ScoreComponent {
    ScoreComponent { name, value }
}

fn weighted(raw: i32, weight: i32) -> i32 {
    i32::try_from(i64::from(raw).saturating_mul(i64::from(weight)).clamp(
        i64::from(-MAX_HEURISTIC_SCORE),
        i64::from(MAX_HEURISTIC_SCORE),
    ))
    .expect("weighted component is clamped to the heuristic range")
}

fn bounded(value: i32) -> i32 {
    value.clamp(-MAX_HEURISTIC_SCORE, MAX_HEURISTIC_SCORE)
}

fn count_i32(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

#[derive(Default)]
struct RawFeatures {
    material: i32,
    mobility: i32,
    piece_safety: i32,
    king_check: i32,
    pawn_advancement: i32,
    pawn_connection: i32,
    promotion_distance: i32,
    promotion_candidate: i32,
    promotion_tier: i32,
    centre_access: i32,
    terrain_activity: i32,
    settlement_ownership: i32,
    governor: i32,
    founder_safety: i32,
    settlement_continuity: i32,
    settlement_development: i32,
    settlement_production: i32,
    produced_pawn: i32,
    transfer_pressure: i32,
}

fn features(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    root: Player,
    weights: &EvaluationWeights,
) -> Result<RawFeatures, EvaluationError> {
    let mut raw = RawFeatures::default();
    let opponent = root.opponent();
    for piece in state.pieces.values() {
        let sign = side(piece.owner, root);
        let piece_value = match piece.kind {
            PieceKind::King => 0,
            PieceKind::Queen => weights.queen,
            PieceKind::Rook => weights.rook,
            PieceKind::Bishop => weights.bishop,
            PieceKind::Knight => weights.knight,
            PieceKind::Pawn => weights.pawn,
        };
        raw.material = raw.material.saturating_add(weighted(sign, piece_value));
        let defenders = count_i32(
            attack_lines_on(scenario, state, piece.at, piece.owner)
                .map_err(rule_error)?
                .len(),
        );
        let attackers = count_i32(
            attack_lines_on(scenario, state, piece.at, piece.owner.opponent())
                .map_err(rule_error)?
                .len(),
        );
        raw.piece_safety += sign * (defenders - attackers);
        raw.centre_access += sign * centre_value(scenario, piece.at);
        raw.terrain_activity += sign
            * match scenario
                .terrain
                .get(&piece.at)
                .copied()
                .unwrap_or(TileTerrain::Open)
            {
                TileTerrain::Road => 2,
                TileTerrain::Forest => -1,
                TileTerrain::Open | TileTerrain::Mountain => 0,
            };
        if piece.kind == PieceKind::Pawn {
            raw.pawn_advancement += sign * pawn_advance(scenario, piece.owner, piece.at);
            raw.promotion_distance += sign * promotion_proximity(scenario, piece.at);
            raw.promotion_candidate += sign
                * i32::from(
                    state
                        .promotion_candidates
                        .get(&piece.id)
                        .copied()
                        .unwrap_or(0),
                );
        }
    }
    raw.pawn_connection = connected_pawns(state, root) - connected_pawns(state, opponent);
    raw.mobility = mobility(scenario, state, root)? - mobility(scenario, state, opponent)?;
    raw.king_check = i32::from(is_in_check(scenario, state, opponent).map_err(rule_error)?)
        - i32::from(is_in_check(scenario, state, root).map_err(rule_error)?);
    raw.promotion_tier =
        promotion_tier(scenario, state, root)? - promotion_tier(scenario, state, opponent)?;
    realm_features(scenario, state, root, 1, &mut raw)?;
    realm_features(scenario, state, opponent, -1, &mut raw)?;
    Ok(raw)
}

fn side(player: Player, root: Player) -> i32 {
    if player == root { 1 } else { -1 }
}

fn rule_error(error: impl std::fmt::Display) -> EvaluationError {
    EvaluationError::Failed(error.to_string())
}

fn mobility(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
) -> Result<i32, EvaluationError> {
    if !matches!(state.phase, TurnPhase::Command) {
        return Ok(0);
    }
    let mut view = state.clone();
    view.active_player = player;
    Ok(count_i32(
        legal_moves(scenario, &view).map_err(rule_error)?.len(),
    ))
}

fn centre_value(scenario: &ScenarioDefinition, at: Coord) -> i32 {
    let doubled_x = i32::from(at.x) * 2;
    let doubled_y = i32::from(at.y) * 2;
    let centre_x = i32::from(scenario.board.width.saturating_sub(1));
    let centre_y = i32::from(scenario.board.height.saturating_sub(1));
    i32::from(scenario.board.width + scenario.board.height)
        - (doubled_x - centre_x).abs()
        - (doubled_y - centre_y).abs()
}

fn pawn_advance(scenario: &ScenarioDefinition, player: Player, at: Coord) -> i32 {
    if scenario
        .rules
        .pawn_forward_y
        .get(&player)
        .copied()
        .unwrap_or(1)
        > 0
    {
        i32::from(at.y)
    } else {
        i32::from(scenario.board.height.saturating_sub(1).saturating_sub(at.y))
    }
}

fn promotion_proximity(scenario: &ScenarioDefinition, at: Coord) -> i32 {
    let max = i32::from(scenario.board.width + scenario.board.height);
    scenario
        .promotion_sites
        .iter()
        .map(|site| i32::from(at.x.abs_diff(site.at.x) + at.y.abs_diff(site.at.y)))
        .min()
        .map_or(0, |distance| max - distance)
}

fn connected_pawns(state: &MatchState, player: Player) -> i32 {
    let pawns: Vec<_> = state
        .pieces
        .values()
        .filter(|piece| piece.owner == player && piece.kind == PieceKind::Pawn)
        .collect();
    count_i32(
        pawns
            .iter()
            .enumerate()
            .flat_map(|(index, first)| {
                pawns[index + 1..]
                    .iter()
                    .map(move |second| (*first, *second))
            })
            .filter(|(first, second)| {
                first.at.x.abs_diff(second.at.x) <= 1 && first.at.y.abs_diff(second.at.y) <= 1
            })
            .count(),
    )
}

fn promotion_tier(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
) -> Result<i32, EvaluationError> {
    let score = realm_control_score(scenario, state, player)
        .map_err(rule_error)?
        .total();
    let rules = scenario.rules.promotion_unlocks;
    Ok(if score >= rules.queen {
        4
    } else if score >= rules.rook {
        3
    } else if score >= rules.bishop {
        2
    } else {
        1
    })
}

fn realm_features(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    player: Player,
    sign: i32,
    raw: &mut RawFeatures,
) -> Result<(), EvaluationError> {
    for settlement in state
        .settlements
        .iter()
        .filter(|site| site.owner == Some(player))
    {
        raw.settlement_ownership += sign;
        raw.settlement_continuity += sign * i32::from(!settlement.cycle_interrupted);
        raw.settlement_development += sign * i32::from(settlement.establishment_progress);
        raw.settlement_production += sign * i32::from(settlement.production_progress);
        raw.produced_pawn += sign * i32::from(settlement.produced_pawn.is_some());
        raw.transfer_pressure += sign * i32::from(settlement.transfer_candidate.is_some());
        let report =
            governance_report(scenario, state, settlement.site_index).map_err(rule_error)?;
        raw.governor += sign * count_i32(report.governors.len());
        if let Some(founder) = settlement.founder.and_then(|id| state.pieces.get(&id)) {
            let attacked = !attack_lines_on(scenario, state, founder.at, player.opponent())
                .map_err(rule_error)?
                .is_empty();
            raw.founder_safety += sign * i32::from(!attacked);
        }
    }
    if player == state.active_player
        && let TurnPhase::ResolvingChoices { queue } = &state.phase
    {
        raw.promotion_tier += queue
            .iter()
            .filter_map(|choice| match choice {
                crownline_core::state::MandatoryChoice::Promote { eligibility, .. } => Some(
                    i32::from(eligibility.allows(PromotionKind::Bishop))
                        + i32::from(eligibility.allows(PromotionKind::Rook))
                        + i32::from(eligibility.allows(PromotionKind::Queen)),
                ),
                crownline_core::state::MandatoryChoice::PlacePawn { .. } => None,
            })
            .sum::<i32>()
            * sign;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crownline_core::{
        scenario::{PieceKind, Player},
        state::{MatchOutcome, OutcomeReason},
    };

    use super::*;
    use crate::{DRAW_SCORE, MATE_SCORE};

    fn fixture() -> (ScenarioDefinition, MatchState) {
        let scenario =
            ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        (scenario, state)
    }

    fn value(result: &Evaluation, name: &str) -> i32 {
        result
            .components
            .iter()
            .find(|component| component.name == name)
            .unwrap()
            .value
    }

    #[test]
    fn symmetric_opening_is_zero_from_both_perspectives() {
        let (scenario, state) = fixture();
        let evaluator = BaselineEvaluator::default();
        let south = evaluator
            .evaluate(&scenario, &state, Player::South)
            .unwrap();
        let north = evaluator
            .evaluate(&scenario, &state, Player::North)
            .unwrap();
        assert_eq!(south.score, 0);
        assert_eq!(north.score, 0);
        assert_eq!(south.score, -north.score);
        assert_eq!(
            south
                .components
                .iter()
                .map(|component| component.value)
                .sum::<i32>(),
            south.score
        );
    }

    #[test]
    fn removing_one_queen_changes_exactly_the_material_component_value() {
        let (scenario, mut state) = fixture();
        let queen = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::Queen)
            .unwrap()
            .id;
        state.pieces.remove(&queen);
        let south = BaselineEvaluator::default()
            .evaluate(&scenario, &state, Player::South)
            .unwrap();
        let north = BaselineEvaluator::default()
            .evaluate(&scenario, &state, Player::North)
            .unwrap();
        assert_eq!(
            value(&south, "material"),
            -EvaluationWeights::default().queen
        );
        assert_eq!(south.score, -north.score);
    }

    #[test]
    fn terminal_result_overrides_every_heuristic_component() {
        let (scenario, mut state) = fixture();
        state.outcome = Some(MatchOutcome {
            winner: Some(Player::North),
            reason: OutcomeReason::Checkmate,
        });
        let loss = BaselineEvaluator::default()
            .evaluate(&scenario, &state, Player::South)
            .unwrap();
        assert_eq!(loss.score, -MATE_SCORE);
        assert_eq!(loss.components, vec![component("terminal", -MATE_SCORE)]);
        state.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        assert_eq!(
            BaselineEvaluator::default()
                .evaluate(&scenario, &state, Player::South)
                .unwrap()
                .score,
            DRAW_SCORE
        );
    }

    #[test]
    fn weights_are_versioned_and_every_component_is_bounded() {
        let (scenario, mut state) = fixture();
        let invalid = EvaluationWeights {
            schema_version: EVALUATION_SCHEMA_VERSION + 1,
            ..EvaluationWeights::default()
        };
        assert!(matches!(
            BaselineEvaluator::new(invalid).evaluate(&scenario, &state, Player::South),
            Err(EvaluationError::Failed(_))
        ));
        let extreme = EvaluationWeights {
            queen: i32::MAX,
            ..EvaluationWeights::default()
        };
        let north_queen = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::North && piece.kind == PieceKind::Queen)
            .unwrap()
            .id;
        state.pieces.remove(&north_queen);
        let result = BaselineEvaluator::new(extreme)
            .evaluate(&scenario, &state, Player::South)
            .unwrap();
        assert!((-MAX_HEURISTIC_SCORE..=MAX_HEURISTIC_SCORE).contains(&result.score));
    }
}
