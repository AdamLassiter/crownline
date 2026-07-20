# Epic 02: Core domain and persistence

Create the deterministic, serializable vocabulary and state transition boundary shared by local and online games.

## Status

- [x] Done

## Implementation notes

- The core crate now owns validated scenarios, canonical match state, typed deterministic action transitions, versioned save/snapshot envelopes, and bounded replayable action journals.
- Identity allocation, ordered collections, canonical hashes, explicit migrations, integrity checks, and idempotent replay form the shared local/server authority boundary.

## Stories

- [02.01 Scenario model](02.01-scenarios/02.01-story.md)
- [02.02 Match state and actions](02.02-match-state/02.02-story.md)
- [02.03 Versioned persistence and replay](02.03-persistence/02.03-story.md)

## Dependencies

- Epic 01.

## Acceptance criteria

- Core state round-trips deterministically and has a stable canonical hash.
- Invalid scenarios and actions fail with typed, actionable errors.
- Saves and action journals are versioned before gameplay features depend upon them.

## Cross-cutting concerns

- Determinism, compatibility, bounded data, and migration safety.
