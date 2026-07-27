# Epic 12: Hidden information

Add optional incomplete-information rules without exposing authoritative state or weakening deterministic play.

## Status

- [ ] In progress

## Stories

- [12.01 Fog of war](12.01-fog-of-war/12.01-story.md)

## Dependencies

- Epics 02-10. This is post-release feature work and does not reopen the initial release gate.

## Acceptance criteria

- Fog-enabled scenarios distinguish undiscovered, explored, and currently visible squares for each player.
- Local and online players receive only the information their seat is entitled to observe.
- Existing perfect-information scenarios, saves, replays, and private rooms retain their current behavior through explicit compatibility handling.
- Hidden information never changes authority: `crownline_core` and the online server still own canonical truth.

## Cross-cutting concerns

- Deterministic visibility, serialization migration, per-seat privacy, accessibility, replay policy, and bounded projection cost on 24x24 maps.
