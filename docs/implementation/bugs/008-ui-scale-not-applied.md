# Bug 008: Configured UI scale is not applied

## Status

- [x] Done

## Linked task and introducing commit

- Task: [01.03.02 Establish configuration and tracing](../01-foundation/01.03-runtime-assets/01.03.02-config-tracing.md)
- Introduced by: `211c5e2` (`feat(runtime): add configuration and tracing [Task 01.03.02]`)
- Exposed by: [10.03.02 Audit readability and accessibility](../10-quality/10.03-playtesting/10.03.02-accessibility.md)

## Reproduction

Set `ui_scale` to any valid non-default value in `settings.ron`, start the client, and inspect the Bevy UI layout resource or compare panel/text sizing.

## Expected behavior

The validated 0.75-2.5 setting becomes Bevy's global `UiScale`, consistently scaling UI node pixel dimensions and text while leaving the world-space board and camera independent.

## Actual behavior

The client loaded and retained `ui_scale`, but startup never inserted it into Bevy. Every UI surface therefore rendered at Bevy's default scale of 1.0.

## Impact

Players could not enlarge interface text and controls or reduce UI footprint even though the configuration claimed to support it. Accessibility testing at configured scales would have tested the wrong layout.

## Resolution

Install the configured value as Bevy's `UiScale` before plugins initialize. A regression covers every documented boundary and representative intermediate scale.

## Dependencies

- Task 01.03.02.

## Acceptance criteria

- Every valid configured scale reaches Bevy's UI layout resource unchanged.
- World-space board geometry and camera scaling remain separate.
- Invalid scales retain the existing field-specific configuration error and fallback behavior.
