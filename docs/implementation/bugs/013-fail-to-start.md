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

- Enabled Bevy's `default_font` feature while retaining the project's explicit minimal feature set. Bevy now registers its embedded Fira Mono subset at the default font asset ID used by menu, panel, help, status, and coordinate text.
- Added a headless regression that constructs the asset and text plugins and proves the default font asset is actually registered; omitting the feature makes this test fail.
- Recorded the embedded font and its SIL Open Font License attribution in the notices shipped with desktop archives.

## Dependencies

- 07.02.01, 07.03.01, 11.02.01.

## Acceptance criteria

- Game startup shows either a menu or gameplay
- Startup UI text has a registered font and does not depend on system fonts.
- Desktop third-party notices identify the embedded default UI font and license.
