use std::collections::BTreeMap;

use crownline_core::{
    Action, GuidedAiMode, GuidedKind, GuidedPredicateContext, MatchState, ObjectiveResult,
    ScenarioDefinition, apply_action, legal_mandatory_choice_actions, legal_moves,
};
use serde::Deserialize;

const SOURCES: [&str; 7] = [
    include_str!("../../../assets/scenarios/guided/challenge-mate-court.ron"),
    include_str!("../../../assets/scenarios/guided/challenge-capture-line.ron"),
    include_str!("../../../assets/scenarios/guided/challenge-terrain-route.ron"),
    include_str!("../../../assets/scenarios/guided/challenge-settlement-defense.ron"),
    include_str!("../../../assets/scenarios/guided/challenge-production-deployment.ron"),
    include_str!("../../../assets/scenarios/guided/challenge-underpromotion.ron"),
    include_str!("../../../assets/scenarios/guided/challenge-warden-realm.ron"),
];

#[derive(Debug, Deserialize)]
struct ArchiveEntry {
    scenario_hash: String,
    start_hash: String,
    branching_factor: u16,
    shortest_solution_actions: u16,
    solutions: Vec<Action>,
    feature_tags: Vec<String>,
}

fn legal_actions(scenario: &ScenarioDefinition, state: &MatchState) -> Vec<Action> {
    let choices = legal_mandatory_choice_actions(state);
    if !choices.is_empty() {
        return choices;
    }
    let mut actions = legal_moves(scenario, state)
        .unwrap()
        .into_iter()
        .map(|movement| Action::Move {
            player: state.active_player,
            piece: movement.piece,
            to: movement.to,
        })
        .collect::<Vec<_>>();
    let hold = Action::Hold {
        player: state.active_player,
    };
    if apply_action(scenario, state, &hold).is_ok() {
        actions.push(hold);
    }
    actions
}

fn succeeds(scenario: &ScenarioDefinition, state: &MatchState, action: &Action) -> bool {
    let Ok(transition) = apply_action(scenario, state, action) else {
        return false;
    };
    scenario.guided.as_ref().unwrap().stages[0]
        .evaluate(&GuidedPredicateContext {
            scenario,
            state: &transition.state,
            events: &transition.events,
            actions_taken: 1,
            turns_elapsed: u16::from(transition.state.active_player != state.active_player),
        })
        .unwrap()
        == ObjectiveResult::Succeeded
}

#[test]
fn exact_archive_exhaustively_matches_every_legal_first_action_and_hash() {
    let archive: BTreeMap<String, ArchiveEntry> = ron::from_str(include_str!(
        "../../../assets/scenarios/guided/challenge-solutions.ron"
    ))
    .unwrap();
    assert_eq!(archive.len(), 6);
    let mut crownlines_specific = 0;
    for source in &SOURCES[..6] {
        let scenario: ScenarioDefinition = ron::from_str(source).unwrap();
        let guided = scenario.guided.as_ref().unwrap();
        assert_eq!(guided.kind, GuidedKind::Challenge);
        assert!(guided.ai.is_none());
        let state = MatchState::from_scenario(&scenario).unwrap();
        let actions = legal_actions(&scenario, &state);
        let regenerated = actions
            .iter()
            .filter(|action| succeeds(&scenario, &state, action))
            .cloned()
            .collect::<Vec<_>>();
        let entry = &archive[&scenario.id];
        assert_eq!(entry.scenario_hash, scenario.canonical_hash().unwrap());
        assert_eq!(entry.start_hash, state.canonical_hash().unwrap());
        assert_eq!(usize::from(entry.branching_factor), actions.len());
        assert_eq!(entry.shortest_solution_actions, 1);
        assert_eq!(entry.solutions, regenerated);
        assert!(!entry.solutions.is_empty());
        if entry.feature_tags.iter().any(|tag| {
            matches!(
                tag.as_str(),
                "terrain"
                    | "settlement"
                    | "transfer"
                    | "production"
                    | "promotion"
                    | "realm_control"
            )
        }) {
            crownlines_specific += 1;
        }
    }
    assert!(crownlines_specific * 2 >= archive.len());
}

#[test]
fn open_challenge_is_bounded_and_uses_only_the_named_warden_profile() {
    let scenario: ScenarioDefinition = ron::from_str(SOURCES[6]).unwrap();
    let guided = scenario.guided.as_ref().unwrap();
    assert_eq!(guided.kind, GuidedKind::Challenge);
    assert_eq!(guided.stages[0].action_limit, Some(100));
    assert_eq!(guided.stages[0].turn_limit, Some(50));
    assert!(matches!(
        &guided.ai.as_ref().unwrap().mode,
        GuidedAiMode::GeneralProfile { profile_id } if profile_id == "warden"
    ));
}
