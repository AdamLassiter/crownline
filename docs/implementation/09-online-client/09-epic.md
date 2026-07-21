# Epic 09: Online client

Integrate private-room networking into the Bevy client while keeping server snapshots authoritative.

## Status

- [x] Done

## Implementation notes

- Task 09.01.01 adds the Bevy online lobby and bounded create/join HTTP boundary with authored scenario/clock selection, safe server errors, TLS policy, normalized room codes, and credential-free invitations.
- Story 09.01 completes private room connection and recovery with OS-backed credential storage, a documented user-only fallback, authenticated restart restoration, bounded jittered reconnect, explicit connection status/recovery controls, and authoritative snapshot adoption.
- Task 09.02.01 adds server-authoritative board submission with one locked pending intent and same-key idempotent retries; canonical board state remains unchanged until an authoritative response arrives.
- Story 09.02 completes monotonic authoritative synchronization: validated newer snapshots replace canonical resources, older/equal state cannot roll the board back, divergence forces authenticated resync, and ECS projections rebuild without deserializing network entities.
- Task 09.03.01 adds a server-anchored presentation clock and explicit online seat, phase, pending-command, transport, and opponent status without granting the client clock or outcome authority.
- Story 09.03 completes server-authoritative draw, resignation, terminal, leave, and rematch UX. Lifecycle controls use their specified protocol envelopes, finished results remain durable, and two accepted seats transition to a fresh persisted match identity and canonical state.

## Stories

- [09.01 Room connection flow](09.01-connection/09.01-story.md)
- [09.02 Command synchronization](09.02-sync/09.02-story.md)
- [09.03 Online match lifecycle](09.03-lifecycle/09.03-story.md)

## Dependencies

- Epics 07 and 08.

## Acceptance criteria

- Players can create/join, reconnect automatically, and play through the same board UI as local mode.
- Stale/rejected commands reconcile without corrupting canonical or presentation state.
- Online terminal, draw, resign, and rematch flows are complete.

## Cross-cutting concerns

- Secret storage, clear connection state, retry bounds, responsiveness, and server authority.
