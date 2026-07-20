# Cross-cutting concerns

Every epic must account for the applicable concerns below.

## Determinism and authority

- `crownline_core` is the sole rules authority and must not depend on Bevy, wall-clock time, networking, or filesystem state.
- Given the same scenario, state, and ordered actions, all platforms must produce the same state and canonical hash.
- Online clients present and predict only; the server validates actions and owns outcomes and clocks.

## Compatibility and serialization

- Scenarios, saves, protocol messages, snapshots, and journals carry explicit schema/protocol versions.
- Readers reject unsupported future versions with actionable errors; migrations are explicit and tested.
- Coordinates, iteration order, hashes, and queued choices must never depend on hash-map iteration order.

## Security and privacy

- Treat all network input as hostile: bound message sizes, validate identifiers, rate-limit room operations, and never trust client time or state.
- Room codes identify rooms but grant no seat authority; reconnect tokens are high entropy and stored hashed.
- Do not collect telemetry or personal data in the initial release. Logs must not contain reconnect tokens or complete credentials.

## Accessibility and readability

- Never communicate ownership, terrain, legality, or match state through color alone.
- Support scalable text, keyboard navigation for menus, remappable board-navigation controls, and color-vision-safe palettes.
- Animation must be brief, optional, and must not delay or obscure a deterministic result.

## Performance

- Rules queries must be responsive on a 24x24 board; avoid per-frame rule recomputation when state and selection have not changed.
- Network snapshots and database transactions are bounded and observable.
- Optimize only after representative profiling; retain benchmark baselines for move generation and full-state serialization.

## Reliability and observability

- Errors shown to players are actionable and do not expose internals; logs retain structured technical context.
- Local saves use atomic replacement. Server state updates and action-journal entries commit in one transaction.
- Server shutdown is graceful, unfinished matches restore after restart, and health checks distinguish process health from database readiness.

## Testing

- Pure rules receive unit, table-driven, property, and deterministic replay tests.
- Protocol/server work receives integration tests using two independent clients.
- Bevy presentation logic is separated from rules so coordinate mapping and UI state can be tested headlessly.
- CI runs formatting, Clippy with warnings denied, tests, scenario validation, and platform builds.

## Licensing and distribution

- Every bundled asset includes a compatible license and attribution where required.
- Desktop archives include notices, version information, and save/protocol compatibility notes.
- The server container runs as a non-root user and stores SQLite data only in its configured volume.

