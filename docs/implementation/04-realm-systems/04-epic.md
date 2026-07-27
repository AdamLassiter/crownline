# Epic 04: Realm systems and match flow

Layer settlements, governance, promotion, Hold, clocks, and terminal outcomes onto ordinary chess moves.

## Status

- [x] Done

## Implementation notes

- Canonical actions now resolve governance, settlement claim/transfer/development/production, promotion, stable turn-start choices, and Move/Hold through one deterministic reducer.
- Optional host-driven clocks, timed journals, full-state repetition, draw/resignation controls, and immutable typed outcomes complete the match-flow layer without adding wall-clock or Bevy dependencies to core rules.
- Stories 04.01-04.05 are complete. Stronger promotion recruits now depend on versioned, scenario-authored current realm control and preserve one frozen batch snapshot across every execution boundary.

## Stories

- [04.01 Governance](04.01-governance/04.01-story.md)
- [04.02 Settlement lifecycle](04.02-settlements/04.02-story.md)
- [04.03 Promotion and turn phases](04.03-turn-phases/04.03-story.md)
- [04.04 Clocks and outcomes](04.04-outcomes/04.04-story.md)
- [04.05 Promotion recruitment progression](04.05-promotion-recruitment-progression/04.05-story.md)

## Dependencies

- Epic 03.

## Acceptance criteria

- Realm effects are automatic consequences of canonical actions.
- Timing is deterministic across local play, server execution, save/load, and replay.
- Match outcomes cover checkmate, timeout, resignation, accepted draw, and threefold repetition.
- Promotion recruitment progresses from Knights through Bishops and Rooks to Queens as current settlement control grows.

## Cross-cutting concerns

- Explainable transitions, lineage cleanup, clock authority, complete-state repetition, and scenario-authored promotion balance.
