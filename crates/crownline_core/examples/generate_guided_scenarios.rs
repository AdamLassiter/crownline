use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crownline_core::{
    Action, GUIDED_SCHEMA_VERSION, GuidedAiConfig, GuidedAiMode, GuidedCompletion, GuidedContent,
    GuidedEventPredicate, GuidedKind, GuidedPredicate, GuidedReplyNode, GuidedStage, GuidedStart,
    MatchState, ScenarioDefinition, apply_action,
    scenario::{
        ArmySetup, BoardSize, Coord, Deployment, Edge, EdgeKind, Fortification, KeepDefinition,
        PieceKind, Player, SCENARIO_SCHEMA_VERSION, ScenarioMetadata, ScenarioRules,
        SettlementSite, TileTerrain,
    },
    state::{MandatoryChoice, PieceId, TurnPhase},
};

fn main() {
    let output = Path::new("assets/scenarios/guided");
    fs::create_dir_all(output).expect("guided scenario directory must be writable");
    for scenario in guided_pack() {
        scenario
            .validate()
            .expect("generated guided scenario must validate");
        let encoded = ron::ser::to_string_pretty(&scenario, ron::ser::PrettyConfig::default())
            .expect("guided scenario must serialize");
        let decoded: ScenarioDefinition =
            ron::from_str(&encoded).expect("generated guided scenario must round-trip");
        assert_eq!(decoded, scenario);
        fs::write(
            output.join(format!("{}.ron", scenario.id)),
            format!("{encoded}\n"),
        )
        .expect("guided scenario must be writable");
    }
}

fn guided_pack() -> Vec<ScenarioDefinition> {
    let mut scenarios = vec![
        capture_and_blocking(),
        knight_jump(),
        forest_stop(),
        mountain_jump(),
        bridge_crossing(),
        tower_rook_wall(),
        crossing_open_practice(),
    ];
    scenarios.extend(realm_pack());
    scenarios
}

fn base(id: &str, name: &str, pieces: &[(Player, PieceKind, Coord)]) -> ScenarioDefinition {
    ScenarioDefinition {
        schema_version: SCENARIO_SCHEMA_VERSION,
        id: id.to_owned(),
        metadata: ScenarioMetadata {
            name: name.to_owned(),
            description: "A compact guided movement and terrain lesson.".to_owned(),
            expected_minutes: (2, 6),
            is_default: false,
        },
        board: BoardSize {
            width: 8,
            height: 8,
        },
        terrain: BTreeMap::new(),
        edges: BTreeMap::new(),
        deployments: pieces
            .iter()
            .map(|(player, kind, at)| Deployment {
                player: *player,
                kind: *kind,
                at: *at,
            })
            .collect(),
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

fn start(scenario: &ScenarioDefinition) -> MatchState {
    MatchState::from_scenario(scenario).expect("unguided lesson base must validate")
}

fn piece_at(state: &MatchState, at: Coord) -> PieceId {
    state
        .pieces
        .values()
        .find(|piece| piece.at == at)
        .expect("lesson piece must exist")
        .id
}

fn stage(
    id: &str,
    success: Vec<GuidedPredicate>,
    hints: usize,
    prerequisite: Option<&str>,
) -> GuidedStage {
    GuidedStage {
        id: id.to_owned(),
        title_key: format!("guided.movement.{id}.title"),
        explanation_key: format!("guided.movement.{id}.explanation"),
        hint_keys: (1..=hints)
            .map(|index| format!("guided.movement.{id}.hint.{index}"))
            .collect(),
        prerequisites: prerequisite.into_iter().map(str::to_owned).collect(),
        success,
        failure: Vec::new(),
        action_limit: Some(8),
        turn_limit: Some(4),
    }
}

fn guide(scenario: &mut ScenarioDefinition, stages: Vec<GuidedStage>, ai: Option<GuidedAiConfig>) {
    let state = start(scenario);
    let kind = if ai.is_some() {
        GuidedKind::OpenPractice
    } else {
        GuidedKind::Tutorial
    };
    guide_state(
        scenario,
        state,
        Player::South,
        "guided.category.movement_terrain",
        kind,
        stages,
        ai,
        Vec::new(),
    );
}

#[allow(clippy::too_many_arguments)]
fn guide_state(
    scenario: &mut ScenarioDefinition,
    mut state: MatchState,
    human_seat: Player,
    category_key: &str,
    kind: GuidedKind,
    stages: Vec<GuidedStage>,
    ai: Option<GuidedAiConfig>,
    reply_nodes: Vec<GuidedReplyNode>,
) {
    state.repetition_counts.clear();
    state
        .repetition_counts
        .insert(state.repetition_key().expect("guided state must hash"), 1);
    scenario.guided = Some(GuidedContent {
        schema_version: GUIDED_SCHEMA_VERSION,
        id: scenario.id.clone(),
        kind,
        category_key: category_key.to_owned(),
        start: GuidedStart {
            state,
            human_seat,
            allow_clock: false,
            allow_controller_changes: false,
        },
        stages,
        ai,
        completion: Some(GuidedCompletion {
            completion_key: if category_key == "guided.category.realm" {
                "guided.realm.complete"
            } else {
                "guided.movement.complete"
            }
            .to_owned(),
            next_guided_id: None,
            records_best_actions: true,
            records_best_turns: true,
        }),
        reply_nodes,
    });
}

fn common(extra: &[(Player, PieceKind, Coord)]) -> Vec<(Player, PieceKind, Coord)> {
    let mut pieces = vec![
        (Player::North, PieceKind::King, Coord::new(0, 0)),
        (Player::South, PieceKind::King, Coord::new(7, 7)),
    ];
    pieces.extend_from_slice(extra);
    pieces
}

fn capture_and_blocking() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-movement-capture",
        "Movement I: Lines and Captures",
        &common(&[
            (Player::North, PieceKind::Pawn, Coord::new(4, 6)),
            (Player::South, PieceKind::Pawn, Coord::new(1, 5)),
            (Player::South, PieceKind::Rook, Coord::new(1, 6)),
        ]),
    );
    let state = start(&scenario);
    let target = piece_at(&state, Coord::new(4, 6));
    guide(
        &mut scenario,
        vec![stage(
            "capture",
            vec![GuidedPredicate::Event(GuidedEventPredicate::Capture {
                piece: Some(target),
            })],
            2,
            None,
        )],
        None,
    );
    scenario
}

