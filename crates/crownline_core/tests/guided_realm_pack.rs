use crownline_core::{
    Action, GuidedPredicateContext, MatchState, ObjectiveResult, ScenarioDefinition, Transition,
    TransitionEvent, apply_action, governance_report,
    scenario::{Coord, Player},
    state::PieceId,
};

const SOURCES: [&str; 6] = [
    include_str!("../../../assets/scenarios/guided/guided-realm-claim.ron"),
    include_str!("../../../assets/scenarios/guided/guided-realm-governance.ron"),
    include_str!("../../../assets/scenarios/guided/guided-realm-production.ron"),
    include_str!("../../../assets/scenarios/guided/guided-realm-transfer.ron"),
    include_str!("../../../assets/scenarios/guided/guided-realm-transfer-cancel.ron"),
    include_str!("../../../assets/scenarios/guided/guided-realm-open-practice.ron"),
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

fn objective(
    scenario: &ScenarioDefinition,
    stage: usize,
    transition: &Transition,
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

fn move_from_to(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    from: Coord,
    to: Coord,
) -> Transition {
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

#[test]
#[allow(clippy::too_many_lines)]
fn realm_lessons_reach_claim_establish_produce_transfer_cancel_and_shared_governance() {
    let claim = scenario("guided-realm-claim");
    let claimed = move_from_to(
        &claim,
        &MatchState::from_scenario(&claim).unwrap(),
        Coord::new(3, 5),
        Coord::new(3, 4),
    );
    assert_eq!(
        objective(&claim, 0, &claimed, 1),
        ObjectiveResult::Succeeded
    );

    let governance = scenario("guided-realm-governance");
    let established = apply_action(
        &governance,
        &MatchState::from_scenario(&governance).unwrap(),
        &Action::Hold {
            player: Player::North,
        },
    )
    .unwrap();
    assert_eq!(
        objective(&governance, 0, &established, 1),
        ObjectiveResult::Succeeded
    );

    let production = scenario("guided-realm-production");
    let produced = apply_action(
        &production,
        &MatchState::from_scenario(&production).unwrap(),
        &Action::PlacePawn {
            player: Player::South,
            settlement_index: 0,
            at: Coord::new(2, 2),
        },
    )
    .unwrap();
    assert_eq!(
        objective(&production, 0, &produced, 1),
        ObjectiveResult::Succeeded
    );
    assert!(produced.events.iter().any(|event| matches!(
        event,
        TransitionEvent::PawnProduced {
            settlement_index: 0,
            ..
        }
    )));

    let transfer = scenario("guided-realm-transfer");
    let transferred = apply_action(
        &transfer,
        &MatchState::from_scenario(&transfer).unwrap(),
        &Action::Hold {
            player: Player::North,
        },
    )
    .unwrap();
    assert_eq!(
        objective(&transfer, 0, &transferred, 1),
        ObjectiveResult::Succeeded
    );

    let cancellation = scenario("guided-realm-transfer-cancel");
    let cancelled = move_from_to(
        &cancellation,
        &MatchState::from_scenario(&cancellation).unwrap(),
        Coord::new(3, 0),
        Coord::new(3, 3),
    );
    assert_eq!(
        objective(&cancellation, 0, &cancelled, 1),
        ObjectiveResult::Succeeded
    );

    let practice = scenario("guided-realm-open-practice");
    let state = MatchState::from_scenario(&practice).unwrap();
    assert!(
        !governance_report(&practice, &state, 0)
            .unwrap()
            .governors
            .is_empty()
    );
    assert!(
        !governance_report(&practice, &state, 1)
            .unwrap()
            .governors
            .is_empty()
    );
    let claimed = move_from_to(&practice, &state, Coord::new(5, 6), Coord::new(5, 5));
    assert_eq!(
        objective(&practice, 0, &claimed, 1),
        ObjectiveResult::Succeeded
    );
    let replied = apply_action(
        &practice,
        &claimed.state,
        &Action::Hold {
            player: Player::North,
        },
    )
    .unwrap();
    assert_eq!(
        objective(&practice, 1, &replied, 2),
        ObjectiveResult::Succeeded
    );
}

#[test]
fn breaking_a_governor_line_prevents_false_establishment_progress() {
    let scenario = scenario("guided-realm-governance");
    let mut state = MatchState::from_scenario(&scenario).unwrap();
    let rook = piece_at(&state, Coord::new(3, 7));
    state.pieces.get_mut(&rook).unwrap().at = Coord::new(4, 7);
    let interrupted = apply_action(
        &scenario,
        &state,
        &Action::Hold {
            player: Player::North,
        },
    )
    .unwrap();
    assert!(!interrupted.state.settlements[0].established);
    assert_eq!(interrupted.state.settlements[0].establishment_progress, 1);
    assert!(interrupted.events.iter().any(|event| matches!(
        event,
        TransitionEvent::SettlementContinuityInterrupted {
            settlement_index: 0
        }
    )));
}
