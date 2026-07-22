# Bug 014: Chess font fallback rewrites unrelated visibility

## Status

- [x] Done

## Linked tasks and introducing commits

- [Task 01.03.01](../01-foundation/01.03-runtime-assets/01.03.01-font.md), commit `c9246f0` added the fallback visibility loop without restricting its query to font-related entities.

## Reproduction

1. Make `assets/fonts/NotoSansSymbols2-Regular.ttf` unavailable or invalid.
2. Start the client and allow `monitor_chess_font` to observe the failed asset.
3. Inspect lifecycle, lobby, help, panel, overlay, and board entity visibility.

## Expected behavior

The Unicode chess glyph entities become hidden and the readable missing-font message becomes visible. All unrelated entities retain the visibility chosen by their owning systems.

## Actual behavior

The fallback system queried every entity with a `Visibility` component. It set unrelated entities to `Inherited`, potentially exposing hidden modal/lobby/help state and overriding intentional board or control visibility every frame after the asset failure.

## Impact

The recovery path for a missing chess font could corrupt the entire presentation state instead of showing one bounded diagnostic.

## Resolution

- Restricted the font-failure query to entities marked `ChessFontText` or `FontFallbackText`.
- Kept fallback activation idempotent while preventing it from mutating any unrelated visibility component.
- Extended the fallback regression with chess, fallback, and unrelated entities; the first two switch as intended while the unrelated hidden entity remains hidden.

## Dependencies

- 01.03.01, Bug 013.

## Acceptance criteria

- Font failure hides Unicode chess text and shows the readable fallback message.
- Unrelated entity visibility is unchanged.
- The behavior is covered without requiring an actual corrupt runtime asset.
