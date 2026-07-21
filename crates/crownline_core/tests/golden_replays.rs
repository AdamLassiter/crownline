use crownline_core::{
    Action, ActionJournal, MatchState, TransitionEvent, apply_timed_action, is_in_check,
    scenario::{PieceKind, Player, ScenarioDefinition},
    state::{MatchOutcome, OutcomeReason},
};

struct GoldenCase {
    name: &'static str,
    journal: &'static [u8],
    scenario: &'static str,
    outcome: MatchOutcome,
}

fn cases() -> [GoldenCase; 6] {
    [
        GoldenCase {
            name: "introductory resignation",
            journal: include_bytes!("fixtures/replays/introductory-resignation.json"),
            scenario: include_str!("../../../assets/scenarios/introductory.ron"),
            outcome: MatchOutcome {
                winner: Some(Player::North),
                reason: OutcomeReason::Resignation,
            },
        },
        GoldenCase {
            name: "standard agreed draw",
            journal: include_bytes!("fixtures/replays/standard-agreed-draw.json"),
            scenario: include_str!("../../../assets/scenarios/standard.ron"),
            outcome: MatchOutcome {
                winner: None,
                reason: OutcomeReason::AgreedDraw,
            },
        },
        GoldenCase {
            name: "large repetition",
            journal: include_bytes!("fixtures/replays/large-repetition.json"),
            scenario: include_str!("../../../assets/scenarios/large.ron"),
            outcome: MatchOutcome {
                winner: None,
                reason: OutcomeReason::ThreefoldRepetition,
            },
        },
        GoldenCase {
            name: "checkmate",
            journal: include_bytes!("fixtures/replays/checkmate.json"),
            scenario: include_str!("fixtures/scenarios/checkmate.ron"),
            outcome: MatchOutcome {
                winner: Some(Player::South),
                reason: OutcomeReason::Checkmate,
            },
        },
        GoldenCase {
            name: "timeout",
            journal: include_bytes!("fixtures/replays/timeout.json"),
            scenario: include_str!("fixtures/scenarios/timeout.ron"),
            outcome: MatchOutcome {
                winner: Some(Player::North),
                reason: OutcomeReason::Timeout,
            },
        },
        GoldenCase {
            name: "combined realms",
            journal: include_bytes!("fixtures/replays/combined-realms.json"),
            scenario: include_str!("fixtures/scenarios/combined-realms.ron"),
            outcome: MatchOutcome {
                winner: Some(Player::South),
                reason: OutcomeReason::Resignation,
            },
        },
    ]
}

fn replay_every_revision(
    case: &GoldenCase,
) -> (ActionJournal, ScenarioDefinition, MatchState, bool, bool) {
    let scenario: ScenarioDefinition = ron::from_str(case.scenario).expect(case.name);
    let journal = ActionJournal::from_json(case.journal).expect(case.name);
    assert_eq!(journal.scenario_id, scenario.id, "{} scenario", case.name);

    let mut state = MatchState::from_scenario(&scenario).expect(case.name);
    state.clocks = journal.initial_clocks;
    assert_eq!(
        state.canonical_hash().unwrap(),
        journal.initial_state_hash,
        "{} initial hash",
        case.name
    );

    let mut saw_check = false;
    let mut saw_double_step = false;
    for record in &journal.records {
        assert_eq!(record.revision_before, state.revision, "{} tail", case.name);
        if let Action::Move { piece, to, .. } = record.action
            && let Some(moving) = state.pieces.get(&piece)
            && moving.kind == PieceKind::Pawn
            && moving.at.y.abs_diff(to.y) == 2
        {
            saw_double_step = true;
        }
        let transition =
            apply_timed_action(&scenario, &state, &record.action, record.elapsed_millis)
                .unwrap_or_else(|error| panic!("{} revision failed: {error}", case.name));
        assert_eq!(
            transition.state.revision, record.revision_after,
            "{} revision",
            case.name
        );
        assert_eq!(transition.events, record.events, "{} events", case.name);
        assert_eq!(
            transition.state.canonical_hash().unwrap(),
            record.state_hash,
            "{} revision {} hash",
            case.name,
            record.revision_after
        );
        saw_check |= Player::ALL
            .into_iter()
            .any(|player| is_in_check(&scenario, &transition.state, player).unwrap());
        state = transition.state;
    }

    assert_eq!(
        journal.replay(&scenario).unwrap(),
        state,
        "{} replay",
        case.name
    );
    assert_eq!(state.outcome, Some(case.outcome), "{} outcome", case.name);
    (journal, scenario, state, saw_check, saw_double_step)
}

#[test]
fn golden_journals_verify_every_revision_and_terminal_reason() {
    let mut reasons = Vec::new();
    for case in cases() {
        replay_every_revision(&case);
        reasons.push(case.outcome.reason);
    }
    reasons.sort_by_key(|reason| *reason as u8);
    reasons.dedup();
    assert_eq!(reasons.len(), 5, "every terminal reason has a fixture");
}

#[test]
fn golden_journals_cover_every_shipped_scenario() {
    let shipped = [
        "introductory-crossing",
        "crownlines-standard",
        "three-theatres",
    ];
    let scenario_ids = cases()
        .into_iter()
        .map(|case| ActionJournal::from_json(case.journal).unwrap().scenario_id)
        .collect::<Vec<_>>();
    for scenario_id in shipped {
        assert!(
            scenario_ids
                .iter()
                .any(|candidate| candidate == scenario_id),
            "shipped scenario {scenario_id} needs a golden journal"
        );
    }
}

#[test]
fn combined_golden_crosses_realms_promotion_check_and_special_pawn_rule() {
    let case = cases()
        .into_iter()
        .find(|case| case.name == "combined realms")
        .unwrap();
    let (journal, _, _, saw_check, saw_double_step) = replay_every_revision(&case);
    let events = journal
        .records
        .iter()
        .flat_map(|record| &record.events)
        .collect::<Vec<_>>();

    assert!(events.iter().any(|event| matches!(
        event,
        TransitionEvent::SettlementContinuityInterrupted { .. }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, TransitionEvent::SettlementEstablished { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, TransitionEvent::PawnProduced { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, TransitionEvent::PiecePromoted { .. }))
    );
    assert!(saw_check, "combined fixture must pass through check");
    assert!(
        saw_double_step,
        "combined fixture must use a Pawn double-step"
    );
}
