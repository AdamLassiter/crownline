# Bug 009: Accessibility audit names the wrong Bevy version

## Status

- [x] Done

## Linked task and introducing commit

- Task: [10.03.02 Audit readability and accessibility](../10-quality/10.03-playtesting/10.03.02-accessibility.md)
- Introduced by: `935fc2e` (`feat(client): complete accessibility audit [Task 10.03.02]`)
- Exposed by: [11.01.01 Write player documentation](../11-release/11.01-documentation/11.01.01-player-docs.md)

## Reproduction

Compare the version named in `docs/accessibility-audit.md` with the locked workspace dependency reported by `cargo tree` or `Cargo.toml`.

## Expected behavior

Recorded audit build metadata identifies the Bevy version actually compiled by the workspace.

## Actual behavior

The audit named Bevy 0.18.1 after consulting an older source tree that was also present in the local Cargo registry. The workspace depends on and compiles Bevy 0.19.0.

## Impact

The accessibility results were valid, but their provenance was inaccurate and could mislead future regression comparisons.

## Resolution

Correct the audit header to Bevy 0.19.0 and retain the Rust version, date, and base revision unchanged.

## Dependencies

- Task 10.03.02.

## Acceptance criteria

- Audit metadata agrees with the locked workspace dependency.
- No test result or accessibility conclusion is changed.
