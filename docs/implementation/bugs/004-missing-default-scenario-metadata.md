# Bug 004: Scenario metadata cannot mark the default scenario

## Status

- [x] Done

## Linked task and introducing commit

- Task: [02.01.01 Define scenario schema](../02-core-domain/02.01-scenarios/02.01.01-schema.md)
- Introduced by: `202d7dd` (`feat(core): define scenario schema [Task 02.01.01]`)
- Exposed by: [05.02.01 Author the 20x20 layout](../05-scenarios/05.02-standard/05.02.01-layout.md)

## Reproduction

Attempt to author the standard scenario so installed-scenario discovery can select it as the default using only `ScenarioMetadata`.

## Expected behavior

Scenario metadata contains a stable, machine-readable default marker independent of display names, descriptions, file ordering, or host-specific hard coding.

## Actual behavior

`ScenarioMetadata` contained only name, description, and expected duration. No authored field could satisfy Task 05.02.01's default-marker acceptance criterion.

## Impact

Local and online hosts would need to duplicate a scenario ID or infer the default from presentation text, creating inconsistent selection behavior and fragile data-only balance changes.

## Resolution

Added a Serde-defaulted `is_default` Boolean to scenario metadata. Existing RON remains backward compatible and explicitly authored standard scenarios can be discovered without presentation coupling.

## Dependencies

- Tasks 02.01.01 and 05.02.01.

## Acceptance criteria

- Authored scenario metadata can explicitly mark one scenario as the default.
- Older RON without the field deserializes with `is_default = false`.
- Canonical scenario serialization and hashing include the marker.
- Workspace formatting, Clippy, and tests pass.
