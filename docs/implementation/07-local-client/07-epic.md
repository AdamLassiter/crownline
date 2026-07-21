# Epic 07: Interaction and local play

Deliver a complete desktop hot-seat game with previews, information panels, setup, clocks, and save/load.

## Status

- [ ] In progress

## Implementation notes

- Local select/reselect/cancel, reducer-confirmed Move and Hold commands, and visible keyboard board focus are complete in Task 07.01.01.
- Non-mutating consequence previews with explicit self-check, attack-line, governance, settlement-progress, capture, check, and promotion explanations are complete in Task 07.01.02.
- Focused promotion and produced-Pawn placement controls, forced queue resolution, and choice-time clock labels are complete in Task 07.01.03.
- Responsive, collapsible match, recent-history, and settlement information panels are complete in Task 07.02.01.
- Scenario-aware rules help, complete visual legends, and state-preserving context links are complete in Task 07.02.02.

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
