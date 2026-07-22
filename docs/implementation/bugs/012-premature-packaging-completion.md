# Bug 012: Packaging tasks were marked done before external release evidence

## Status

- [x] Done

## Linked tasks and introducing commits

- [11.02.01 Package desktop clients](../11-release/11.02-packaging/11.02.01-desktop.md), commit `efe9078`.
- [11.02.02 Publish the server image](../11-release/11.02-packaging/11.02.02-server-image.md), commit `506d708`.

## Reproduction

Read both task statuses before any trusted `vMAJOR.MINOR.PATCH` workflow has run. Compare the available local Linux evidence with the acceptance criteria requiring all native desktop archives and published immutable server coordinates, digest, scan records, SBOM, and attestations.

## Expected behavior

A task is marked Done only after every acceptance criterion has recorded evidence. Implemented release automation remains In progress while its native runners or external registry publication have not run.

## Actual behavior

Both task files were marked Done when their workflows and local Linux smoke tests were implemented, even though no release tag had produced the external artifacts required to satisfy all acceptance criteria.

## Impact

The implementation index overstated release readiness and could allow Task 11.03.02 to proceed without exact candidate artifacts.

## Resolution

Tasks 11.02.01 and 11.02.02, and their parent story, are restored to In progress with their exact external completion gates stated below the status. Implementation notes retain the completed local work and link this correction. No workflow or artifact behavior changes.

## Dependencies

- Tasks 11.02.01 and 11.02.02.

## Acceptance criteria

- Desktop packaging remains In progress until every native matrix job records clean-unpack and release evidence.
- Server packaging remains In progress until immutable GHCR tags and accompanying digest/evidence are actually published.
- Story 11.02 does not report completion while either task is incomplete.
