# Epic 04: Realm systems and match flow

Layer settlements, governance, promotion, Hold, clocks, and terminal outcomes onto ordinary chess moves.

## Stories

- [04.01 Governance](04.01-governance/04.01-story.md)
- [04.02 Settlement lifecycle](04.02-settlements/04.02-story.md)
- [04.03 Promotion and turn phases](04.03-turn-phases/04.03-story.md)
- [04.04 Clocks and outcomes](04.04-outcomes/04.04-story.md)

## Dependencies

- Epic 03.

## Acceptance criteria

- Realm effects are automatic consequences of canonical actions.
- Timing is deterministic across local play, server execution, save/load, and replay.
- Match outcomes cover checkmate, timeout, resignation, accepted draw, and threefold repetition.

## Cross-cutting concerns

- Explainable transitions, lineage cleanup, clock authority, and complete-state repetition.

