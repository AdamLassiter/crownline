# Epic 14: Guided scenarios and challenges

Create progressively harder authored experiences that teach Crownlines mechanics and later test mastery through compact challenge puzzles.

## Status

- [ ] Not started

## Stories

- [14.01 Guided-play framework](14.01-framework/14.01-story.md)
- [14.02 Tutorial and challenge packs](14.02-scenario-packs/14.02-story.md)

## Dependencies

- Epics 02-07, 10, and 13. Perfect-information guided content does not depend on Epic 12.

## Acceptance criteria

- Tutorials introduce rules in a deliberate sequence with observable learning objectives, contextual explanations, and no hidden rule exceptions.
- Challenges use versioned goal/position data, deterministic validation, and an appropriate AI profile or authored defense tree.
- Difficulty labels are supported by solver metrics and human completion evidence rather than map size alone.
- Guided content reuses canonical rules/reducer, rendering, accessibility, save compatibility, and AI boundaries.

## Cross-cutting concerns

- Data-driven content, localization-ready instructional text, keyboard/readability support, deterministic objective validation, progress privacy, and no coupling between scenario files and Bevy systems.
