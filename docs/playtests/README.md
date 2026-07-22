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
