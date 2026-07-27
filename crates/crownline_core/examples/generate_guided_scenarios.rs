use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crownline_core::{
    Action, GUIDED_SCHEMA_VERSION, GuidedAiConfig, GuidedAiMode, GuidedCompletion, GuidedContent,
    GuidedEventPredicate, GuidedKind, GuidedPredicate, GuidedReplyNode, GuidedStage, GuidedStart,
    MatchState, ScenarioDefinition, apply_action, realm_control_score,
    scenario::{
        ArmySetup, BoardSize, CastlingRoute, Coord, Deployment, Edge, EdgeKind, Fortification,
        KeepDefinition, PieceKind, Player, PromotionSite, SCENARIO_SCHEMA_VERSION,
        ScenarioMetadata, ScenarioRules, SettlementSite, TileTerrain,
    },
    state::{MandatoryChoice, OutcomeReason, PieceId, PromotionEligibility, TurnPhase},
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
    scenarios.extend(royal_pack());
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
            completion_key: match category_key {
                "guided.category.realm" => "guided.realm.complete",
                "guided.category.royal" => "guided.royal.complete",
                _ => "guided.movement.complete",
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

fn royal_pack() -> Vec<ScenarioDefinition> {
    vec![
        en_passant_window(),
        knight_only_promotion(),
        frozen_promotion_batch(),
        answer_check(),
        castle_safely(),
        checkmate_finish(),
        accept_draw(),
        royal_open_practice(),
    ]
}

fn royal_stage(
    id: &str,
    success: Vec<GuidedPredicate>,
    hints: usize,
    prerequisite: Option<&str>,
) -> GuidedStage {
    let mut stage = stage(id, success, hints, prerequisite);
    stage.title_key = format!("guided.royal.{id}.title");
    stage.explanation_key = format!("guided.royal.{id}.explanation");
    stage.hint_keys = (1..=hints)
        .map(|index| format!("guided.royal.{id}.hint.{index}"))
        .collect();
    stage.action_limit = Some(12);
    stage.turn_limit = Some(6);
    stage
}

fn royal_guide(
    scenario: &mut ScenarioDefinition,
    state: MatchState,
    stages: Vec<GuidedStage>,
    kind: GuidedKind,
    ai: Option<GuidedAiConfig>,
) {
    guide_state(
        scenario,
        state,
        Player::South,
        "guided.category.royal",
        kind,
        stages,
        ai,
        Vec::new(),
    );
}

fn en_passant_window() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-royal-en-passant",
        "Royal I: The En Passant Window",
        &common(&[
            (Player::North, PieceKind::Pawn, Coord::new(3, 1)),
            (Player::South, PieceKind::Pawn, Coord::new(4, 3)),
        ]),
    );
    let mut state = start(&scenario);
    state.active_player = Player::North;
    let north_pawn = piece_at(&state, Coord::new(3, 1));
    state = apply_action(
        &scenario,
        &state,
        &Action::Move {
            player: Player::North,
            piece: north_pawn,
            to: Coord::new(3, 3),
        },
    )
    .expect("authored double-step must be legal")
    .state;
    royal_guide(
        &mut scenario,
        state,
        vec![royal_stage(
            "en_passant",
            vec![GuidedPredicate::Event(GuidedEventPredicate::Capture {
                piece: Some(north_pawn),
            })],
            2,
            None,
        )],
        GuidedKind::Tutorial,
        None,
    );
    scenario
}

fn knight_only_promotion() -> ScenarioDefinition {
    let site = Coord::new(2, 2);
    let mut scenario = base(
        "guided-royal-promotion-knight",
        "Royal II: Earn Promotion Choices",
        &common(&[(Player::South, PieceKind::Pawn, site)]),
    );
    scenario.promotion_sites.push(PromotionSite {
        id: "south-court".to_owned(),
        at: site,
    });
    let mut state = start(&scenario);
    let pawn = piece_at(&state, site);
    let eligibility = PromotionEligibility::default();
    state
        .promotion_candidates
        .insert(pawn, scenario.rules.promotion_cycles);
    state.phase = TurnPhase::ResolvingChoices {
        queue: vec![MandatoryChoice::Promote {
            pawn,
            site_index: 0,
            eligibility,
        }],
    };
    royal_guide(
        &mut scenario,
        state,
        vec![royal_stage(
            "promotion_knight",
            vec![GuidedPredicate::Event(GuidedEventPredicate::Promotion {
                pawn: Some(pawn),
                kind: Some(PieceKind::Knight),
            })],
            2,
            None,
        )],
        GuidedKind::Tutorial,
        None,
    );
    scenario
}

