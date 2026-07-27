use crownline_core::{
    PromotionEligibility, RealmControlScore, scenario::ScenarioDefinition, state::PromotionKind,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionTierProbe {
    pub strategy: String,
    pub control: RealmControlScore,
    pub score: u32,
    pub allowed_kinds: Vec<PromotionKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionProgressionProbe {
    pub scenario_id: String,
    pub scenario_hash: String,
    pub board: (u16, u16),
    pub settlement_count: usize,
    pub promotion_site_count: usize,
    pub bishop_threshold: u32,
    pub rook_threshold: u32,
    pub queen_threshold: u32,
    pub maximum_full_control_score: u32,
    pub rush_queen_legal: bool,
    pub queen_requires_two_full_settlements: bool,
    pub tiers: Vec<PromotionTierProbe>,
}

pub fn shipped_scenarios() -> Vec<ScenarioDefinition> {
    [
        include_str!("../../../../assets/scenarios/introductory.ron"),
        include_str!("../../../../assets/scenarios/standard.ron"),
        include_str!("../../../../assets/scenarios/large.ron"),
    ]
    .into_iter()
    .map(|source| ron::from_str(source).expect("shipped scenario must decode"))
    .collect()
}

pub fn all_promotion_progression_probes() -> Vec<PromotionProgressionProbe> {
    shipped_scenarios()
        .iter()
        .map(promotion_progression_probe)
        .collect()
}

fn promotion_progression_probe(scenario: &ScenarioDefinition) -> PromotionProgressionProbe {
    let stages = [
        (
            "promotion rush without settlement control",
            RealmControlScore::default(),
        ),
        (
            "claim and govern one settlement",
            RealmControlScore {
                owned_settlements: 1,
                governed_settlements: 1,
                established_settlements: 0,
            },
        ),
        (
            "establish one governed settlement",
            RealmControlScore {
                owned_settlements: 1,
                governed_settlements: 1,
                established_settlements: 1,
            },
        ),
        (
            "establish and govern two settlements",
            RealmControlScore {
                owned_settlements: 2,
                governed_settlements: 2,
                established_settlements: 2,
            },
        ),
    ];
    let tiers = stages
        .into_iter()
        .map(|(strategy, control)| {
            let eligibility =
                PromotionEligibility::from_control(control, scenario.rules.promotion_unlocks);
            PromotionTierProbe {
                strategy: strategy.to_owned(),
                control,
                score: control.total(),
                allowed_kinds: PromotionKind::RECRUITMENT_ORDER
                    .into_iter()
                    .filter(|kind| eligibility.allows(*kind))
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let maximum_full_control_score = u32::try_from(scenario.settlements.len())
        .expect("validated settlement count fits u32")
        .saturating_mul(4);
    PromotionProgressionProbe {
        scenario_id: scenario.id.clone(),
        scenario_hash: scenario.canonical_hash().expect("validated scenario hash"),
        board: (scenario.board.width, scenario.board.height),
        settlement_count: scenario.settlements.len(),
        promotion_site_count: scenario.promotion_sites.len(),
        bishop_threshold: scenario.rules.promotion_unlocks.bishop,
        rook_threshold: scenario.rules.promotion_unlocks.rook,
        queen_threshold: scenario.rules.promotion_unlocks.queen,
        maximum_full_control_score,
        rush_queen_legal: tiers[0].allowed_kinds.contains(&PromotionKind::Queen),
        queen_requires_two_full_settlements: scenario.rules.promotion_unlocks.queen == 8,
        tiers,
    }
}
