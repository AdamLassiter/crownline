use crownline_core::{
    Action, GuidedPredicateContext, MatchState, ObjectiveResult, ScenarioDefinition, Transition,
    apply_action, is_in_check, legal_mandatory_choice_actions,
    scenario::{Coord, Player},
    state::{OutcomeReason, PieceId, PromotionKind},
};

const SOURCES: [&str; 8] = [
    include_str!("../../../assets/scenarios/guided/guided-royal-en-passant.ron"),
    include_str!("../../../assets/scenarios/guided/guided-royal-promotion-knight.ron"),
    include_str!("../../../assets/scenarios/guided/guided-royal-promotion-batch.ron"),
    include_str!("../../../assets/scenarios/guided/guided-royal-answer-check.ron"),
    include_str!("../../../assets/scenarios/guided/guided-royal-castling.ron"),
    include_str!("../../../assets/scenarios/guided/guided-royal-checkmate.ron"),
    include_str!("../../../assets/scenarios/guided/guided-royal-draw.ron"),
    include_str!("../../../assets/scenarios/guided/guided-royal-open-practice.ron"),
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

fn objective(scenario: &ScenarioDefinition, stage: usize, transition: &Transition) {
    assert_eq!(
        scenario.guided.as_ref().unwrap().stages[stage]
            .evaluate(&GuidedPredicateContext {
                scenario,
                state: &transition.state,
                events: &transition.events,
                actions_taken: 1,
                turns_elapsed: 0,
            })
            .unwrap(),
        ObjectiveResult::Succeeded
    );
}

#[test]
fn en_passant_uses_the_canonical_immediate_window() {
    let scenario = scenario("guided-royal-en-passant");
    let state = MatchState::from_scenario(&scenario).unwrap();
    assert!(state.en_passant.is_some());
    let captured = apply_action(
        &scenario,
        &state,
        &Action::Move {
            player: Player::South,
            piece: piece_at(&state, Coord::new(4, 3)),
            to: Coord::new(3, 2),
        },
    )
    .unwrap();
    objective(&scenario, 0, &captured);
    assert!(captured.state.en_passant.is_none());
}

#[test]
fn promotion_lessons_enforce_locks_and_preserve_a_frozen_batch() {
    let knight = scenario("guided-royal-promotion-knight");
    let state = MatchState::from_scenario(&knight).unwrap();
    let pawn = piece_at(&state, Coord::new(2, 2));
    let actions = legal_mandatory_choice_actions(&state);
    assert_eq!(
        actions,
        vec![Action::ChoosePromotion {
            player: Player::South,
            pawn,
            promote_to: PromotionKind::Knight,
        }]
    );
    let promoted = apply_action(&knight, &state, &actions[0]).unwrap();
    objective(&knight, 0, &promoted);

    let batch = scenario("guided-royal-promotion-batch");
    let state = MatchState::from_scenario(&batch).unwrap();
    let west = piece_at(&state, Coord::new(2, 2));
    let east = piece_at(&state, Coord::new(5, 2));
    let first = apply_action(
        &batch,
        &state,
        &Action::ChoosePromotion {
            player: Player::South,
            pawn: west,
            promote_to: PromotionKind::Bishop,
        },
    )
    .unwrap();
    objective(&batch, 0, &first);
    assert_eq!(legal_mandatory_choice_actions(&first.state).len(), 4);
    let second = apply_action(
        &batch,
        &first.state,
        &Action::ChoosePromotion {
            player: Player::South,
            pawn: east,
            promote_to: PromotionKind::Rook,
        },
    )
    .unwrap();
    objective(&batch, 1, &second);
}

#[test]
fn check_forbids_hold_but_allows_the_declared_block_and_castling_moves_both_pieces() {
    let check = scenario("guided-royal-answer-check");
    let state = MatchState::from_scenario(&check).unwrap();
    assert!(is_in_check(&check, &state, Player::South).unwrap());
    assert!(
        apply_action(
            &check,
            &state,
            &Action::Hold {
                player: Player::South
            }
        )
        .is_err()
    );
    let blocked = apply_action(
        &check,
        &state,
        &Action::Move {
            player: Player::South,
            piece: piece_at(&state, Coord::new(5, 6)),
            to: Coord::new(4, 5),
        },
    )
    .unwrap();
    objective(&check, 0, &blocked);

    let castling = scenario("guided-royal-castling");
    let state = MatchState::from_scenario(&castling).unwrap();
    let castled = apply_action(
        &castling,
        &state,
        &Action::Move {
            player: Player::South,
            piece: piece_at(&state, Coord::new(4, 7)),
            to: Coord::new(6, 7),
        },
    )
    .unwrap();
    objective(&castling, 0, &castled);
}

#[test]
fn terminal_lessons_finish_through_normal_outcome_rules() {
    let mate = scenario("guided-royal-checkmate");
    let state = MatchState::from_scenario(&mate).unwrap();
    let finished = apply_action(
        &mate,
        &state,
        &Action::Move {
            player: Player::South,
            piece: piece_at(&state, Coord::new(1, 2)),
            to: Coord::new(1, 1),
        },
    )
    .unwrap();
    objective(&mate, 0, &finished);
    assert_eq!(
        finished.state.outcome.unwrap().reason,
        OutcomeReason::Checkmate
    );

    let draw = scenario("guided-royal-draw");
    let state = MatchState::from_scenario(&draw).unwrap();
    let finished = apply_action(
        &draw,
        &state,
        &Action::RespondToDraw {
            player: Player::South,
            accept: true,
        },
    )
    .unwrap();
    objective(&draw, 0, &finished);
    assert_eq!(
        finished.state.outcome.unwrap().reason,
        OutcomeReason::AgreedDraw
    );
}

#[test]
fn combined_practice_is_small_bounded_and_uses_steward() {
    let scenario = scenario("guided-royal-open-practice");
    let guided = scenario.guided.as_ref().unwrap();
    assert_eq!(scenario.board.width, 8);
    assert_eq!(scenario.board.height, 8);
    assert_eq!(guided.stages[0].action_limit, Some(80));
    assert_eq!(guided.stages[0].turn_limit, Some(40));
    assert!(matches!(
        &guided.ai.as_ref().unwrap().mode,
        crownline_core::GuidedAiMode::GeneralProfile { profile_id }
            if profile_id == "steward"
    ));
    assert!(scenario.deployments.len() <= 12);
}
