# Bug 001: Scenario rules fail the strict Boolean-structure lint

## Status

- [x] Done

## Linked task and introducing commit

- Task: [02.01.01 Define scenario schema](../02-core-domain/02.01-scenarios/02.01.01-schema.md)
- Introduced by: `202d7dd` (`feat(core): define scenario schema [Task 02.01.01]`)

## Reproduction

Run:

```sh
cargo clippy --workspace --all-targets -- -D warnings
```

Clippy rejects `ScenarioRules` because it contains more than three Boolean fields.

## Expected behavior

The completed scenario schema passes the repository's warnings-denied quality gate and uses explicit domain states where a Boolean obscures intent.

## Actual behavior

`require_standard_armies` was the fourth Boolean and triggered `clippy::struct_excessive_bools`.

## Impact

The full local and CI quality gates failed, contradicting Task 02.01.01's completion evidence.

## Resolution

Replaced `require_standard_armies: bool` with the serialized `ArmySetup::{Standard, Custom}` enum and updated validation and fixtures.

## Dependencies

- Task 02.01.01.

## Acceptance criteria

- Scenario behavior remains explicit and serializable.
- Standard scenarios still enforce complete armies; focused rule fixtures may select custom armies.
- Workspace formatting, Clippy, and tests pass.
