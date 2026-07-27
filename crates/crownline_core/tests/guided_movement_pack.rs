use crownline_core::{
    Action, GuidedPredicateContext, MatchState, ObjectiveResult, ScenarioDefinition, apply_action,
    scenario::{Coord, Player},
    state::PieceId,
};

const SOURCES: [&str; 7] = [
    include_str!("../../../assets/scenarios/guided/guided-movement-capture.ron"),
    include_str!("../../../assets/scenarios/guided/guided-movement-knight.ron"),
    include_str!("../../../assets/scenarios/guided/guided-terrain-forest.ron"),
    include_str!("../../../assets/scenarios/guided/guided-terrain-mountain.ron"),
    include_str!("../../../assets/scenarios/guided/guided-crossing-bridge.ron"),
    include_str!("../../../assets/scenarios/guided/guided-crossing-tower-rook.ron"),
    include_str!("../../../assets/scenarios/guided/guided-movement-open-practice.ron"),
];

fn scenario(id: &str) -> ScenarioDefinition {
    SOURCES
        .iter()
        .map(|source| ron::from_str::<ScenarioDefinition>(source).unwrap())
        .find(|scenario| scenario.id == id)
        .unwrap()
}

fn piece_at(state: &MatchState, at: Coord) -> PieceId {
    state
        .pieces
        .values()
        .find(|piece| piece.at == at)
        .unwrap()
        .id
}

fn move_piece(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    from: Coord,
    to: Coord,
) -> crownline_core::Transition {
    apply_action(
        scenario,
        state,
        &Action::Move {
            player: state.active_player,
            piece: piece_at(state, from),
            to,
        },
    )
    .unwrap()
}

fn objective(
    scenario: &ScenarioDefinition,
    stage: usize,
    transition: &crownline_core::Transition,
    actions: u16,
) -> ObjectiveResult {
    scenario.guided.as_ref().unwrap().stages[stage]
        .evaluate(&GuidedPredicateContext {
            scenario,
            state: &transition.state,
            events: &transition.events,
            actions_taken: actions,
            turns_elapsed: actions / 2,
        })
        .unwrap()
}

#[test]
fn every_movement_and_terrain_stage_has_a_canonical_reachable_solution() {
    for (id, from, to) in [
        (
            "guided-movement-capture",
            Coord::new(1, 6),
            Coord::new(4, 6),
        ),
        ("guided-movement-knight", Coord::new(2, 6), Coord::new(3, 4)),
        ("guided-terrain-forest", Coord::new(1, 6), Coord::new(1, 3)),
        (
            "guided-terrain-mountain",
            Coord::new(1, 6),
            Coord::new(2, 4),
        ),
        ("guided-crossing-bridge", Coord::new(3, 5), Coord::new(3, 3)),
        (
            "guided-crossing-tower-rook",
            Coord::new(3, 5),
            Coord::new(3, 3),
        ),
    ] {
        let scenario = scenario(id);
        scenario.validate().unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let transition = move_piece(&scenario, &state, from, to);
        assert_eq!(
            objective(&scenario, 0, &transition, 1),
            ObjectiveResult::Succeeded,
            "{id}"
        );
    }

    let scenario = scenario("guided-movement-open-practice");
    let state = MatchState::from_scenario(&scenario).unwrap();
    let crossed = move_piece(&scenario, &state, Coord::new(3, 5), Coord::new(3, 3));
    assert_eq!(
        objective(&scenario, 0, &crossed, 1),
        ObjectiveResult::Succeeded
    );
    let held = apply_action(
        &scenario,
        &crossed.state,
        &Action::Hold {
            player: Player::North,
        },
    )
    .unwrap();
    let captured = move_piece(&scenario, &held.state, Coord::new(3, 3), Coord::new(3, 2));
    assert_eq!(
        objective(&scenario, 1, &captured, 2),
        ObjectiveResult::Succeeded
    );
}

#[test]
fn blockers_reject_illegal_attempts_without_consuming_a_revision() {
    let mountain = scenario("guided-terrain-mountain");
    let state = MatchState::from_scenario(&mountain).unwrap();
    let before = state.canonical_hash().unwrap();
    let illegal = Action::Move {
        player: Player::South,
        piece: piece_at(&state, Coord::new(1, 6)),
        to: Coord::new(1, 5),
    };
    assert!(apply_action(&mountain, &state, &illegal).is_err());
    assert_eq!(state.canonical_hash().unwrap(), before);

    let river = scenario("guided-crossing-bridge");
    let state = MatchState::from_scenario(&river).unwrap();
    let illegal = Action::Move {
        player: Player::South,
        piece: piece_at(&state, Coord::new(2, 5)),
        to: Coord::new(2, 3),
    };
    assert!(apply_action(&river, &state, &illegal).is_err());
}
