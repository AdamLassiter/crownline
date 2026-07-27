# Bug 022: PieceId used in UI language

## Status

- [x] Done

## Resolution

- Guided objectives now resolve every referenced piece against the live
  canonical state and describe its owner, kind, and current chess-notation
  square. If the piece has just left the board, the stage-start description is
  used without exposing its internal identity.
- Removed `PieceId(...)` formatting from the match and settlement panels,
  mandatory-promotion copy, transition history, and hover previews so the same
  implementation detail cannot leak through another player-facing surface.
- Follow-up implementation replaces Cartesian `(x, y)` copy across these
  surfaces and the authored guidance catalogue with the shared canonical board
  formatter (`a1`, `b12`, and so on). Labels describe the same square regardless
  of camera orientation.
- Added a regression that reproduces the first capture lesson and verifies both
  the initial `North Pawn at e7` description and an updated `f7` square after
  that same piece moves.

## Linked task and introducing commit

- [Task 14.01.02](../14-guided-scenarios/14.01-framework/14.01.02-guidance-ui.md),
  commit `271564b`, introduced the generic guided objective formatter.

## Reproduction

1. Begin the first guided scenario
2. Inspect the help text for this scenario
3. Notice the help text desribes the objective as 'capture piece PieceId(1)'

## Expected behavior

Text should describe the piece type and currently occupied square in canonical
chess notation (updated if the piece moves).

## Actual behavior

Text describes an internal PieceId.

## Impact

This does not successfully identify the objective to the user and leaves the guided sccenario with an unknown objective.

## Dependencies

- 14.01.02, 07.02.02.

## Acceptance criteria

- All piece descriptions in-game must use user-friendly chess notation when
  referring to specific pieces.
