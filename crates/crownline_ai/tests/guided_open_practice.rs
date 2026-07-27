use std::time::Instant;

use crownline_ai::{
    AlphaBetaSearch, BaselineEvaluator, CancellationToken, DifficultyConfig, DifficultyProfile,
    SearchPolicy, SearchRequest, StableMoveOrderer,
};
use crownline_core::{
    Action, MatchState, ScenarioDefinition, apply_action,
    scenario::{Coord, Player},
};

#[test]
fn apprentice_open_practice_reply_is_legal_deterministic_and_bounded() {
    let scenario: ScenarioDefinition = ron::from_str(include_str!(
        "../../../assets/scenarios/guided/guided-movement-open-practice.ron"
    ))
    .unwrap();
    let state = MatchState::from_scenario(&scenario).unwrap();
    let rook = state
        .pieces
        .values()
        .find(|piece| piece.at == Coord::new(3, 5))
        .unwrap()
        .id;
    let crossed = apply_action(
        &scenario,
        &state,
        &Action::Move {
            player: Player::South,
            piece: rook,
            to: Coord::new(3, 3),
        },
    )
    .unwrap();
    let mut config = DifficultyConfig::for_profile(DifficultyProfile::Apprentice);
    config.move_time_millis = None;
    let evaluator = BaselineEvaluator::new(config.evaluation);
    let search = || {
        AlphaBetaSearch
            .search(SearchRequest {
                scenario: &scenario,
                state: &crossed.state,
                root: Player::North,
                evaluator: &evaluator,
                orderer: &StableMoveOrderer,
                limits: config.search_limits(Instant::now()),
                cancellation: &CancellationToken::default(),
            })
            .unwrap()
    };
    let first = search();
    let second = search();
    assert_eq!(first, second);
    assert!(first.nodes <= config.max_nodes);
    assert!(first.quiescence_nodes <= config.max_quiescence_nodes);
    let reply = first.action.unwrap();
    let replied = apply_action(&scenario, &crossed.state, &reply).unwrap();
    assert_eq!(replied.state.revision, crossed.state.revision + 1);
}

#[test]
fn steward_realm_practice_reply_is_legal_deterministic_and_bounded() {
    let scenario: ScenarioDefinition = ron::from_str(include_str!(
        "../../../assets/scenarios/guided/guided-realm-open-practice.ron"
    ))
    .unwrap();
    let state = MatchState::from_scenario(&scenario).unwrap();
    let pawn = state
        .pieces
        .values()
        .find(|piece| piece.at == Coord::new(5, 6))
        .unwrap()
        .id;
    let claimed = apply_action(
        &scenario,
        &state,
        &Action::Move {
            player: Player::South,
            piece: pawn,
            to: Coord::new(5, 5),
        },
    )
    .unwrap();
    let mut config = DifficultyConfig::for_profile(DifficultyProfile::Steward);
    config.move_time_millis = None;
    let evaluator = BaselineEvaluator::new(config.evaluation);
    let search = || {
        AlphaBetaSearch
            .search(SearchRequest {
                scenario: &scenario,
                state: &claimed.state,
                root: Player::North,
                evaluator: &evaluator,
                orderer: &StableMoveOrderer,
                limits: config.search_limits(Instant::now()),
                cancellation: &CancellationToken::default(),
            })
            .unwrap()
    };
    let first = search();
    let second = search();
    assert_eq!(first, second);
    assert!(first.nodes <= config.max_nodes);
    assert!(first.quiescence_nodes <= config.max_quiescence_nodes);
    let reply = first.action.unwrap();
    let replied = apply_action(&scenario, &claimed.state, &reply).unwrap();
    assert_eq!(replied.state.revision, claimed.state.revision + 1);
}

#[test]
fn steward_royal_practice_reply_is_legal_deterministic_and_bounded() {
    let scenario: ScenarioDefinition = ron::from_str(include_str!(
        "../../../assets/scenarios/guided/guided-royal-open-practice.ron"
    ))
    .unwrap();
    let state = MatchState::from_scenario(&scenario).unwrap();
    let pawn = state
        .pieces
        .values()
        .find(|piece| piece.at == Coord::new(2, 5))
        .unwrap()
        .id;
    let claimed = apply_action(
        &scenario,
        &state,
        &Action::Move {
            player: Player::South,
            piece: pawn,
            to: Coord::new(2, 4),
        },
    )
    .unwrap();
    let mut config = DifficultyConfig::for_profile(DifficultyProfile::Steward);
    config.move_time_millis = None;
    let evaluator = BaselineEvaluator::new(config.evaluation);
    let search = || {
        AlphaBetaSearch
            .search(SearchRequest {
                scenario: &scenario,
                state: &claimed.state,
                root: Player::North,
                evaluator: &evaluator,
                orderer: &StableMoveOrderer,
                limits: config.search_limits(Instant::now()),
                cancellation: &CancellationToken::default(),
            })
            .unwrap()
    };
    let first = search();
    let second = search();
    assert_eq!(first, second);
    assert!(first.nodes <= config.max_nodes);
    assert!(first.quiescence_nodes <= config.max_quiescence_nodes);
    let reply = first.action.unwrap();
    let replied = apply_action(&scenario, &claimed.state, &reply).unwrap();
    assert_eq!(replied.state.revision, claimed.state.revision + 1);
}

#[test]
fn warden_challenge_reply_is_legal_deterministic_and_bounded() {
    let scenario: ScenarioDefinition = ron::from_str(include_str!(
        "../../../assets/scenarios/guided/challenge-warden-realm.ron"
    ))
    .unwrap();
    let state = MatchState::from_scenario(&scenario).unwrap();
    let pawn = state
        .pieces
        .values()
        .find(|piece| piece.at == Coord::new(2, 5))
        .unwrap()
        .id;
    let claimed = apply_action(
        &scenario,
        &state,
        &Action::Move {
            player: Player::South,
            piece: pawn,
            to: Coord::new(2, 4),
        },
    )
    .unwrap();
    let mut config = DifficultyConfig::for_profile(DifficultyProfile::Warden);
    config.move_time_millis = None;
    let evaluator = BaselineEvaluator::new(config.evaluation);
    let search = || {
        AlphaBetaSearch
            .search(SearchRequest {
                scenario: &scenario,
                state: &claimed.state,
                root: Player::North,
                evaluator: &evaluator,
                orderer: &StableMoveOrderer,
                limits: config.search_limits(Instant::now()),
                cancellation: &CancellationToken::default(),
            })
            .unwrap()
    };
    let first = search();
    let second = search();
    println!("warden guided evidence: {first:?}");
    assert_eq!(first, second);
    assert!(first.nodes <= config.max_nodes);
    assert!(first.quiescence_nodes <= config.max_quiescence_nodes);
    assert!(first.completed_depth >= 1);
    let reply = first.action.unwrap();
    let replied = apply_action(&scenario, &claimed.state, &reply).unwrap();
    assert_eq!(replied.state.revision, claimed.state.revision + 1);
}
