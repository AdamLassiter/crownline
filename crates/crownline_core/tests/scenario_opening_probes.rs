mod support;

use support::all_opening_probes;

#[test]
fn deterministic_opening_probes_match_archived_evidence() {
    let actual = serde_json::to_string_pretty(&all_opening_probes()).unwrap();
    let expected = include_str!("../../../docs/playtests/automated-opening-probes.json").trim();
    assert_eq!(actual, expected);
}

#[test]
fn every_scenario_reaches_crossing_and_contact_without_immediate_mate() {
    for probe in all_opening_probes() {
        assert_eq!(probe.immediate_mate_moves, 0, "{}", probe.scenario_id);
        assert!(
            probe.first_crossing_turn.north.is_some(),
            "{} {:?}: North never crossed",
            probe.scenario_id,
            probe.flank
        );
        assert!(
            probe.first_crossing_turn.south.is_some(),
            "{} {:?}: South never crossed",
            probe.scenario_id,
            probe.flank
        );
        assert!(
            probe.first_contact_turn.is_some(),
            "{} {:?}: no interaction",
            probe.scenario_id,
            probe.flank
        );
        assert!(probe.completed_plies <= 80);
    }
}
