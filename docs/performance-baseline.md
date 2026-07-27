# Performance baseline

This baseline covers the maximum shipped 24x24 scenario with deterministic opening, redistributed dense-midgame, and sparse-endgame states. Run it with:

```sh
./scripts/benchmark.sh
```

The command uses an optimized release build, one test thread, fixed iteration counts, and `std::hint::black_box`. It is a regression and workload benchmark, not a substitute for frame-time capture on every supported GPU and display.

## Regression budgets

The scheduled benchmark fails when a release-profile mean exceeds:

| Operation | Maximum mean |
| --- | ---: |
| Move generation | 10 ms |
| Attack and governance maps | 10 ms |
| Complete selected-piece hover preview and semantic overlay model | 25 ms |
| Canonical hash | 5 ms |
| Canonical JSON serialization | 5 ms |
| Canonical snapshot clone/projection boundary | 5 ms |
| Unchanged revision-cached Bevy update | 5 ms |
| Revision-invalidated Bevy update | 30 ms |
| Both fog visibility masks | 5 ms |
| Both authenticated seat projections | 5 ms |
| Combined reconnect projection JSON | 256 KiB |

These ceilings are intentionally above the initial machine's means so normal hosted-runner variance does not create noise. Changes that approach a ceiling require profiling and an explicit baseline note rather than silently raising the threshold.

## Initial local result

Recorded 2026-07-22 from the worktree based on `7320c76`, Rust 1.95.0, Linux x86-64, and an AMD Ryzen 9 7950X3D environment exposing 16 logical CPUs and 15 GiB RAM. Values are mean microseconds per operation:

| Workload | Pieces | JSON bytes | Moves | Attack/governance | Overlay preview | Hash | Serialize | Projection |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Opening | 32 | 5,099 | 75.36 | 111.60 | 12,307.37 | 6.16 | 4.25 | 0.18 |
| Dense midgame | 32 | 8,316 | 579.26 | 139.53 | 17,537.07 | 9.32 | 5.94 | 0.90 |
| Sparse endgame | 12 | 6,254 | 99.20 | 97.78 | 10,178.26 | 6.41 | 3.98 | 0.81 |

On the 24x24 opening state, an unchanged cache-hit Bevy update averaged 252.12 microseconds and a forced canonical-revision invalidation averaged 275.57 microseconds. Both include the headless rendering schedules; they do not include GPU presentation.

### Fog-of-war extension

Recorded 2026-07-27 on the same AMD Ryzen 9 7950X3D class environment after
adding protocol-3 seat views. Radius 3 is applied to the unchanged 24x24 map;
payload bytes are the combined North and South `PlayerView` JSON sizes:

| Workload | Pieces | Both visibility masks | Both projections | Combined JSON |
| --- | ---: | ---: | ---: | ---: |
| Opening | 32 | 22.45 us | 124.39 us | 25,880 B |
| Dense midgame | 32 | 22.38 us | 298.38 us | 83,339 B |
| Sparse endgame | 12 | 14.78 us | 220.00 us | 78,146 B |

The same run measured the expanded headless Bevy schedules at 697.12 us for an
unchanged update and 770.53 us for revision invalidation, both below their
existing budgets. The projection workload includes projection hashing. Network
envelopes add small fixed metadata beyond the recorded `PlayerView` bytes and
remain below the 256 KiB combined reconnect ceiling.

## Allocation and invalidation interpretation

- Move and attack queries allocate stable result vectors. Governance adds per-settlement paths; its cost scales with major pieces and sites rather than all 576 board squares.
- Overlay preview is the dominant measured path. It allocates ordered coordinate/kind maps, text lines, attack-set differences, governance maps before and after the preview, and a complete reducer transition. This is intentionally uncached while the hovered destination changes.
- Canonical JSON serialization allocates one output byte vector. Hashing currently serializes the canonical state before SHA-256, so it also pays a temporary serialization allocation. The emitted JSON-byte column makes state-size growth visible.
- Snapshot projection clones ordered piece, settlement, repetition, choice, and rights collections. It remains small here, but is expected to grow with journal-independent canonical state size.
- `OverlayCacheKey` contains scenario ID, canonical revision, selected piece, and hovered coordinate. Unchanged updates reuse the existing model; any key change rebuilds semantic maps and presentation entities. No cache survives a canonical revision without that full key proof.

The benchmark output must be reviewed alongside golden replay and rules tests. Performance work may change implementation strategy, but it must not change canonical hashes or transition semantics.
