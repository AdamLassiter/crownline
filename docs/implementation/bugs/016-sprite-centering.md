# Bug 016: Piece glyphs are not centered properly

## Status

- [x] Done

## Linked tasks and introducing commits

- [Task 06.01.02](../06-rendering/06.01-board-pieces/06.01.02-pieces.md), commit `8249218` centered each chess glyph by its font layout box without compensating for the bundled font's asymmetric visual ink bounds.

## Reproduction

1. Start a new match
2. Examine the vertical alignment of pieces on their squares

## Expected behavior

Pieces are correctly horizontally and vertically centered.

## Actual behavior

Pieces are offset vertically, appearing 'higher' on their square than they should be.

## Impact

Poor visual aesthetics dissuade the user.

## Resolution

- Measured the actual ink bounding boxes of all six chess characters in the bundled `NotoSansSymbols2-Regular.ttf` at the 4x maximum-zoom raster size. Their visual centres sit 3.125-4.25 world pixels above the font layout centre, depending on piece kind.
- Added stable per-kind vertical corrections derived from those measured bounds: King -3.625 px, Queen/Rook -3.75 px, Bishop -3.125 px, Knight -4.25 px, and Pawn -4.125 px. Horizontal ink bounds are already symmetric and require no correction.
- Applied the same offsets to canonical piece presentations and short-lived capture/promotion retirement ghosts so transitions cannot jump vertically between differently aligned glyphs.
- Kept the offset in world units before Bug 015's raster counter-scaling, preserving the same visual centering at every supported camera zoom.
- Added regression coverage for every piece kind and the bounded compensation range.

## Dependencies

- 01.03.01, 06.01.02, 06.03.02, Bug 015.

## Acceptance criteria

- Pieces are visually centered on their squares, incorporating a small offset factor to make up for font glyph alignment if required.
- Every piece kind uses its measured bundled-font ink correction.
- Retirement ghosts use the same correction as live pieces.
- Live graphical verification confirms every starting piece is visually centered at maximum zoom.
