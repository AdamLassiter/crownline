# Server soak baseline

The bounded server soak exercises the real loopback HTTP/WebSocket service and a file-backed SQLite WAL database. Run it with:

```sh
./scripts/soak.sh
```

The command uses an optimized release build, one test thread, temporary databases, and synthetic player names and reconnect tokens. It binds only to `127.0.0.1` and has no configuration for targeting a deployed service.

## Workload

Each run performs the following lifecycle:

- Creates eight independently clocked rooms through HTTP, joins a second player, authenticates both seats over WebSocket, and starts each match.
- Applies actions, disconnects and reauthenticates one seat in every room, leaves active and resigned matches, rejects an invalid token, and recovers after malformed input.
- Leaves one additional subscriber unread while 40 draw offer/reject revisions are committed, crossing the room event broadcast capacity of 32, then verifies that subscriber reaches the exact latest snapshot.
- Queries the live WAL database, creates an online `VACUUM INTO` backup, runs SQLite integrity checking, and relates match snapshot rows to persisted actions.
- Stops the server after dropping every client, checks process RSS against a 64 MiB recovery allowance, starts a new server from the backup, reauthenticates an active match, and verifies its revision, stored hash, and newly computed canonical hash.

The soak is deliberately bounded enough for a scheduled CI runner. It complements, rather than replaces, longer deployment monitoring.

## Initial local result

Recorded 2026-07-22 from the worktree based on `86994d5`, Rust 1.95.0, Linux x86-64, and an AMD Ryzen 9 7950X3D environment exposing 16 logical CPUs and 15 GiB RAM:

| Metric | Result |
| --- | ---: |
| Rooms | 8 |
| Persisted actions | 59 |
| Persisted snapshots | 67 |
| Backup size | 978,944 bytes |
| Live action-count query | 140 microseconds |
| Online backup and integrity check | 5 ms |
| RSS before server start | 4,784 KiB |
| RSS after client cleanup and server stop | 15,432 KiB |

Snapshot growth is one initial row per match plus one row per accepted action, so this run requires exactly `8 + 59 = 67` snapshots. The backup size is checked against a documented loose bound of 256 KiB plus 64 KiB per action; the measured database is well below it. Query and backup timings are reported as evidence rather than enforced latency ceilings because hosted runner storage varies substantially.

## Queue and lifecycle bounds

- Canonical room snapshots use a Tokio `watch` channel, which retains one latest value rather than a per-client history. Room events use a broadcast channel with capacity 32; lag is bounded and snapshot catch-up is authoritative.
- Each match actor has a bounded command channel with default capacity 64. WebSocket message size/rate and global/per-IP connection limits are enforced by `ServerLimits`.
- The soak crosses the event window without reading one client and proves both continued publisher progress and exact latest-revision recovery. It does not infer queue memory from RSS alone.
- The harness drops every socket and awaits the aborted top-level server task before its RSS sample. Focused `ConnectionRegistry` and match-actor idle-unload tests remain the direct assertions that connection slots and actor tasks return to baseline; the scheduled soak runs after the complete quality job containing those tests.

Investigate a sustained increase in RSS, database bytes per action, or backup/query time before changing these bounds. Never point this harness at production data or credentials.
