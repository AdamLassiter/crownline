use crownline_core::{
    Action, ActionJournal, AppendOutcome, FogScenarioVariant, IdempotencyKey, MatchState,
    ScenarioDefinition, legal_moves, project_player_view,
};

fn fog_scenario() -> ScenarioDefinition {
    let base: ScenarioDefinition =
        ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
    let variant: FogScenarioVariant = ron::from_str(include_str!(
        "../../../assets/scenarios/introductory-fog.ron"
    ))
    .unwrap();
    variant.apply(&base).unwrap()
}

fn deterministic_trace() -> Vec<String> {
    let scenario = fog_scenario();
    let mut state = MatchState::from_scenario(&scenario).unwrap();
    let mut journal = ActionJournal::new("fog-golden-v1", &scenario).unwrap();
    let mut trace = Vec::new();
    for step in 0..=12 {
        let north = project_player_view(&scenario, &state, crownline_core::scenario::Player::North)
            .unwrap();
        let south = project_player_view(&scenario, &state, crownline_core::scenario::Player::South)
            .unwrap();
        trace.push(format!(
            "{}:{}:{}:{}:{}:{}",
            state.revision,
            state.canonical_hash().unwrap(),
            north.projection_hash,
            south.projection_hash,
            north.squares.len(),
            south.squares.len()
        ));
        if step == 12 {
            break;
        }
        let moves = legal_moves(&scenario, &state).unwrap();
        let candidate = moves[(step * 17 + 5) % moves.len()];
        let action = Action::Move {
            player: state.active_player,
            piece: candidate.piece,
            to: candidate.to,
        };
        let AppendOutcome::Accepted(transition) = journal
            .append(
                &scenario,
                &state,
                IdempotencyKey([u8::try_from(step).unwrap(); 16]),
                &action,
            )
            .unwrap()
        else {
            panic!("golden action must be newly accepted")
        };
        state = transition.state;
    }
    let replayed = journal.replay(&scenario).unwrap();
    assert_eq!(replayed, state);
    for seat in crownline_core::scenario::Player::ALL {
        assert_eq!(
            project_player_view(&scenario, &replayed, seat)
                .unwrap()
                .projection_hash,
            project_player_view(&scenario, &state, seat)
                .unwrap()
                .projection_hash
        );
    }
    trace
}

#[test]
fn golden_fog_trace_reproduces_both_exploration_masks_and_projection_hashes() {
    const EXPECTED: &[&str] = &[
        "0:854199f303aedb2d7fd7549b5ee4e0e45c12bd4fec8adba7fec4d65d0e6c80b5:6164acce4484e07796276ebc004ec6368935808aadfbbf17cbbde8e6906bf920:3932c104369cbb62d66caca3a2e7b80d54a6b8947c57878c2dcf0355846a4c0b:70:70",
        "1:7d0a51c0b6b606ae98a06c258f251dc6857a8b156a23bed7d95628fc2abd9442:ef2c778e1bbac46ca926a2487c07b57af0203f19b42bf4116e0c0e3d795db9b0:7ae03de3bf2e30cbc05d7394829efa45633b111244073c4eb8b53e827ebaa892:70:74",
        "2:10d5ee33eed5846162b5a8865524456ba3d5d159bef022c61aae0c6eb753b6a6:43e1f3b0a45c7444b95e4a2ca505a3c4fcd49104b5ceeadec973153e24d34cc8:60a9b9c251cadaf04fe149b32f09d30825e94f2e159cd476c6918adbf9b1bf75:77:74",
        "3:c816a638871d19b6a2ea83934253ddf3d7cc19ba5fd1015c145d989a7314c1d3:a133e5d845155b2cbaaa36a865b778cefe0e196ea4b9917b73746a4da89a59e8:53f31ee1f655e851cd40b1dca850f34d16f9e083781296142ed3d930673b1943:77:74",
        "4:284361d359b5fe6275e5103d08e4e7d957e72a915d6fbd2df09c2684c953431f:07dd80702fb4aa9433f8bce70ee24a7fe330a006f78e28ec1ba07179b73e258b:b50551e11682b01177a5374053669dfb25ceb98c4b17c6cef0feab0ee22ece0c:97:74",
        "5:95fcd8fd426b4d7f507c3b73b94bd76fe85790e2cedc2e0f45f2c52321667fcb:bc3450aaf9dc30a8ead5a8ba885606f88a620a75ef059442a75713d178e7c566:f7725b57576ec58bec0791a70faff61f710569cb969ca6aec4451c85854c9a79:97:78",
        "6:7f109b5bd3a2eedca4a7ed9a7bb353d0c63d6135d0cda91d3fcaca49c57450e9:c659a49c86d90f320aaf8d50eb1381a100e606494eeaf43caf4c2edfa9c7d9ea:1298d95e6bf0df92b1e6818e9512d3345e6e0d1bee40ebee94f2cdca744654c9:108:78",
        "7:a0ccdecc287dc4f2208259a8a141d43d24f41ee1f150694bb36caeefdafa6f06:ace813416d94357f4c3210d1a58b072e5616638d274d7a48f1c84ecb340f981e:ed02f5e10e46fb79d43d44607c8aac0ec4ff15a404863c97e14262925b4c18d3:108:85",
        "8:964832ccda7e86fa7cf1b5a2529f619e894b26e0422e1b81642cf8067c8fd36f:431b44aad9dd77b7e1fab5f639076ff9cd087a24ed54338ba53fac788c652db1:8ace9ea74d78ab811d51e307c905d30aa0e1bc006fc55b12357ae3204539bd3b:110:85",
        "9:251b4cb4d0dd25ca4b9ac09134a51072fd41b98d4f781192ea06710836ff56a2:8e49bc2b1cf04f2bd5bd8a958b3d3bfba8d3fcd2667348ced77839ebadf5d37a:2c34cc43bf0a0911d4d414e50f33145a37af006ac92d9dccef9d65bdb723d7ee:110:85",
        "10:45507a1695820d0ee7f24a386c162064d37da836d2ddbfbd35fb1d4b8d1cca23:205debcfe54fd6a91e0d36f13fabbdc45dab2574cd1b58977759d766487dbe33:a9bd3b31e75d5fed7ed01b7cb53587d416bb647a743b36fc370ce052412ffda4:110:85",
        "11:13dfe9b5666d7c1f0a0d3344bfbb582c0a8fa95b3f1331c1a11dd964040689d2:8de16ca8ad72bbfce06a2bf507157f23b41ebf4618439f68416959a905d74dd6:2d24fcabb92efad6904ff7494e68c61fce5679fb9f71fd7541a38c979367780d:110:102",
        "12:052b1b4cc487a24fe3966d1be738b826eb716c232ec03b9babb38b18ede03558:a5cf185a1522ccbd29da0aac1f8b928bcd508079e201e78b7c6d1779454064b4:f87633cf75ea83fc1edb2743896a6be7adcefb6eb88dbe0174252bc08d3dff6b:114:102",
    ];
    assert_eq!(deterministic_trace(), EXPECTED);
}

#[test]
fn fog_variant_is_opt_in_and_preserves_the_base_scenario() {
    let base: ScenarioDefinition =
        ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
    let original_hash = base.canonical_hash().unwrap();
    let fog = fog_scenario();
    assert_eq!(base.canonical_hash().unwrap(), original_hash);
    assert!(base.rules.fog.is_none());
    assert_eq!(fog.rules.fog.unwrap().vision_radius, 3);
    assert_ne!(fog.canonical_hash().unwrap(), original_hash);
}
