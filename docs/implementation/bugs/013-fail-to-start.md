# Bug 013: Game fails to start

## Status

- [x] Done

## Linked tasks and introducing commits

- [Task 07.02.01](../07-local-client/07.02-information/07.02.01-panels.md), commit `9604a1f` first introduced UI text using Bevy's empty default font handle.
- [Task 07.03.01](../07-local-client/07.03-match-lifecycle/07.03.01-setup-end.md), commit `65f0226` placed that invisible text inside the opaque startup modal, turning the defect into a startup blocker.

## Reproduction

Start the game with `cargo run`.

## Expected behavior

The game starts up and presents a menu or gameplay.

## Actual behavior

The game starts up with a blank window, with only a single dark rectangle drawn to the screen.

## Impact

The game cannot even be tested in this state, let alone released and played.

## Resolution

- The first fix in commit `0bf219c` was insufficient: Bevy's `2d` and `ui` feature groups already enabled the default font transitively, so making it explicit did not change rendering. The font invariant test and required Fira Mono notice remain valid, but they did not resolve the blank frame.
- Reproduced the exact frame on the live X11 session and inspected runtime layout and visibility. Text shaped hundreds of glyphs correctly and 529 sprites existed, but `sync_lifecycle_ui` had set every entity with a `Visibility` component to `Hidden`; only the marked setup modal root was restored to visible, leaving its children and the board hidden.
- Restricted lifecycle visibility mutation to entities marked `SetupRoot`, `PauseRoot`, or `OutcomeRoot`. Board sprites, pieces, panel text, controls, and modal children now retain their own visibility state.
- Strengthened the startup regression to prove the setup text remains inherited/visible at the component boundary and that lifecycle synchronization cannot alter an unrelated visible entity.
- Launched the fixed executable on the live X11 display and captured the resulting frame: the board, pieces, information panels, setup instructions, name fields, and help control all render.

## Dependencies

- 07.02.01, 07.03.01, 11.02.01.

## Acceptance criteria

- Game startup shows either a menu or gameplay.
- Startup UI text has a registered font and does not depend on system fonts.
- Desktop third-party notices identify the embedded default UI font and license.