fn knight_jump() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-movement-knight",
        "Movement II: Knight Jumps",
        &common(&[(Player::South, PieceKind::Knight, Coord::new(2, 6))]),
    );
    scenario
        .terrain
        .insert(Coord::new(2, 5), TileTerrain::Mountain);
    scenario
        .terrain
        .insert(Coord::new(3, 5), TileTerrain::Mountain);
    guide(
        &mut scenario,
        vec![stage(
            "knight_jump",
            vec![GuidedPredicate::PieceAt {
                player: Player::South,
                kind: PieceKind::Knight,
                at: Coord::new(3, 4),
            }],
            2,
            None,
        )],
        None,
    );
    scenario
}

fn forest_stop() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-terrain-forest",
        "Terrain I: Forests Stop Rays",
        &common(&[(Player::South, PieceKind::Rook, Coord::new(1, 6))]),
    );
    scenario
        .terrain
        .insert(Coord::new(1, 3), TileTerrain::Forest);
    let rook = piece_at(&start(&scenario), Coord::new(1, 6));
    guide(
        &mut scenario,
        vec![stage(
            "forest",
            vec![GuidedPredicate::Event(GuidedEventPredicate::EnterTerrain {
                piece: Some(rook),
                terrain: TileTerrain::Forest,
            })],
            2,
            None,
        )],
        None,
    );
    scenario
}

fn mountain_jump() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-terrain-mountain",
        "Terrain II: Mountains Block",
        &common(&[(Player::South, PieceKind::Knight, Coord::new(1, 6))]),
    );
    scenario
        .terrain
        .insert(Coord::new(1, 5), TileTerrain::Mountain);
    scenario
        .terrain
        .insert(Coord::new(2, 5), TileTerrain::Mountain);
    guide(
        &mut scenario,
        vec![stage(
            "mountain",
            vec![GuidedPredicate::PieceAt {
                player: Player::South,
                kind: PieceKind::Knight,
                at: Coord::new(2, 4),
            }],
            2,
            None,
        )],
        None,
    );
    scenario
}

