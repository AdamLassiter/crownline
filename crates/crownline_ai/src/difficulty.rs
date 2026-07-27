use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{EvaluationWeights, SearchLimits};

pub const DIFFICULTY_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DifficultyProfile {
    Apprentice,
    Steward,
    Warden,
}

impl DifficultyProfile {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Apprentice => "apprentice",
            Self::Steward => "steward",
            Self::Warden => "warden",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "apprentice" => Some(Self::Apprentice),
            "steward" => Some(Self::Steward),
            "warden" => Some(Self::Warden),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredOpponentPolicy {
    Search(DifficultyConfig),
    FirstLegal,
}

pub fn registered_opponent_policy(id: &str) -> Option<RegisteredOpponentPolicy> {
    match id {
        "teaching_first_legal" => Some(RegisteredOpponentPolicy::FirstLegal),
        "teaching_material" => Some(RegisteredOpponentPolicy::Search(teaching_material())),
        "teaching_terrain" => Some(RegisteredOpponentPolicy::Search(teaching_terrain())),
        "teaching_realm" => Some(RegisteredOpponentPolicy::Search(teaching_realm())),
        _ => None,
    }
}

fn teaching_material() -> DifficultyConfig {
    let mut config = DifficultyConfig::for_profile(DifficultyProfile::Apprentice);
    let weights = &mut config.evaluation;
    weights.mobility = 0;
    weights.king_check = 0;
    weights.pawn_advancement = 0;
    weights.pawn_connection = 0;
    weights.centre_access = 0;
    weights.settlement_ownership = 0;
    config
}

fn teaching_terrain() -> DifficultyConfig {
    let mut config = teaching_material();
    config.evaluation.mobility = 2;
    config.evaluation.centre_access = 2;
    config.evaluation.terrain_activity = 5;
    config
}

fn teaching_realm() -> DifficultyConfig {
    let mut config = DifficultyConfig::for_profile(DifficultyProfile::Steward);
    let weights = &mut config.evaluation;
    weights.mobility = 0;
    weights.piece_safety = 0;
    weights.king_check = 0;
    weights.pawn_advancement = 0;
    weights.pawn_connection = 0;
    weights.promotion_distance = 0;
    weights.promotion_candidate = 0;
    weights.promotion_tier = 0;
    weights.centre_access = 0;
    weights.terrain_activity = 0;
    config
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifficultyConfig {
    pub schema_version: u16,
    pub profile: DifficultyProfile,
    pub max_depth: u16,
    pub max_nodes: u64,
    pub max_quiescence_depth: u16,
    pub max_quiescence_nodes: u64,
    pub move_time_millis: Option<u64>,
    pub evaluation: EvaluationWeights,
}

impl DifficultyConfig {
    #[must_use]
    pub fn for_profile(profile: DifficultyProfile) -> Self {
        let mut evaluation = EvaluationWeights::default();
        let (max_depth, max_nodes, max_quiescence_depth, max_quiescence_nodes, move_time_millis) =
            match profile {
                DifficultyProfile::Apprentice => {
                    evaluation.piece_safety = 0;
                    evaluation.promotion_distance = 0;
                    evaluation.promotion_candidate = 0;
                    evaluation.promotion_tier = 0;
                    evaluation.terrain_activity = 0;
                    evaluation.governor = 0;
                    evaluation.founder_safety = 0;
                    evaluation.settlement_continuity = 0;
                    evaluation.settlement_development = 0;
                    evaluation.settlement_production = 0;
                    evaluation.produced_pawn = 0;
                    evaluation.transfer_pressure = 0;
                    (1, 500, 1, 250, Some(250))
                }
                DifficultyProfile::Steward => {
                    evaluation.founder_safety = 0;
                    evaluation.transfer_pressure = 0;
                    (2, 8_000, 2, 4_000, Some(1_000))
                }
                DifficultyProfile::Warden => (4, 100_000, 4, 50_000, Some(3_000)),
            };
        Self {
            schema_version: DIFFICULTY_SCHEMA_VERSION,
            profile,
            max_depth,
            max_nodes,
            max_quiescence_depth,
            max_quiescence_nodes,
            move_time_millis,
            evaluation,
        }
    }

    #[must_use]
    pub fn search_limits(self, started_at: Instant) -> SearchLimits {
        SearchLimits {
            max_depth: self.max_depth,
            max_nodes: self.max_nodes,
            max_quiescence_depth: self.max_quiescence_depth,
            max_quiescence_nodes: self.max_quiescence_nodes,
            deadline: self
                .move_time_millis
                .map(|millis| started_at + Duration::from_millis(millis)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioDifficultyOverride {
    pub schema_version: u16,
    pub scenario_id: String,
    pub profile: DifficultyProfile,
    pub max_depth: Option<u16>,
    pub max_nodes: Option<u64>,
    pub max_quiescence_depth: Option<u16>,
    pub max_quiescence_nodes: Option<u64>,
    pub move_time_millis: Option<Option<u64>>,
}

impl ScenarioDifficultyOverride {
    /// Applies explicit effort overrides to the named scenario's base profile.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported schema or mismatched scenario id.
    pub fn apply(&self, scenario_id: &str) -> Result<DifficultyConfig, &'static str> {
        if self.schema_version != DIFFICULTY_SCHEMA_VERSION {
            return Err("unsupported difficulty override schema");
        }
        if self.scenario_id != scenario_id {
            return Err("difficulty override belongs to another scenario");
        }
        let mut config = DifficultyConfig::for_profile(self.profile);
        config.max_depth = self.max_depth.unwrap_or(config.max_depth);
        config.max_nodes = self.max_nodes.unwrap_or(config.max_nodes);
        config.max_quiescence_depth = self
            .max_quiescence_depth
            .unwrap_or(config.max_quiescence_depth);
        config.max_quiescence_nodes = self
            .max_quiescence_nodes
            .unwrap_or(config.max_quiescence_nodes);
        config.move_time_millis = self.move_time_millis.unwrap_or(config.move_time_millis);
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use crownline_core::{ActionJournal, MatchState, ScenarioDefinition, scenario::Player};

    use super::*;
    use crate::{
        AlphaBetaSearch, BaselineEvaluator, CancellationToken, SearchPolicy, SearchRequest,
        StableMoveOrderer,
    };

    #[test]
    fn profiles_are_monotonic_in_search_and_enabled_features() {
        let apprentice = DifficultyConfig::for_profile(DifficultyProfile::Apprentice);
        let steward = DifficultyConfig::for_profile(DifficultyProfile::Steward);
        let warden = DifficultyConfig::for_profile(DifficultyProfile::Warden);
        assert!(apprentice.max_depth < steward.max_depth && steward.max_depth < warden.max_depth);
        assert!(apprentice.max_nodes < steward.max_nodes && steward.max_nodes < warden.max_nodes);
        assert!(
            apprentice.max_quiescence_nodes < steward.max_quiescence_nodes
                && steward.max_quiescence_nodes < warden.max_quiescence_nodes
        );
        let apprentice_weights = ron::to_string(&apprentice.evaluation).unwrap();
        let steward_weights = ron::to_string(&steward.evaluation).unwrap();
        let warden_weights = ron::to_string(&warden.evaluation).unwrap();
        assert_ne!(apprentice_weights, steward_weights);
        assert_ne!(steward_weights, warden_weights);
        assert_eq!(warden.evaluation, EvaluationWeights::default());
    }

    #[test]
    fn scenario_override_changes_only_explicit_effort_fields() {
        let override_config = ScenarioDifficultyOverride {
            schema_version: DIFFICULTY_SCHEMA_VERSION,
            scenario_id: "challenge".to_owned(),
            profile: DifficultyProfile::Steward,
            max_depth: Some(3),
            max_nodes: None,
            max_quiescence_depth: None,
            max_quiescence_nodes: Some(5_000),
            move_time_millis: Some(None),
        };
        let baseline = DifficultyConfig::for_profile(DifficultyProfile::Steward);
        let changed = override_config.apply("challenge").unwrap();
        assert_eq!(changed.max_depth, 3);
        assert_eq!(changed.max_nodes, baseline.max_nodes);
        assert_eq!(changed.evaluation, baseline.evaluation);
        assert_eq!(changed.move_time_millis, None);
        assert!(override_config.apply("tutorial").is_err());
    }

    #[test]
    fn profile_round_trip_preserves_version_and_exact_limits() {
        for profile in [
            DifficultyProfile::Apprentice,
            DifficultyProfile::Steward,
            DifficultyProfile::Warden,
        ] {
            let config = DifficultyConfig::for_profile(profile);
            let encoded = ron::to_string(&config).unwrap();
            assert_eq!(ron::from_str::<DifficultyConfig>(&encoded).unwrap(), config);
        }
    }

    #[test]
    fn stable_ids_and_registered_teaching_policies_are_closed_and_bounded() {
        for profile in [
            DifficultyProfile::Apprentice,
            DifficultyProfile::Steward,
            DifficultyProfile::Warden,
        ] {
            assert_eq!(DifficultyProfile::from_id(profile.id()), Some(profile));
        }
        assert_eq!(DifficultyProfile::from_id("unknown"), None);
        assert_eq!(
            registered_opponent_policy("teaching_first_legal"),
            Some(RegisteredOpponentPolicy::FirstLegal)
        );
        for id in ["teaching_material", "teaching_terrain", "teaching_realm"] {
            let Some(RegisteredOpponentPolicy::Search(config)) = registered_opponent_policy(id)
            else {
                panic!("missing registered policy {id}");
            };
            assert!(config.max_depth > 0 && config.max_nodes > 0);
            assert!(config.move_time_millis.is_some());
        }
        let RegisteredOpponentPolicy::Search(material) =
            registered_opponent_policy("teaching_material").unwrap()
        else {
            unreachable!();
        };
        let RegisteredOpponentPolicy::Search(terrain) =
            registered_opponent_policy("teaching_terrain").unwrap()
        else {
            unreachable!();
        };
        let RegisteredOpponentPolicy::Search(realm) =
            registered_opponent_policy("teaching_realm").unwrap()
        else {
            unreachable!();
        };
        assert_eq!(material.evaluation.terrain_activity, 0);
        assert_eq!(material.evaluation.settlement_ownership, 0);
        assert!(terrain.evaluation.terrain_activity > 0);
        assert_eq!(terrain.evaluation.settlement_ownership, 0);
        assert_eq!(realm.evaluation.terrain_activity, 0);
        assert!(realm.evaluation.settlement_ownership > 0);
        assert_eq!(registered_opponent_policy("arbitrary_weights"), None);
    }

    #[test]
    fn node_limited_corpus_is_reproducible_across_maps_and_rule_phases() {
        let introductory: ScenarioDefinition =
            ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
        let standard: ScenarioDefinition =
            ron::from_str(include_str!("../../../assets/scenarios/standard.ron")).unwrap();
        let large: ScenarioDefinition =
            ron::from_str(include_str!("../../../assets/scenarios/large.ron")).unwrap();
        let combined: ScenarioDefinition = ron::from_str(include_str!(
            "../../crownline_core/tests/fixtures/scenarios/combined-realms.ron"
        ))
        .unwrap();
        let journal: ActionJournal = serde_json::from_str(include_str!(
            "../../crownline_core/tests/fixtures/replays/combined-realms.json"
        ))
        .unwrap();
        let mut choices = journal.clone();
        choices.records.truncate(4);
        let choice_state = choices.replay(&combined).unwrap();
        let mut pre_capture = journal;
        pre_capture.records.truncate(8);
        let capture_state = pre_capture.replay(&combined).unwrap();

        let opening_intro = MatchState::from_scenario(&introductory).unwrap();
        let opening_standard = MatchState::from_scenario(&standard).unwrap();
        let opening_large = MatchState::from_scenario(&large).unwrap();
        let cases = [
            (&introductory, &opening_intro, "16x16 terrain opening"),
            (&standard, &opening_standard, "20x20 crossing opening"),
            (&large, &opening_large, "24x24 chokepoint opening"),
            (&combined, &choice_state, "promotion and production choices"),
            (&combined, &capture_state, "capture and developed realm"),
        ];
        let mut config = DifficultyConfig::for_profile(DifficultyProfile::Apprentice);
        config.max_nodes = 2_000;
        config.max_quiescence_nodes = 500;
        config.move_time_millis = None;
        let evaluator = BaselineEvaluator::new(config.evaluation);
        let orderer = StableMoveOrderer;
        let search = AlphaBetaSearch;
        for (scenario, state, label) in cases {
            let token = CancellationToken::default();
            let run = || {
                search
                    .search(SearchRequest {
                        scenario,
                        state,
                        root: state.active_player,
                        evaluator: &evaluator,
                        orderer: &orderer,
                        limits: config.search_limits(Instant::now()),
                        cancellation: &token,
                    })
                    .unwrap()
            };
            let first = run();
            let second = run();
            assert_eq!(first, second, "non-reproducible corpus case: {label}");
            assert!(first.action.is_some(), "no completed decision for: {label}");
            assert!(matches!(state.active_player, Player::North | Player::South));
        }
    }
}
