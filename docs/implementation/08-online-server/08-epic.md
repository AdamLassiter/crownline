# Epic 08: Online protocol and authoritative server

Host private-room matches with server-authoritative rules, clocks, durable state, and secure reconnection.

## Status

- [ ] In progress

## Implementation notes

- Task 08.01.01 establishes the bounded, versioned room and match wire-message contract with typed validation and stable public errors.
- Task 08.01.02 completes the synchronization contract with authoritative snapshots, revision checks, and bounded idempotent mutation results.
- Tasks 08.02.01-08.02.02 provide the private-room lifecycle and hashed, redacted, rate-limited reconnect credentials for seat authority.
- Task 08.02.03 completes bounded room operation with configurable request/connection limits, observable throttling, and cleanup that preserves every started match.
- Task 08.03.01 adds bounded per-match command actors with total ordering, revisioned results, backpressure, isolated durable recovery, and idle unload.

## Stories

- [08.01 Versioned protocol](08.01-protocol/08.01-story.md)
- [08.02 Rooms and seat authority](08.02-rooms/08.02-story.md)
- [08.03 Authoritative match hosting](08.03-authority/08.03-story.md)
- [08.04 Persistence and container operations](08.04-operations/08.04-story.md)

## Dependencies

- Epics 02-05; foundation server shell from Epic 01.

## Acceptance criteria

- Two accountless clients can create, join, play, disconnect, reconnect, and finish a private match.
- Every accepted action is authorized, validated, journaled, snapshotted, and broadcast atomically.
- Unfinished matches recover after server restart with authoritative clocks.

## Cross-cutting concerns

- Hostile-input handling, credential secrecy, idempotency, transactionality, rate limits, and observability.