fn bridge_crossing() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-crossing-bridge",
        "Crossings I: Bridge the River",
        &common(&[
            (Player::South, PieceKind::Rook, Coord::new(2, 5)),
            (Player::South, PieceKind::Rook, Coord::new(3, 5)),
        ]),
    );
    for x in 0..scenario.board.width {
        scenario.edges.insert(
            Edge::new(Coord::new(x, 3), Coord::new(x, 4)),
            if x == 3 {
                EdgeKind::Bridge
            } else {
                EdgeKind::River
            },
        );
    }
    let rook = piece_at(&start(&scenario), Coord::new(3, 5));
    guide(
        &mut scenario,
        vec![stage(
            "bridge",
            vec![GuidedPredicate::Event(GuidedEventPredicate::CrossEdge {
                piece: Some(rook),
                kind: EdgeKind::Bridge,
            })],
            2,
            None,
        )],
        None,
    );
    scenario
}

fn tower_rook_wall() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-crossing-tower-rook",
        "Crossings II: Gates and Tower Rooks",
        &common(&[(Player::South, PieceKind::Rook, Coord::new(3, 5))]),
    );
    let wall = Edge::new(Coord::new(3, 5), Coord::new(3, 4));
    scenario.edges.insert(wall, EdgeKind::Wall);
    scenario.edges.insert(
        Edge::new(Coord::new(4, 5), Coord::new(4, 4)),
        EdgeKind::Gate,
    );
    scenario.fortifications.push(Fortification {
        id: "south-tower".to_owned(),
        owner: Player::South,
        tower: Coord::new(3, 5),
        projected_wall: wall,
    });
    scenario.keeps.push(KeepDefinition {
        id: "south-keep".to_owned(),
        owner: Player::South,
        tiles: BTreeSet::from([Coord::new(3, 5), Coord::new(4, 5)]),
        gates: BTreeSet::from([Edge::new(Coord::new(4, 5), Coord::new(4, 4))]),
        fortification_ids: BTreeSet::from(["south-tower".to_owned()]),
    });
    let rook = piece_at(&start(&scenario), Coord::new(3, 5));
    guide(
        &mut scenario,
        vec![stage(
            "tower_rook",
            vec![GuidedPredicate::Event(GuidedEventPredicate::CrossEdge {
                piece: Some(rook),
                kind: EdgeKind::Wall,
            })],
            2,
            None,
        )],
        None,
    );
    scenario
}

fn crossing_open_practice() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-movement-open-practice",
        "Movement Assessment: Crossing Contact",
        &common(&[
            (Player::North, PieceKind::Pawn, Coord::new(3, 2)),
            (Player::South, PieceKind::Rook, Coord::new(3, 5)),
        ]),
    );
    for x in 0..scenario.board.width {
        scenario.edges.insert(
            Edge::new(Coord::new(x, 3), Coord::new(x, 4)),
            if x == 3 {
                EdgeKind::Bridge
            } else {
                EdgeKind::River
            },
        );
    }
    let state = start(&scenario);
    let rook = piece_at(&state, Coord::new(3, 5));
    let pawn = piece_at(&state, Coord::new(3, 2));
    guide(
        &mut scenario,
        vec![
            stage(
                "assessment_cross",
                vec![GuidedPredicate::Event(GuidedEventPredicate::CrossEdge {
                    piece: Some(rook),
                    kind: EdgeKind::Bridge,
                })],
                0,
                None,
            ),
            stage(
                "assessment_capture",
                vec![GuidedPredicate::Event(GuidedEventPredicate::Capture {
                    piece: Some(pawn),
                })],
                0,
                Some("assessment_cross"),
            ),
        ],
        Some(GuidedAiConfig {
            seat: Player::North,
            mode: GuidedAiMode::GeneralProfile {
                profile_id: "apprentice".to_owned(),
            },
            max_actions: Some(8),
        }),
    );
    scenario
}

fn realm_pack() -> Vec<ScenarioDefinition> {
    vec![
        settlement_claim(),
        governance_cycle(),
        production_choice(),
        settlement_transfer(),
        transfer_cancellation(),
        realm_open_practice(),
    ]
}

