# Epic 10: Quality, balance, and performance

Prove rules correctness, online durability, UI readability, map balance, and maximum-board responsiveness before release.

## Status

- [ ] In progress

## Implementation notes

- Task 10.01.01 establishes the core rules verification layer with readable fixtures, interaction regressions, generated King-safety walks, transactional rejection checks, and canonical serialization/hash invariants.
- Story 10.01 completes rules verification with versioned golden journals for every shipped scenario and terminal reason, including a combined realm transition path and per-revision event/hash enforcement.
- Task 10.02.01 adds real-loopback HTTP/WebSocket integration over isolated durable SQLite, proving the two-client lifecycle, one-winner concurrency, idempotency, credential recovery, persisted deadlines, terminal/rematch behavior, and secret-safe responses.
- Story 10.02 completes integration verification with headless Bevy projection/input/reconciliation tests across authored board sizes and common resolutions, including exact reconnect reconstruction for every canonical turn surface.
- Task 10.04.01 establishes release-profile 24x24 baselines for core queries, previews, canonical data work, projection, and Bevy cache invalidation, with documented allocation sources and scheduled regression ceilings.

## Stories

- [10.01 Rules verification](10.01-rules-tests/10.01-story.md)
- [10.02 Server and client integration](10.02-integration/10.02-story.md)
- [10.03 Playtesting and accessibility](10.03-playtesting/10.03-story.md)
- [10.04 Performance and soak testing](10.04-performance/10.04-story.md)

## Dependencies

- Testing grows with every epic; final completion depends on Epics 05-09.

## Acceptance criteria

- Automated suites cover rule interactions, persistence, reconnect, concurrency, and platform builds.
- Structured playtests answer the GDD prototype questions with recorded evidence.
- The 24x24 scenario meets agreed responsiveness and stability budgets.

## Cross-cutting concerns

- Reproducible fixtures, representative hardware, privacy-preserving metrics, and regression ownership.
