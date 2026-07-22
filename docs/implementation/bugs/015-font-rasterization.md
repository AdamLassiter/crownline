# Bug 015: Rasterized fonts look pixelated at higher zoom levels

## Status

- [x] Done

## Linked tasks and introducing commits

- [Task 06.02.02](../06-rendering/06.02-camera/06.02.02-camera-controls.md), commit `f1dc1f2` added camera zoom without increasing the raster resolution of world-space text.

## Reproduction

1. Start a new match
2. Zoom in lots

## Expected behavior

The game maintains good aesthetics at any zoom level.

## Actual behavior

Fonts are rasterized at a default zoom level and appear pixelated and rough around the edges when zoomed in.

## Impact

Poor visual aesthetics dissuade the user.

## Resolution

- Added automatic camera-aware raster levels for every pixel-sized `Text2d` entity. World text uses 1x atlases at ordinary/zoomed-out scales, 2x below camera scale 1.0, and 4x below scale 0.5 through the maximum supported zoom at scale 0.25.
- Counter-scaled each text transform by the same multiplier, preserving its world-space dimensions, position, rotation hierarchy, and apparent zoom behavior while ensuring the glyph atlas is never magnified beyond its source resolution.
- Kept raster levels discrete and bounded to three cached sizes per base font size rather than generating a new atlas on every wheel step.
- Registration is automatic for existing and future pixel-sized world text, covering chess pieces, coordinates, terrain/feature/edge marks, overlays, transition notices and ghosts, board controls, promotion choices, and Pawn-placement choices. Screen-space Bevy UI text is unaffected.
- Added regressions proving full zoom-range pixel density, exact world-size preservation and restoration, bounded atlas multipliers, and automatic `Text2d` registration.

## Dependencies

- 06.01.02, 06.01.03, 06.02.02, 06.03.01, 06.03.02, 07.01.01, 07.01.03.

## Acceptance criteria

- No supported camera zoom magnifies world-text glyphs beyond their raster source resolution.
- World-space text dimensions and placement do not change when raster level changes.
- Raster size changes are bounded to reusable 1x, 2x, and 4x levels.
