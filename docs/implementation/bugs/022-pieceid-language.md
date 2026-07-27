# Bug 022: PieceId used in UI language

## Status

- [x] Done

## Resolution

- Guided objectives now resolve every referenced piece against the live
  canonical state and describe its owner, kind, and current coordinate. If the
  piece has just left the board, the stage-start description is used without
  exposing its internal identity.
- Removed `PieceId(...)` formatting from the match and settlement panels,
  mandatory-promotion copy, transition history, and hover previews so the same
  implementation detail cannot leak through another player-facing surface.
- Added a regression that reproduces the first capture lesson and verifies both
  the initial `North Pawn at (4, 6)` description and an updated coordinate after
  that same piece moves.

## Linked task and introducing commit

- [Task 14.01.02](../14-guided-scenarios/14.01-framework/14.01.02-guidance-ui.md),
  commit `271564b`, introduced the generic guided objective formatter.

## Reproduction

1. Begin the first guided scenario
2. Inspect the help text for this scenario
3. Notice the help text desribes the objective as 'capture piece PieceId(1)'

## Expected behavior

Text should describe the piece type and currently occupied square (updated if the piece moves).

## Actual behavior

Text describes an internal PieceId.

## Impact

This does not successfully identify the objective to the user and leaves the guided sccenario with an unknown objective.

## Dependencies

- 14.01.02, 07.02.02.

## Acceptance criteria

- All piece descriptions in-game must use user-friendly notation when referring to specific pieces.
