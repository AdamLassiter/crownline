# Bug 019: Fog save rejected by stale scenario catalog bound

## Status

- [x] Done

## Resolution

- Save decoding now validates the selected setup index against the authoritative current `ScenarioCatalog` length instead of the historical literal value `3`.
- A regression test accepts every current catalog index, including the fog scenario at index 3, and rejects the first index beyond the catalog.

## Linked tasks and introducing commits

- [Task 07.04.02](../07-local-client/07.04-persistence/07.04.02-save-load.md), commit `7c6fb1f`, introduced a save-wrapper check fixed to the original three-scenario catalog.
- [Task 12.01.06](../12-fog-of-war/12.01-visibility/12.01.06-validation.md), commit `47bfc2f`, added the fourth fog scenario and exposed the stale bound.

## Reproduction

1. Select The Veiled Crossing, which is catalog index 3.
2. Start a local human match and save it.
3. Load that slot.

## Expected behavior

Every scenario offered by current local setup can round-trip through a save slot.

## Actual behavior

The wrapper decoder rejects index 3 as `saved scenario selection is invalid` because it requires the index to be less than the literal value 3.

## Impact

The authored fog scenario can be played but its local saves cannot be loaded.

## Dependencies

- 07.04.02, 12.01.06.

## Acceptance criteria

- Every current catalog index is accepted by save decoding.
- The first index outside the current catalog is rejected.
- Existing save formats and embedded scenario validation remain unchanged.