fn realm_stage(
    id: &str,
    success: Vec<GuidedPredicate>,
    hints: usize,
    prerequisite: Option<&str>,
) -> GuidedStage {
    let mut stage = stage(id, success, hints, prerequisite);
    stage.title_key = format!("guided.realm.{id}.title");
    stage.explanation_key = format!("guided.realm.{id}.explanation");
    stage.hint_keys = (1..=hints)
        .map(|index| format!("guided.realm.{id}.hint.{index}"))
        .collect();
    stage.action_limit = Some(10);
    stage.turn_limit = Some(5);
    stage
}

fn add_settlement(scenario: &mut ScenarioDefinition, id: &str, at: Coord) {
    scenario.settlements.push(SettlementSite {
        id: id.to_owned(),
        at,
    });
}

fn own_settlement(
    scenario: &ScenarioDefinition,
    state: &mut MatchState,
    index: usize,
    owner: Player,
    founder_at: Coord,
    established: bool,
) {
    let founder = piece_at(state, founder_at);
    let settlement = &mut state.settlements[index];
    settlement.owner = Some(owner);
    settlement.founder = Some(founder);
    settlement.established = established;
    settlement.establishment_progress = if established {
        scenario.rules.establishment_cycles
    } else {
        0
    };
}

fn settlement_claim() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-realm-claim",
        "Realm I: Claim and Founder",
        &common(&[(Player::South, PieceKind::Pawn, Coord::new(3, 5))]),
    );
    add_settlement(&mut scenario, "claim-village", Coord::new(3, 4));
    let state = start(&scenario);
    guide_state(
        &mut scenario,
        state,
        Player::South,
        "guided.category.realm",
        GuidedKind::Tutorial,
        vec![realm_stage(
            "claim",
            vec![GuidedPredicate::Event(
                GuidedEventPredicate::SettlementClaimed {
                    settlement_index: Some(0),
                },
            )],
            2,
            None,
        )],
        None,
        Vec::new(),
    );
    scenario
}

fn governance_cycle() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-realm-governance",
        "Realm II: Governance and Continuity",
        &common(&[
            (Player::South, PieceKind::Pawn, Coord::new(3, 3)),
            (Player::South, PieceKind::Rook, Coord::new(3, 7)),
        ]),
    );
    scenario.rules.establishment_cycles = 2;
    add_settlement(&mut scenario, "governed-village", Coord::new(3, 3));
    let mut state = start(&scenario);
    own_settlement(
        &scenario,
        &mut state,
        0,
        Player::South,
        Coord::new(3, 3),
        false,
    );
    state.settlements[0].establishment_progress = 1;
    state.active_player = Player::North;
    guide_state(
        &mut scenario,
        state,
        Player::North,
        "guided.category.realm",
        GuidedKind::Tutorial,
        vec![realm_stage(
            "governance",
            vec![GuidedPredicate::Event(
                GuidedEventPredicate::SettlementEstablished {
                    settlement_index: Some(0),
                },
            )],
            2,
            None,
        )],
        None,
        Vec::new(),
    );
    scenario
}

fn production_choice() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-realm-production",
        "Realm III: Production and Placement",
        &common(&[
            (Player::South, PieceKind::Pawn, Coord::new(3, 3)),
            (Player::South, PieceKind::Rook, Coord::new(3, 7)),
        ]),
    );
    add_settlement(&mut scenario, "productive-village", Coord::new(3, 3));
    let mut state = start(&scenario);
    own_settlement(
        &scenario,
        &mut state,
        0,
        Player::South,
        Coord::new(3, 3),
        true,
    );
    state.settlements[0].production_progress = scenario.rules.production_cycles;
    state.phase = TurnPhase::ResolvingChoices {
        queue: vec![MandatoryChoice::PlacePawn {
            settlement_index: 0,
            legal_squares: BTreeSet::from([
                Coord::new(2, 2),
                Coord::new(3, 2),
                Coord::new(4, 2),
                Coord::new(2, 3),
                Coord::new(4, 3),
                Coord::new(2, 4),
                Coord::new(3, 4),
                Coord::new(4, 4),
            ]),
        }],
    };
    guide_state(
        &mut scenario,
        state,
        Player::South,
        "guided.category.realm",
        GuidedKind::Tutorial,
        vec![realm_stage(
            "production",
            vec![GuidedPredicate::Event(GuidedEventPredicate::PawnProduced {
                settlement_index: Some(0),
            })],
            2,
            None,
        )],
        None,
        Vec::new(),
    );
    scenario
}

