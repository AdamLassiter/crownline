# Bug 017: UI placement is inconsistent and overlaps other elements

## Status

- [x] Done

## Linked tasks and introducing commits

- [Task 06.02.02](../06-rendering/06.02-camera/06.02.02-camera-controls.md), commit `f1dc1f2`, fitted the board against the whole window rather than a UI-free board viewport.
- [Task 07.01.03](../07-local-client/07.01-interaction/07.01.03-choices.md), commit `8d9f161`, presented promotion guidance as world-space text below the board.
- [Task 07.02.01](../07-local-client/07.02-information/07.02.01-panels.md), commit `9604a1f`, independently overlaid both side panels without reserving board space; its implicit opposite insets also placed both panels at the left edge.
- [Task 07.02.02](../07-local-client/07.02-information/07.02.02-help.md), commit `8f49126`, independently overlaid the help control at the bottom of the window.

## Reproduction

1. Start a new match
2. Examine the UI
    1. Promote a piece and examine the promotion choice UI
    2. Open/close the Match/Settlement menus and examine the floating UI components

## Expected behavior

UI components do not overlap, and have a clear space they own.

## Actual behavior

UI elements often overlap one another and can completely occlude one another.

## Impact

Poor visual aesthetics dissuade the user.

## Resolution

- Defined one shared responsive screen contract: 22% for each side panel, the central 56% for the board, and the bottom 20% for help, mandatory choices, connection state, and move feedback.
- Added a board-only camera with a physical viewport matching the central board region and a separate full-window UI camera. Camera fitting, panning, zooming, pointer projection, and raster scaling now explicitly select the board camera, avoiding ambiguity once two cameras exist.
- Kept the board fit inside 90% of its dedicated viewport, leaving every rank and file label visible around the board at the initial fit.
- Made opposing panel insets explicit: Match is pinned left with an automatic right inset, while Settlements is pinned right with an automatic left inset. Collapsing either body retains its owned side region instead of allowing another surface to occupy it.
- Converted interaction help and mandatory-promotion guidance from world text to a centered screen-space bottom log. Promotion no longer spawns a second glyph strip beneath the board. The same bounded log includes the latest transition feedback while the complete ordered history remains in the Match panel, preventing separate feedback and choice surfaces from colliding.
- Assigned online match/connection state to the lower-left region and lifecycle controls to the lower-right region so online-only surfaces follow the same layout contract.
- Added regressions for physical viewport allocation, explicit opposing panel edges, screen-space interaction/transition text, and the existing supported viewport fit matrix.
- Verified the result in a live X11 local match at 1904x1000: Match owns the left, Settlements owns the right, the complete 20x20 board and all coordinate labels remain inside the centre, and help/interaction guidance owns the bottom without overlap.

## Dependencies

- 06.02.02, 07.01.03, 07.02.01, 07.02.02, 09.03.01, Bug 008, Bug 015.

## Acceptance criteria

- The settlements UI occupies its own space on the RHS of the screen
- The match UI occupies its own space on the LHS of the screen
- The help, mandatory choices and move feedback 'log lines' UI occupies its own space on the bottom of the screen
- The board owns the rest of the screen, and the rank and file labels are always visible along the edge of the board viewport
- Panel collapse state does not change the region assigned to the board or another UI surface.
- Pointer-to-board projection uses only the board camera and rejects positions outside its viewport.
- Mandatory-choice and transition feedback are screen-space UI in the bottom region rather than world-space text below or over the board.
