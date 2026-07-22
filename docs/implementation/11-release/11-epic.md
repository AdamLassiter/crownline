# Epic 11: Release and operations

Package, document, verify, and distribute the cross-platform client and containerized server with a clear compatibility policy.

## Status

- [ ] In progress

## Implementation notes

- Task 11.01.02 completes provider-neutral server operations documentation and connects its backup/restore procedure to the scheduled multi-room soak evidence.

## Stories

- [11.01 Player and operator documentation](11.01-documentation/11.01-story.md)
- [11.02 Desktop and server packaging](11.02-packaging/11.02-story.md)
- [11.03 Compatibility and release gate](11.03-release-gate/11.03-story.md)

## Dependencies

- Epics 01-10.

## Acceptance criteria

- Linux, Windows, and macOS client archives and the Linux server image are reproducible from a tagged revision.
- Documentation covers play, hosting, backup, recovery, and compatibility.
- Release candidates pass automated gates and recorded manual smoke tests.

## Cross-cutting concerns

- Signing/checksums, licenses, rollback, migrations, secret-free artifacts, and user-visible known limitations.
