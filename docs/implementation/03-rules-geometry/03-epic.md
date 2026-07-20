# Epic 03: Chess rules and terrain geometry

Implement legal chess movement on Crownlines' larger, terrain-shaped boards while preserving check, pins, and deterministic attack semantics.

## Stories

- [03.01 Movement and attacks](03.01-movement/03.01-story.md)
- [03.02 Terrain and fortifications](03.02-terrain/03.02-story.md)
- [03.03 Royal safety and optional rules](03.03-royal-rules/03.03-story.md)

## Dependencies

- Epic 02.

## Acceptance criteria

- Every standard piece moves and captures correctly on arbitrary supported boards.
- Terrain changes geometry without adding combat statistics or randomness.
- Legal actions never leave the acting player's King attacked.

## Cross-cutting concerns

- Determinism, performance on 24x24 maps, explainable illegality, and property testing.

