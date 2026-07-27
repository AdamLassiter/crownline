# Playtest evidence

This directory holds reproducible, non-identifying evidence used to support Crownlines playtesting. Automated probes are regression evidence, not human playtest results.

## Automated opening probes

`automated-opening-probes.json` records deterministic West- and East-flank opening probes for every shipped scenario. Each probe uses only legal core actions, resolves mandatory choices, and records:

- the scenario canonical hash and chosen opposing-half settlement targets;
- first central-boundary crossing for each player;
- first settlement claim for each player when reached before the probe stops;
- first objective interaction through a claim, transfer, contest, capture, or check;
- the count of initial legal moves that cause immediate checkmate;
- final canonical state and a SHA-256 digest of the ordered action/event/state-hash trace.

Regenerate the evidence with:

```sh
cargo run -p crownline_core --example generate_opening_probes
```

The integration test regenerates the same report in memory and requires exact equality with the archived file. It also requires both players to cross the central boundary, an objective interaction to occur within 80 plies, and no immediate first-move checkmate.

These probes exercise deterministic geometry and rules paths. They cannot establish player comprehension, match duration, subjective balance, geographic or first-player bias under human decision-making, or whether a scenario is enjoyable. Those remain acceptance gates in the scenario validation and structured balance-playtest tasks.

## Automated promotion progression probes

`automated-promotion-progression.json` records the reviewed 0/2/4/8 control
ladder, authored thresholds, scenario hashes, settlement/promotion-site counts,
and maximum full-control score for every shipped map. Regenerate it with:

```sh
cargo run -p crownline_core --example generate_promotion_progression_probes
```

The integration test recreates governed and established positions using each
map's own terrain and edge geometry, verifies the reported score, enumerates the
same mandatory actions used by clients and future AI, and applies every unlocked
kind through the reducer. It also removes governance and transfers ownership to
prove future-batch relocking. The review and threshold decision are in
[`promotion-progression-review.md`](promotion-progression-review.md).

These are deterministic balance checks, not human reports from the `F8` capture
workflow. They answer the narrow progression and reachability questions without
claiming enjoyment, comprehension, match pacing, or first-player balance; those
remain open in Task 10.03.01.
