# Epic 07: Interaction and local play

Deliver a complete desktop hot-seat game with previews, information panels, setup, clocks, and save/load.

## Stories

- [07.01 Board interaction](07.01-interaction/07.01-story.md)
- [07.02 Information and help](07.02-information/07.02-story.md)
- [07.03 Local match lifecycle](07.03-match-lifecycle/07.03-story.md)

## Dependencies

- Epics 04 and 06; scenario selector initially uses available Epic 05 assets.

## Acceptance criteria

- Two players can configure, play, save, load, and finish a complete match without developer tools.
- UI exposes rule consequences without recommending a best move.
- All mandatory choices and terminal outcomes are recoverable after save/load.

## Cross-cutting concerns

- Input accessibility, state/UI separation, atomic saves, and clear destructive confirmations.

