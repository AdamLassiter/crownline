# Bug 023: Menu hotkey controls fail to action anything

## Status

- [x] Done

## Resolution

- Local setup now presents visible pointer-and-keyboard controls for scenario
  selection, both player controllers, player names, side swapping, clock
  enablement, base time, increment, and starting the match.
- Every visible control and retained shortcut emits the same typed menu action.
  The original lifecycle hotkey system is gated while the menu owns input, so
  actions cannot be missed or applied twice.
- Letter and punctuation accelerators are deliberately suppressed while a text
  field owns focus; function and navigation accelerators remain available.
- Added a regression for `X`, `F2`, `C`, `-`, `+`, PageUp/PageDown, and F7/F8,
  including the editable-text focus boundary.

## Linked task and introducing commit

- [Task 07.03.01](../07-local-client/07.03-match-lifecycle/07.03.01-setup-end.md),
  commit `65f0226`, introduced the text-and-hotkey-driven setup screen.
- [Task 07.04.03](../07-local-client/07.04-menu-system/07.04.03-local-setup.md)
  replaces it with discoverable GUI controls and a shared action dispatcher.

## Reproduction

1. Open the game to the main menu
2. Press a hotkey - x, F2, C, +, -, etc.
3. Notice that nothing changes w.r.t. the chosen hotkey

## Expected behavior

Hotkeys should perform the actions for which they are described.

## Actual behavior

Hotkeys do nothing.

## Impact

The user is unable to select any game settings.

## Dependencies

- 07.03.01, 07.03.02, 13.02.03, 07.04.01, 07.04.02.

## Acceptance criteria

- Every setup action is available through a visible pointer-and-keyboard
  control.
- Every advertised accelerator works in its documented context and invokes the
  same action as its corresponding control.
- Text editing cannot accidentally trigger character-based setup actions.
