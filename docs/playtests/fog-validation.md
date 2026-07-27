# Fog-of-war paired validation

## Automated evidence

- Scenario: **The Veiled Crossing**, authored overlay schema 1, radius 3.
- Golden trace: `crates/crownline_core/tests/fog_validation.rs` pins canonical
  hashes, both projection hashes, and both explored-square counts for revisions
  0 through 12.
- Privacy: generated disclosure properties, headless Bevy entity/tile tests,
  two-seat protocol serialization scans, opaque rejection tests, and active
  export blocking run in `scripts/check.sh`.
- Performance: the 2026-07-27 24x24 release-profile results and ceilings are in
  `docs/performance-baseline.md`.

## Required human session record

This gate requires at least two side-swapped games by the same pair, followed by
separate answers before discussion. Do not tune radius 3 from one side or one
game. Record build commit, display size, input method, clock, game duration,
side, result, and whether the players had prior Crownlines experience.

Each player must answer without prompting:

1. What do `?`, dim terrain/`·`, and normal terrain mean?
2. Why can an enemy piece disappear, and is its last square still known?
3. Which facts remain public even when their cause is unseen?
4. When can a complete replay truth be exported?
5. Did the handoff ever show the outgoing or incoming board to the wrong player?
6. Was radius 3 useful, too restrictive, or too revealing, and at what moment?
7. Did hidden blockers, check, or settlement changes feel understandable?
8. Did uncertainty cause avoidable stalemate, frustration, or excessive match length?

## Status

- [ ] Two side-swapped local games completed.
- [ ] Two side-swapped online games completed.
- [ ] Both players correctly explained all three tile states.
- [ ] Both players correctly explained disappearing enemies and public facts.
- [ ] No accidental shoulder-surfing frame was observed.
- [ ] Radius decision recorded with evidence.
- [ ] Check, settlement, duration, and frustration notes reviewed.

Radius 3 remains the authored baseline until this record is completed. Automated
tests establish determinism, privacy boundaries, compatibility, and performance;
they cannot establish comprehension or playability.
