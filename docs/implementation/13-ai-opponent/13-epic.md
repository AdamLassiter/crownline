# Epic 13: AI opponent

Provide a modest, deterministic opponent built around extensible adversarial search rather than attempting a full-strength chess engine.

## Status

- [ ] Not started

## Stories

- [13.01 Search engine](13.01-search/13.01-story.md)
- [13.02 Evaluation, difficulty, and client integration](13.02-opponent/13.02-story.md)

## Dependencies

- Epics 02-05 and 10.01. Local presentation depends on Epic 07.

## Acceptance criteria

- The initial engine uses iterative alpha-beta minimax, bounded quiescence search, deterministic move ordering, and a rule-aware heuristic.
- Search handles Move, Hold, promotion, and produced-Pawn placement without assuming that every reducer action changes the active player.
- Difficulty comes from named, testable search/evaluation budgets rather than hidden rule advantages or claims of chess-engine strength.
- Search, evaluation, ordering, and limits are replaceable behind narrow interfaces so later transposition tables, parallel search, or other strategies do not require client rewrites.
- AI computation never blocks Bevy's frame loop or becomes a second rules authority.

## Cross-cutting concerns

- Determinism, cancellation, bounded time/nodes/memory, replayable decisions, exact terminal scoring, and no gameplay dependency from `crownline_core` on AI code.
- The first implementation is perfect-information only. Fog-aware belief/search behavior requires an explicit later design and must not inspect hidden canonical state while pretending to use a seat view.
