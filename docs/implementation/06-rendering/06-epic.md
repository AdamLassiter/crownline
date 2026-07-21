# Epic 06: Board rendering

Render a scalable, readable 2D board using tinted checker tiles, Unicode chess glyphs, and code-native overlays.

## Status

- [x] Done

## Implementation notes

- The default authored scenario projects through stable coordinate transforms into accessible terrain tiles, Unicode pieces, sites, ownership cues, Keeps, barriers, labels, and revision-cached semantic overlays.
- Fit/pan/zoom controls, UI-aware picking, explicit precedence, textual overlay equivalents, and non-blocking presentation-only transitions complete the scalable 2D rendering boundary without changing canonical rules.

## Stories

- [06.01 Board and pieces](06.01-board-pieces/06.01-story.md)
- [06.02 Camera and coordinates](06.02-camera/06.02-story.md)
- [06.03 Semantic overlays and motion](06.03-overlays/06.03-story.md)

## Dependencies

- Epic 01, 02.01; piece rendering can begin before full rules completion.

## Acceptance criteria

- All three map sizes render accurately at arbitrary supported window sizes.
- Pieces, ownership, terrain, sites, and barriers are distinguishable without external sprite art.
- Rendering is a projection of canonical state and never changes rules.

## Cross-cutting concerns

- Accessibility, font licensing, layering, stable coordinates, and bounded entity churn.
