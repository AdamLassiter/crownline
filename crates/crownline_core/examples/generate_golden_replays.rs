use std::collections::BTreeMap;

use crownline_core::{
    Action, ActionJournal, AppendOutcome, ClockSettings, IdempotencyKey, MatchState,
    ScenarioDefinition, TransitionEvent, is_in_check, legal_mandatory_choice_actions,
    scenario::{
        ArmySetup, BoardSize, Coord, Deployment, PieceKind, Player, PromotionSite,
        SCENARIO_SCHEMA_VERSION, ScenarioMetadata, ScenarioRules, SettlementSite,
    },
    state::{MandatoryChoice, MatchOutcome, OutcomeReason, PieceId, TurnPhase},
};

const APPLICATION_VERSION: &str = "0.1.0-golden";

fn authored(source: &str) -> ScenarioDefinition {
    ron::from_str(source).expect("authored scenario must parse")
}

fn piece_at(state: &MatchState, at: Coord) -> PieceId {
    state
        .pieces
        .values()
        .find(|piece| piece.at == at)
        .expect("fixture piece must exist")
        .id
}

fn append(
    journal: &mut ActionJournal,
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    action: &Action,
    elapsed_millis: u64,
) {
    let key_byte = u8::try_from(journal.records.len() + 1).expect("small golden journal");
    let AppendOutcome::Accepted(transition) = journal
        .append_timed(
            scenario,
            state,
            IdempotencyKey([key_byte; 16]),
            action,
            elapsed_millis,
        )
        .expect("golden action must apply")
    else {
        panic!("golden keys must be unique");
    };
    *state = transition.state;
}

fn untimed_journal(
    scenario: &ScenarioDefinition,
    actions: impl IntoIterator<Item = Action>,
) -> (ActionJournal, MatchState) {
    let mut journal = ActionJournal::new(APPLICATION_VERSION, scenario).unwrap();
    let mut state = MatchState::from_scenario(scenario).unwrap();
    for action in actions {
        append(&mut journal, scenario, &mut state, &action, 0);
    }
    (journal, state)
}

fn simple_scenario(id: &str, deployments: Vec<Deployment>) -> ScenarioDefinition {
    ScenarioDefinition {
        schema_version: SCENARIO_SCHEMA_VERSION,
        id: id.to_owned(),
        metadata: ScenarioMetadata {
            name: id.to_owned(),
            description: "Golden replay fixture".to_owned(),
            expected_minutes: (1, 2),
            is_default: false,
        },
        board: BoardSize {
            width: 8,
            height: 8,
        },
        terrain: BTreeMap::new(),
        edges: BTreeMap::new(),
        deployments,
        settlements: Vec::new(),
        promotion_sites: Vec::new(),
        keeps: Vec::new(),
        fortifications: Vec::new(),
        castling_routes: Vec::new(),
        rules: ScenarioRules {
            army_setup: ArmySetup::Custom,
            ..ScenarioRules::default()
        },
        guided: None,
    }
}

fn combined_scenario() -> ScenarioDefinition {
    let mut scenario = simple_scenario(
        "golden-combined-realms",
        vec![
            Deployment {
                player: Player::North,
                kind: PieceKind::King,
                at: Coord::new(7, 0),
            },
            Deployment {
                player: Player::North,
                kind: PieceKind::Pawn,
                at: Coord::new(7, 1),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::King,
                at: Coord::new(7, 7),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::Rook,
                at: Coord::new(3, 7),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::Rook,
                at: Coord::new(7, 3),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::Pawn,
                at: Coord::new(3, 5),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::Pawn,
                at: Coord::new(0, 2),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::Pawn,
                at: Coord::new(4, 6),
            },
        ],
    );
    scenario.settlements.push(SettlementSite {
        id: "golden-town".to_owned(),
        at: Coord::new(3, 4),
    });
    scenario.promotion_sites.push(PromotionSite {
        id: "golden-court".to_owned(),
        at: Coord::new(0, 1),
    });
    scenario.rules.establishment_cycles = 1;
    scenario.rules.production_cycles = 1;
    scenario.rules.promotion_cycles = 1;
    scenario
        .validate()
        .expect("combined scenario must validate");
    scenario
}

