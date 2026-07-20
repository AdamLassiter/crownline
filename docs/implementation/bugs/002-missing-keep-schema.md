# Bug 002: Completed scenario schema omits Keeps

## Status

- [x] Done

## Linked task and introducing commit

- Task: [02.01.01 Define scenario schema](../02-core-domain/02.01-scenarios/02.01.01-schema.md)
- Introduced by: `202d7dd` (`feat(core): define scenario schema [Task 02.01.01]`)

## Reproduction

Inspect `ScenarioDefinition` after Task 02.01.01. It represents fortifications and castling routes but has no field or type for a player's Keep, deployment area, gates, or linked towers.

## Expected behavior

The completed scenario schema can describe both starting Keeps required by GDD sections 4, 11, and 20 and supports Task 02.01.02's Keep-exit validation.

## Actual behavior

Keep ownership and geometry cannot be authored or validated.

## Impact

Shipped scenarios could not prove protected deployment, multiple exits, or ownership of linked fortifications.

## Resolution

Added `KeepDefinition` with stable ID, owner, tile set, gate-edge set, and linked fortification IDs, plus a versioned `keeps` collection on `ScenarioDefinition`.

## Dependencies

- Task 02.01.01.

## Acceptance criteria

- Keeps serialize and deserialize through the versioned scenario schema.
- Keep tiles, gates, owners, and linked fortifications have deterministic ordered representations.
- Existing custom rule fixtures remain valid with no Keeps.
- Workspace formatting, Clippy, and tests pass.
