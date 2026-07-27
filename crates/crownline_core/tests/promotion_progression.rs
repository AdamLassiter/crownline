#[path = "support/promotion.rs"]
mod support;

use std::collections::BTreeSet;

use crownline_core::{
    Action, MatchState, PromotionEligibility, apply_action, governance_report,
    legal_mandatory_choice_actions, realm_control_score,
    scenario::{Coord, PieceKind, Player, ScenarioDefinition, TileTerrain},
    state::{MandatoryChoice, PieceId, PromotionKind, TurnPhase},
};

#[derive(Clone, Copy)]
enum ControlStage {
    Rush,
    GovernedClaim,
    OneEstablished,
    TwoEstablished,
}

impl ControlStage {
    const fn settlement_count(self) -> usize {
        match self {
            Self::Rush => 0,
            Self::GovernedClaim | Self::OneEstablished => 1,
            Self::TwoEstablished => 2,
        }
    }

    const fn established(self) -> bool {
        matches!(self, Self::OneEstablished | Self::TwoEstablished)
    }

    const fn expected_score(self) -> u32 {
        match self {
            Self::Rush => 0,
            Self::GovernedClaim => 2,
            Self::OneEstablished => 4,
            Self::TwoEstablished => 8,
        }
    }
}

#[allow(clippy::too_many_lines)]
fn progression_position(
    scenario: &ScenarioDefinition,
    control_stage: ControlStage,
) -> (MatchState, PieceId, Vec<PieceId>) {
    let mut state = MatchState::from_scenario(scenario).unwrap();
    let piece = |player, kind, skip: usize| {
        state
            .pieces
            .values()
            .filter(|piece| piece.owner == player && piece.kind == kind)
            .nth(skip)
            .cloned()
            .unwrap()
    };
    let north_king = piece(Player::North, PieceKind::King, 0);
    let south_king = piece(Player::South, PieceKind::King, 0);
    let candidate = piece(Player::South, PieceKind::Pawn, 0);
    let founders = [
        piece(Player::South, PieceKind::Pawn, 1),
        piece(Player::South, PieceKind::Pawn, 2),
    ];
    let governors = [
        piece(Player::South, PieceKind::Queen, 0),
        piece(Player::South, PieceKind::Rook, 0),
    ];
    state.pieces.clear();
    state.pieces.insert(north_king.id, north_king);
    state.pieces.insert(south_king.id, south_king);
    let mut candidate = candidate;
    candidate.at = scenario.promotion_sites[0].at;
    state.pieces.insert(candidate.id, candidate.clone());
    for settlement in &mut state.settlements {
        settlement.owner = None;
        settlement.founder = None;
        settlement.establishment_progress = 0;
        settlement.established = false;
        settlement.production_progress = 0;
        settlement.produced_pawn = None;
        settlement.cycle_interrupted = false;
        settlement.completed_cycle_continuous = true;
        settlement.transfer_candidate = None;
    }

    for (index, founder) in founders
        .iter()
        .take(control_stage.settlement_count())
        .enumerate()
    {
        let mut founder = founder.clone();
        founder.at = scenario.settlements[index].at;
        assert!(!state.pieces.values().any(|piece| piece.at == founder.at));
        state.pieces.insert(founder.id, founder.clone());
        let settlement = &mut state.settlements[index];
        settlement.owner = Some(Player::South);
        settlement.founder = Some(founder.id);
        settlement.established = control_stage.established();
        settlement.establishment_progress = if control_stage.established() {
            scenario.rules.establishment_cycles
        } else {
            0
        };
    }

    let mut governor_ids = Vec::new();
    for (index, mut governor) in governors
        .into_iter()
        .take(control_stage.settlement_count())
        .enumerate()
    {
        let mut placed = false;
        'coordinates: for y in 0..scenario.board.height {
            for x in 0..scenario.board.width {
                let at = Coord::new(x, y);
                if scenario.terrain.get(&at) == Some(&TileTerrain::Mountain)
                    || state.pieces.values().any(|piece| piece.at == at)
                {
                    continue;
                }
                governor.at = at;
                state.pieces.insert(governor.id, governor.clone());
                let all_governed = (0..=index).all(|settlement_index| {
                    governance_report(scenario, &state, u16::try_from(settlement_index).unwrap())
                        .is_ok_and(|report| !report.governors.is_empty())
                });
                if all_governed {
                    placed = true;
                    break 'coordinates;
                }
                state.pieces.remove(&governor.id);
            }
        }
        assert!(placed, "{} settlement {index}", scenario.id);
        governor_ids.push(governor.id);
    }

    let control = realm_control_score(scenario, &state, Player::South).unwrap();
    assert_eq!(
        control.total(),
        control_stage.expected_score(),
        "{}",
        scenario.id
    );
    state.active_player = Player::South;
    state.phase = TurnPhase::ResolvingChoices {
        queue: vec![MandatoryChoice::Promote {
            pawn: candidate.id,
            site_index: 0,
            eligibility: PromotionEligibility::from_control(
                control,
                scenario.rules.promotion_unlocks,
            ),
        }],
    };
    state.validate_invariants().unwrap();
    (state, candidate.id, governor_ids)
}