fn resolve_choices(
    journal: &mut ActionJournal,
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
) {
    while let TurnPhase::ResolvingChoices { queue } = &state.phase {
        let action = match queue.first().expect("choice queue cannot be empty") {
            MandatoryChoice::Promote { .. } => legal_mandatory_choice_actions(state)
                .into_iter()
                .next()
                .expect("promotion has at least the Knight action"),
            MandatoryChoice::PlacePawn {
                settlement_index,
                legal_squares,
            } => Action::PlacePawn {
                player: state.active_player,
                settlement_index: *settlement_index,
                at: *legal_squares
                    .iter()
                    .next()
                    .expect("placement must be possible"),
            },
        };
        append(journal, scenario, state, &action, 0);
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "a golden journal is clearest as one linear, reviewable action path"
)]
fn combined_journal() -> (ActionJournal, MatchState) {
    let scenario = combined_scenario();
    let mut journal = ActionJournal::new(APPLICATION_VERSION, &scenario).unwrap();
    let mut state = MatchState::from_scenario(&scenario).unwrap();

    let founder = piece_at(&state, Coord::new(3, 5));
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Move {
            player: Player::South,
            piece: founder,
            to: Coord::new(3, 4),
        },
        0,
    );
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Hold {
            player: Player::North,
        },
        0,
    );
    resolve_choices(&mut journal, &scenario, &mut state);

    let promotion_pawn = piece_at(&state, Coord::new(0, 2));
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Move {
            player: Player::South,
            piece: promotion_pawn,
            to: Coord::new(0, 1),
        },
        0,
    );
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Hold {
            player: Player::North,
        },
        0,
    );
    resolve_choices(&mut journal, &scenario, &mut state);

    let double_step_pawn = piece_at(&state, Coord::new(4, 6));
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Move {
            player: Player::South,
            piece: double_step_pawn,
            to: Coord::new(4, 4),
        },
        0,
    );
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Hold {
            player: Player::North,
        },
        0,
    );
    resolve_choices(&mut journal, &scenario, &mut state);

    let checking_rook = piece_at(&state, Coord::new(7, 3));
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Move {
            player: Player::South,
            piece: checking_rook,
            to: Coord::new(7, 1),
        },
        0,
    );
    assert!(is_in_check(&scenario, &state, Player::North).unwrap());
    let north_king = piece_at(&state, Coord::new(7, 0));
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Move {
            player: Player::North,
            piece: north_king,
            to: Coord::new(6, 0),
        },
        0,
    );

    let governor = piece_at(&state, Coord::new(3, 7));
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Move {
            player: Player::South,
            piece: governor,
            to: Coord::new(2, 7),
        },
        0,
    );
    append(
        &mut journal,
        &scenario,
        &mut state,
        &Action::Resign {
            player: Player::North,
        },
        0,
    );

    let events = journal
        .records
        .iter()
        .flat_map(|record| &record.events)
        .collect::<Vec<_>>();
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
    assert!(events.iter().any(|event| matches!(
        event,
        TransitionEvent::SettlementContinuityInterrupted { .. }
    )));
    (journal, state)
}

fn emit(name: &str, journal: &ActionJournal, outcome: MatchOutcome) {
    let replayed = match name {
        "combined-realms.json" => journal.replay(&combined_scenario()).unwrap(),
        _ => panic!("authored fixture emission supplies replay state separately"),
    };
    assert_eq!(replayed.outcome, Some(outcome));
    println!("=== {name}");
    print!("{}", String::from_utf8(journal.to_json().unwrap()).unwrap());
    println!();
}

fn emit_scenario(name: &str, scenario: &ScenarioDefinition) {
    println!("=== {name}");
    println!(
        "{}",
        ron::ser::to_string_pretty(scenario, ron::ser::PrettyConfig::default()).unwrap()
    );
}

