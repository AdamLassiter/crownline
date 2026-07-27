# Scenario authoring guide

Crownlines scenarios are versioned RON `ScenarioDefinition` documents. Authors
should begin from one of `assets/scenarios/*.ron`, preserve deterministic IDs and
coordinate ordering, and run `./scripts/check.sh` after every change.

## Optional fog of war

Fog is disabled when `rules.fog` is omitted or written as `None`. Enabled fog
uses an independently versioned block:

```ron
fog: Some((schema_version: 1, vision_radius: 3)),
```

Radius 3 is the initial tuning baseline. The radius uses Chebyshev distance,
terrain and edges deliberately do not block vision, and validation bounds it to
`1..=max(board.width, board.height) - 1`. See the complete deterministic
[fog-of-war contract](fog-of-war-contract.md) before authoring a variant. No
shipped scenario should enable the block until the seat-state, client, online,
privacy, and validation tasks in Story 12.01 are complete.

## Promotion progression

`rules.promotion_unlocks` authors the cumulative control scores for Bishop, Rook,
and Queen. Knight is always available and has no authored threshold.

```ron
promotion_unlocks: (bishop: 2, rook: 4, queen: 8),
```

The values must be positive and nondecreasing. Current scenario schema 2 files
should write them explicitly. Schema 1 or omitted data receives the compatibility
default `(2, 4, 8)`; omission is not a recommendation for new content.

Control is calculated for the promoting player at the owner-turn boundary:

```text
score = owned settlements
      + governed owned settlements
      + 2 * established owned settlements
```

Settlement transfers and completed cycles resolve before this calculation. All
promotions becoming ready at that boundary share one frozen snapshot. Control
lost later affects future batches only.

Before tuning, verify that the map can reach each configured tier through normal
settlement ownership and governance, and that the maximum is not effectively
equivalent to total map conquest. Under 2/4/8, one claimed and governed
settlement unlocks Bishop, one fully established governed settlement unlocks
Rook, and two such settlements unlock Queen. Use scenario data, not client code
or special-case reducer branches, for variants.

Regenerate and review deterministic promotion evidence with:

```sh
cargo run -p crownline_core --example generate_promotion_progression_probes
./scripts/check.sh
```

If thresholds change, archive the before/after scenario hashes, state the
balance hypothesis, and obtain a new side-swapped human pair under
[`playtesting.md`](playtesting.md) before claiming subjective improvement.

## Compatibility and validation

Increasing scenario schema or changing authored rules changes the scenario
canonical hash and may make active online matches unrecoverable unless an
explicit migration exists. Update compatibility documentation and golden
fixtures deliberately. Validation rejects malformed boards, duplicate IDs,
out-of-range timing, invalid unlock ordering, inaccessible rule metadata, and
other aggregate errors before match construction. The optional fog block has
its own schema version; absent or explicitly disabled fog is omitted from
canonical serialization and leaves existing scenario hashes unchanged.