fn frozen_promotion_batch() -> ScenarioDefinition {
    let west = Coord::new(2, 2);
    let east = Coord::new(5, 2);
    let mut scenario = base(
        "guided-royal-promotion-batch",
        "Royal III: Frozen Promotion Batch",
        &common(&[
            (Player::South, PieceKind::Pawn, west),
            (Player::South, PieceKind::Pawn, east),
            (Player::South, PieceKind::Pawn, Coord::new(1, 4)),
            (Player::South, PieceKind::Pawn, Coord::new(6, 4)),
            (Player::South, PieceKind::Rook, Coord::new(1, 7)),
            (Player::South, PieceKind::Rook, Coord::new(6, 7)),
        ]),
    );
    scenario.promotion_sites = vec![
        PromotionSite {
            id: "west-court".to_owned(),
            at: west,
        },
        PromotionSite {
            id: "east-court".to_owned(),
            at: east,
        },
    ];
    add_settlement(&mut scenario, "west-realm", Coord::new(1, 4));
    add_settlement(&mut scenario, "east-realm", Coord::new(6, 4));
    let mut state = start(&scenario);
    own_settlement(
        &scenario,
        &mut state,
        0,
        Player::South,
        Coord::new(1, 4),
        true,
    );
    own_settlement(
        &scenario,
        &mut state,
        1,
        Player::South,
        Coord::new(6, 4),
        true,
    );
    let west_pawn = piece_at(&state, west);
    let east_pawn = piece_at(&state, east);
    state
        .promotion_candidates
        .insert(west_pawn, scenario.rules.promotion_cycles);
    state
        .promotion_candidates
        .insert(east_pawn, scenario.rules.promotion_cycles);
    let control = realm_control_score(&scenario, &state, Player::South)
        .expect("authored realm control must be measurable");
    assert_eq!(control.total(), 8);
    let eligibility = PromotionEligibility::from_control(control, scenario.rules.promotion_unlocks);
    state.phase = TurnPhase::ResolvingChoices {
        queue: vec![
            MandatoryChoice::Promote {
                pawn: west_pawn,
                site_index: 0,
                eligibility: eligibility.clone(),
            },
            MandatoryChoice::Promote {
                pawn: east_pawn,
                site_index: 1,
                eligibility,
            },
        ],
    };
    royal_guide(
        &mut scenario,
        state,
        vec![
            royal_stage(
                "promotion_bishop",
                vec![GuidedPredicate::Event(GuidedEventPredicate::Promotion {
                    pawn: Some(west_pawn),
                    kind: Some(PieceKind::Bishop),
                })],
                2,
                None,
            ),
            royal_stage(
                "promotion_rook",
                vec![GuidedPredicate::Event(GuidedEventPredicate::Promotion {
                    pawn: Some(east_pawn),
                    kind: Some(PieceKind::Rook),
                })],
                2,
                Some("promotion_bishop"),
            ),
        ],
        GuidedKind::Tutorial,
        None,
    );
    scenario
}

fn answer_check() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-royal-answer-check",
        "Royal IV: Answer Check Before Realm Work",
        &[
            (Player::North, PieceKind::King, Coord::new(0, 0)),
            (Player::North, PieceKind::Rook, Coord::new(4, 0)),
            (Player::North, PieceKind::Bishop, Coord::new(1, 4)),
            (Player::South, PieceKind::King, Coord::new(4, 7)),
            (Player::South, PieceKind::Bishop, Coord::new(5, 6)),
            (Player::South, PieceKind::Knight, Coord::new(3, 6)),
            (Player::South, PieceKind::Pawn, Coord::new(2, 6)),
            (Player::South, PieceKind::Rook, Coord::new(2, 7)),
        ],
    );
    add_settlement(&mut scenario, "delayed-realm", Coord::new(2, 6));
    let mut state = start(&scenario);
    own_settlement(
        &scenario,
        &mut state,
        0,
        Player::South,
        Coord::new(2, 6),
        false,
    );
    state.settlements[0].establishment_progress = 1;
    royal_guide(
        &mut scenario,
        state,
        vec![royal_stage(
            "answer_check",
            vec![
                GuidedPredicate::InCheck {
                    player: Player::South,
                    expected: false,
                },
                GuidedPredicate::PieceAt {
                    player: Player::South,
                    kind: PieceKind::Bishop,
                    at: Coord::new(4, 5),
                },
            ],
            2,
            None,
        )],
        GuidedKind::Tutorial,
        None,
    );
    scenario
}

