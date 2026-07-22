# Bug 011: Concurrent tag workflows can race to create a release

## Status

- [x] Done

## Linked task and introducing commit

- Introduced by [11.02.01 Package desktop clients](../11-release/11.02-packaging/11.02.01-desktop.md), commit `efe9078`.
- Exposed while implementing [11.02.02 Publish the server image](../11-release/11.02-packaging/11.02.02-server-image.md).

## Reproduction

Push a release tag while two publication workflows independently run `gh release view TAG || gh release create TAG`. Allow both to complete the view before either create finishes.

## Expected behavior

Exactly one workflow creates the shared GitHub release. The other observes that release and continues uploading its independently named assets.

## Actual behavior

Both workflows could observe a missing release, then the losing `gh release create` failed because the winner had created it first. The losing workflow stopped before uploading otherwise valid artifacts.

## Impact

A valid tagged release could omit all desktop artifacts solely because server publication won a timing race, requiring a manual rerun.

## Resolution

Desktop publication now retries a failed create as a release view. A genuine creation failure still fails because the fallback view also fails; a concurrent winner satisfies the view and publication continues.

## Dependencies

- Task 11.02.01.

## Acceptance criteria

- Two tag workflows may safely converge on one shared release.
- A non-race creation failure is not hidden.
- Desktop asset upload remains idempotent through `--clobber`.
