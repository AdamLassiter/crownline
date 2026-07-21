# Bug 006: Populated match states cannot round-trip through JSON

## Status

- [x] Done

## Linked task and introducing commit

- Task: [02.02.01 Define canonical match state](../02-core-domain/02.02-match-state/02.02.01-state.md)
- Introduced by: `065ae42` (`feat(core): add canonical match state [Task 02.02.01]`)
- Exposed by: [08.01.02 Define synchronization contract](../08-online-server/08.01-protocol/08.01.02-sync-contract.md)

## Reproduction

Serialize and deserialize a standard `MatchState` containing pieces with `serde_json`.

## Expected behavior

The canonical state round-trips with identical piece IDs and hash.

## Actual behavior

JSON object keys encode numeric `PieceId` values as strings, while the derived newtype deserializer accepted only a numeric value. Deserialization failed on the first piece key with `invalid type: string "0", expected u32`.

## Impact

Local saves and authoritative online snapshots could serialize populated matches but could not load them again.

## Resolution

Added a `PieceId` deserializer that accepts both ordinary numeric values and numeric JSON object keys while preserving the existing numeric serializer. Added a populated canonical-state JSON round-trip regression.

## Dependencies

- Task 02.02.01.

## Acceptance criteria

- Populated match states deserialize numeric `PieceId` JSON object keys.
- Numeric `PieceId` values remain backward compatible.
- Workspace formatting, Clippy, and tests pass.