fn castle_safely() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-royal-castling",
        "Royal V: Use an Authored Castling Route",
        &common(&[(Player::South, PieceKind::Rook, Coord::new(7, 7))]),
    );
    scenario.deployments.iter_mut().for_each(|piece| {
        if piece.player == Player::South && piece.kind == PieceKind::King {
            piece.at = Coord::new(4, 7);
        }
    });
    scenario.castling_routes.push(CastlingRoute {
        id: "south-east".to_owned(),
        player: Player::South,
        king_start: Coord::new(4, 7),
        rook_start: Coord::new(7, 7),
        king_path: vec![Coord::new(5, 7), Coord::new(6, 7)],
        king_destination: Coord::new(6, 7),
        rook_destination: Coord::new(5, 7),
    });
    let state = start(&scenario);
    royal_guide(
        &mut scenario,
        state,
        vec![royal_stage(
            "castle",
            vec![
                GuidedPredicate::PieceAt {
                    player: Player::South,
                    kind: PieceKind::King,
                    at: Coord::new(6, 7),
                },
                GuidedPredicate::PieceAt {
                    player: Player::South,
                    kind: PieceKind::Rook,
                    at: Coord::new(5, 7),
                },
            ],
            2,
            None,
        )],
        GuidedKind::Tutorial,
        None,
    );
    scenario
}

fn checkmate_finish() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-royal-checkmate",
        "Royal VI: Finish Checkmate",
        &[
            (Player::North, PieceKind::King, Coord::new(0, 0)),
            (Player::South, PieceKind::King, Coord::new(2, 2)),
            (Player::South, PieceKind::Queen, Coord::new(1, 2)),
        ],
    );
    let state = start(&scenario);
    royal_guide(
        &mut scenario,
        state,
        vec![royal_stage(
            "checkmate",
            vec![GuidedPredicate::Outcome {
                winner: Some(Player::South),
                reason: OutcomeReason::Checkmate,
            }],
            2,
            None,
        )],
        GuidedKind::Tutorial,
        None,
    );
    scenario
}

fn accept_draw() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-royal-draw",
        "Royal VII: A Canonical Draw",
        &common(&[]),
    );
    let mut state = start(&scenario);
    state.outstanding_draw_offer = Some(Player::North);
    royal_guide(
        &mut scenario,
        state,
        vec![royal_stage(
            "draw",
            vec![GuidedPredicate::Outcome {
                winner: None,
                reason: OutcomeReason::AgreedDraw,
            }],
            1,
            None,
        )],
        GuidedKind::Tutorial,
        None,
    );
    scenario
}

fn royal_open_practice() -> ScenarioDefinition {
    let mut scenario = base(
        "guided-royal-open-practice",
        "Royal Assessment: Crown and Realm",
        &[
            (Player::North, PieceKind::King, Coord::new(3, 0)),
            (Player::North, PieceKind::Rook, Coord::new(0, 0)),
            (Player::North, PieceKind::Bishop, Coord::new(6, 1)),
            (Player::North, PieceKind::Pawn, Coord::new(2, 2)),
            (Player::North, PieceKind::Pawn, Coord::new(5, 2)),
            (Player::South, PieceKind::King, Coord::new(4, 7)),
            (Player::South, PieceKind::Rook, Coord::new(7, 7)),
            (Player::South, PieceKind::Bishop, Coord::new(1, 6)),
            (Player::South, PieceKind::Pawn, Coord::new(2, 5)),
            (Player::South, PieceKind::Pawn, Coord::new(5, 5)),
        ],
    );
    add_settlement(&mut scenario, "central-west", Coord::new(2, 4));
    add_settlement(&mut scenario, "central-east", Coord::new(5, 3));
    let mut final_stage = royal_stage(
        "practice_finish",
        vec![GuidedPredicate::Event(GuidedEventPredicate::MatchEnded)],
        2,
        None,
    );
    final_stage.action_limit = Some(80);
    final_stage.turn_limit = Some(40);
    let state = start(&scenario);
    royal_guide(
        &mut scenario,
        state,
        vec![final_stage],
        GuidedKind::OpenPractice,
        Some(GuidedAiConfig {
            seat: Player::North,
            mode: GuidedAiMode::GeneralProfile {
                profile_id: "steward".to_owned(),
            },
            max_actions: Some(40),
        }),
    );
    scenario
}
