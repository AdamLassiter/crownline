# Epic 01: Foundation

Establish the workspace, dependency boundaries, automated checks, and licensed assets on which all later work relies.

## Status

- [ ] In progress

## Stories

- [01.01 Workspace architecture](01.01-workspace/01.01-story.md)
- [01.02 Engineering automation](01.02-automation/01.02-story.md)
- [01.03 Runtime foundations and assets](01.03-runtime-assets/01.03-story.md)

## Dependencies

- None.

## Acceptance criteria

- The empty stub is replaced by a compiling workspace with client, core, protocol, and server boundaries.
- CI enforces the shared definition of done.
- The desktop client opens a Bevy window and the server exposes a health endpoint.
- All bundled assets have recorded licenses.

## Cross-cutting concerns

- Compatibility, licensing, platform portability, build reproducibility, and observability.
