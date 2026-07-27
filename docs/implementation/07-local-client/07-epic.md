# Epic 07: Interaction and local play

Deliver a complete desktop game client with previews, information panels, setup,
clocks, save/load, and a unified pointer-and-keyboard menu system.

## Status

- [x] Done

## Implementation notes

- Local select/reselect/cancel, reducer-confirmed Move and Hold commands, and visible keyboard board focus are complete in Task 07.01.01.
- Non-mutating consequence previews with explicit self-check, attack-line, governance, settlement-progress, capture, check, and promotion explanations are complete in Task 07.01.02.
- Focused promotion and produced-Pawn placement controls, forced queue resolution, and choice-time clock labels are complete in Task 07.01.03.
- Responsive, collapsible match, recent-history, and settlement information panels are complete in Task 07.02.01.
- Scenario-aware rules help, complete visual legends, and state-preserving context links are complete in Task 07.02.02.
- Keyboard-driven local setup, scenario switching, pause/settings, match controls, outcomes, and fresh rematches are complete in Task 07.03.01.
- Optional monotonic local clocks with bounded setup, pause semantics, exact expiration, and Move/Hold increment handling are complete in Task 07.03.02.
- Atomic platform-local save slots, validated canonical restoration, pending-choice recovery, and user-facing failure guidance are complete in Task 07.03.03.
- Story 07.04 replaces the remaining text-and-hotkey-driven setup, online,
  settings, persistence, and lifecycle surfaces with one native Bevy GUI menu
  system.
- Story 07.04 is complete with typed pointer/keyboard action parity,
  transactional settings, context-aware match and save controls, confirmed
  destructive actions, modal input ownership, and live minimum/desktop
  viewport evidence.
- Task 07.04.08 adds responsive paired controls, explicit read-only/editable
  surface colors, consistent exit actions, direct Quit, and sequential guided
  scenario unlocking.

## Stories

- [07.01 Board interaction](07.01-interaction/07.01-story.md)
- [07.02 Information and help](07.02-information/07.02-story.md)
- [07.03 Local match lifecycle](07.03-match-lifecycle/07.03-story.md)
- [07.04 Unified GUI menu system](07.04-menu-system/07.04-story.md)

## Dependencies

- Epics 04 and 06; scenario selector initially uses available Epic 05 assets.

## Acceptance criteria

- Two players can configure, play, save, load, and finish a complete match
  through visible pointer-and-keyboard controls without developer tools.
- UI exposes rule consequences without recommending a best move.
- All mandatory choices and terminal outcomes are recoverable after save/load.

## Cross-cutting concerns

- Input accessibility, state/UI separation, atomic saves, and clear destructive confirmations.