#[allow(
    clippy::too_many_lines,
    reason = "the generator keeps the complete reviewed fixture inventory together"
)]
fn main() {
    let introductory = authored(include_str!("../../../assets/scenarios/introductory.ron"));
    let standard = authored(include_str!("../../../assets/scenarios/standard.ron"));
    let large = authored(include_str!("../../../assets/scenarios/large.ron"));

    let authored_cases = [
        (
            "introductory-resignation.json",
            &introductory,
            vec![Action::Resign {
                player: Player::South,
            }],
            MatchOutcome {
                winner: Some(Player::North),
                reason: OutcomeReason::Resignation,
            },
        ),
        (
            "standard-agreed-draw.json",
            &standard,
            vec![
                Action::OfferDraw {
                    player: Player::South,
                },
                Action::RespondToDraw {
                    player: Player::North,
                    accept: true,
                },
            ],
            MatchOutcome {
                winner: None,
                reason: OutcomeReason::AgreedDraw,
            },
        ),
        (
            "large-repetition.json",
            &large,
            vec![
                Action::Hold {
                    player: Player::South,
                },
                Action::Hold {
                    player: Player::North,
                },
                Action::Hold {
                    player: Player::South,
                },
                Action::Hold {
                    player: Player::North,
                },
            ],
            MatchOutcome {
                winner: None,
                reason: OutcomeReason::ThreefoldRepetition,
            },
        ),
    ];
    for (name, scenario, actions, outcome) in authored_cases {
        let (journal, state) = untimed_journal(scenario, actions);
        assert_eq!(state.outcome, Some(outcome));
        assert_eq!(journal.replay(scenario).unwrap(), state);
        println!("=== {name}");
        print!("{}", String::from_utf8(journal.to_json().unwrap()).unwrap());
        println!();
    }

    let checkmate = simple_scenario(
        "golden-checkmate",
        vec![
            Deployment {
                player: Player::North,
                kind: PieceKind::King,
                at: Coord::new(0, 0),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::King,
                at: Coord::new(2, 2),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::Queen,
                at: Coord::new(1, 2),
            },
        ],
    );
    emit_scenario("checkmate.ron", &checkmate);
    let initial = MatchState::from_scenario(&checkmate).unwrap();
    let queen = piece_at(&initial, Coord::new(1, 2));
    let (journal, state) = untimed_journal(
        &checkmate,
        [Action::Move {
            player: Player::South,
            piece: queen,
            to: Coord::new(1, 1),
        }],
    );
    assert_eq!(state.outcome.unwrap().reason, OutcomeReason::Checkmate);
    println!("=== checkmate.json");
    print!("{}", String::from_utf8(journal.to_json().unwrap()).unwrap());
    println!();

    let timeout = simple_scenario(
        "golden-timeout",
        vec![
            Deployment {
                player: Player::North,
                kind: PieceKind::King,
                at: Coord::new(4, 0),
            },
            Deployment {
                player: Player::South,
                kind: PieceKind::King,
                at: Coord::new(4, 7),
            },
        ],
    );
    emit_scenario("timeout.ron", &timeout);
    let settings = ClockSettings {
        base_minutes: 1,
        increment_seconds: 0,
    };
    let mut journal =
        ActionJournal::new_with_clocks(APPLICATION_VERSION, &timeout, settings).unwrap();
    let mut state = MatchState::from_scenario(&timeout).unwrap();
    state.clocks = journal.initial_clocks;
    append(
        &mut journal,
        &timeout,
        &mut state,
        &Action::Hold {
            player: Player::South,
        },
        60_000,
    );
    assert_eq!(state.outcome.unwrap().reason, OutcomeReason::Timeout);
    println!("=== timeout.json");
    print!("{}", String::from_utf8(journal.to_json().unwrap()).unwrap());
    println!();

    emit_scenario("combined-realms.ron", &combined_scenario());
    let (journal, state) = combined_journal();
    let outcome = state.outcome.expect("combined journal must be terminal");
    emit("combined-realms.json", &journal, outcome);
}