fn promotion_kinds(state: &MatchState) -> Vec<PromotionKind> {
    legal_mandatory_choice_actions(state)
        .into_iter()
        .filter_map(|action| match action {
            Action::ChoosePromotion { promote_to, .. } => Some(promote_to),
            _ => None,
        })
        .collect()
}

#[test]
fn deterministic_promotion_probes_match_archived_evidence() {
    let actual =
        serde_json::to_string_pretty(&support::all_promotion_progression_probes()).unwrap();
    let expected =
        include_str!("../../../docs/playtests/automated-promotion-progression.json").trim();
    assert_eq!(actual, expected);
}

#[test]
fn every_shipped_map_supports_each_exact_progression_tier() {
    let stages = [
        (ControlStage::Rush, vec![PromotionKind::Knight]),
        (
            ControlStage::GovernedClaim,
            vec![PromotionKind::Knight, PromotionKind::Bishop],
        ),
        (
            ControlStage::OneEstablished,
            vec![
                PromotionKind::Knight,
                PromotionKind::Bishop,
                PromotionKind::Rook,
            ],
        ),
        (
            ControlStage::TwoEstablished,
            vec![
                PromotionKind::Knight,
                PromotionKind::Bishop,
                PromotionKind::Rook,
                PromotionKind::Queen,
            ],
        ),
    ];
    for scenario in support::shipped_scenarios() {
        assert!(scenario.settlements.len() >= 2, "{}", scenario.id);
        assert!(!scenario.promotion_sites.is_empty(), "{}", scenario.id);
        for (stage, expected) in &stages {
            let (state, pawn, _) = progression_position(&scenario, *stage);
            assert_eq!(promotion_kinds(&state), *expected, "{}", scenario.id);
            for kind in expected {
                let transition = apply_action(
                    &scenario,
                    &state,
                    &Action::ChoosePromotion {
                        player: Player::South,
                        pawn,
                        promote_to: *kind,
                    },
                )
                .unwrap_or_else(|error| panic!("{} {kind:?}: {error}", scenario.id));
                assert_eq!(transition.state.revision, state.revision + 1);
            }
        }
    }
}

#[test]
fn governance_loss_and_transfer_relock_future_batches_on_every_map() {
    for scenario in support::shipped_scenarios() {
        let (established, _, governor_ids) =
            progression_position(&scenario, ControlStage::OneEstablished);
        assert_eq!(
            promotion_kinds(&established).last(),
            Some(&PromotionKind::Rook)
        );

        let mut governance_lost = established.clone();
        governance_lost.pieces.remove(&governor_ids[0]);
        let control = realm_control_score(&scenario, &governance_lost, Player::South).unwrap();
        assert_eq!(control.total(), 3, "{}", scenario.id);
        let TurnPhase::ResolvingChoices { queue } = &mut governance_lost.phase else {
            unreachable!();
        };
        let MandatoryChoice::Promote { eligibility, .. } = &mut queue[0] else {
            unreachable!();
        };
        *eligibility =
            PromotionEligibility::from_control(control, scenario.rules.promotion_unlocks);
        assert_eq!(
            promotion_kinds(&governance_lost),
            vec![PromotionKind::Knight, PromotionKind::Bishop]
        );

        let mut transferred = established;
        transferred.settlements[0].owner = Some(Player::North);
        let control = realm_control_score(&scenario, &transferred, Player::South).unwrap();
        assert_eq!(control.total(), 0, "{}", scenario.id);
        let TurnPhase::ResolvingChoices { queue } = &mut transferred.phase else {
            unreachable!();
        };
        let MandatoryChoice::Promote { eligibility, .. } = &mut queue[0] else {
            unreachable!();
        };
        *eligibility =
            PromotionEligibility::from_control(control, scenario.rules.promotion_unlocks);
        assert_eq!(promotion_kinds(&transferred), vec![PromotionKind::Knight]);
    }
}

#[test]
fn archived_probe_covers_exact_scores_without_claiming_human_results() {
    for probe in support::all_promotion_progression_probes() {
        assert_eq!(
            probe
                .tiers
                .iter()
                .map(|tier| tier.score)
                .collect::<Vec<_>>(),
            vec![0, 2, 4, 8]
        );
        assert!(!probe.rush_queen_legal);
        assert!(probe.queen_requires_two_full_settlements);
        assert!(probe.maximum_full_control_score >= 8);
        assert_eq!(
            probe.tiers[3]
                .allowed_kinds
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            PromotionKind::RECRUITMENT_ORDER.into_iter().collect()
        );
    }
}