fn contested_base(id: &str, name: &str) -> (ScenarioDefinition, MatchState, PieceId) {
    let mut scenario = base(
        id,
        name,
        &common(&[
            (Player::North, PieceKind::Pawn, Coord::new(0, 1)),
            (Player::North, PieceKind::Rook, Coord::new(3, 0)),
            (Player::South, PieceKind::Pawn, Coord::new(3, 4)),
        ]),
    );
    add_settlement(&mut scenario, "contested-village", Coord::new(3, 3));
    let mut state = start(&scenario);
    own_settlement(
        &scenario,
        &mut state,
        0,
        Player::North,
        Coord::new(0, 1),
        true,
    );
    let candidate = piece_at(&state, Coord::new(3, 4));
    let contested = apply_action(
        &scenario,
        &state,
        &Action::Move {
            player: Player::South,
            piece: candidate,
            to: Coord::new(3, 3),
        },
    )
    .expect("contest setup move must be legal")
    .state;
    (scenario, contested, candidate)
}

fn settlement_transfer() -> ScenarioDefinition {
    let (mut scenario, state, _) =
        contested_base("guided-realm-transfer", "Realm IV: Contest and Transfer");
    guide_state(
        &mut scenario,
        state,
        Player::North,
        "guided.category.realm",
        GuidedKind::Tutorial,
        vec![realm_stage(
            "transfer",
            vec![GuidedPredicate::Event(
                GuidedEventPredicate::SettlementTransferred {
                    settlement_index: Some(0),
                },
            )],
            2,
            None,
        )],
        None,
        Vec::new(),
    );
    scenario
}

fn transfer_cancellation() -> ScenarioDefinition {
    let (mut scenario, state, _) = contested_base(
        "guided-realm-transfer-cancel",
        "Realm V: Defend a Contested Settlement",
    );
    guide_state(
        &mut scenario,
        state,
        Player::North,
        "guided.category.realm",
        GuidedKind::Tutorial,
        vec![realm_stage(
            "transfer_cancel",
            vec![GuidedPredicate::Event(
                GuidedEventPredicate::SettlementTransferCancelled {
                    settlement_index: Some(0),
                },
            )],
            2,
            None,
        )],
        None,
        Vec::new(),
    );
    scenario
}

fn realm_open_practice() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-realm-open-practice",
        "Realm Assessment: Shared Governance",
        &common(&[
            (Player::South, PieceKind::Pawn, Coord::new(3, 3)),
            (Player::South, PieceKind::Pawn, Coord::new(5, 6)),
            (Player::South, PieceKind::Pawn, Coord::new(6, 7)),
            (Player::South, PieceKind::Rook, Coord::new(3, 7)),
        ]),
    );
    add_settlement(&mut scenario, "south-road", Coord::new(3, 3));
    add_settlement(&mut scenario, "south-flank", Coord::new(6, 7));
    add_settlement(&mut scenario, "open-village", Coord::new(5, 5));
    let mut state = start(&scenario);
    own_settlement(
        &scenario,
        &mut state,
        0,
        Player::South,
        Coord::new(3, 3),
        true,
    );
    own_settlement(
        &scenario,
        &mut state,
        1,
        Player::South,
        Coord::new(6, 7),
        true,
    );
    guide_state(
        &mut scenario,
        state,
        Player::South,
        "guided.category.realm",
        GuidedKind::OpenPractice,
        vec![
            realm_stage(
                "practice_claim",
                vec![GuidedPredicate::Event(
                    GuidedEventPredicate::SettlementClaimed {
                        settlement_index: Some(2),
                    },
                )],
                0,
                None,
            ),
            realm_stage(
                "practice_govern",
                vec![
                    GuidedPredicate::SettlementGoverned {
                        settlement_index: 0,
                        player: Player::South,
                    },
                    GuidedPredicate::SettlementGoverned {
                        settlement_index: 1,
                        player: Player::South,
                    },
                    GuidedPredicate::SettlementOwned {
                        settlement_index: 2,
                        player: Player::South,
                    },
                ],
                0,
                Some("practice_claim"),
            ),
        ],
        Some(GuidedAiConfig {
            seat: Player::North,
            mode: GuidedAiMode::GeneralProfile {
                profile_id: "steward".to_owned(),
            },
            max_actions: Some(12),
        }),
        Vec::new(),
    );
    scenario
}
