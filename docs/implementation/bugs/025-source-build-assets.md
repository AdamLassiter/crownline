# Bug 025: Source builds resolve assets beside the debug binary

## Status

- [x] Done

## Resolution

- Runtime asset discovery still prefers an `assets` directory installed beside
  the executable, preserving packaged desktop behavior.
- When packaged sibling assets are absent, source builds now select the
  repository's absolute manifest-relative `assets` directory. The final
  relative fallback remains for environments where neither development nor
  packaged assets can be inspected during startup.
- A regression requires the selected runtime root to contain the bundled chess
  font, catching the former silent fallback before a live run.
- A native 800x480 source-build run loads the bundled font without an asset
  error and reaches the unified Home menu.

## Linked task and introducing commit

- [Task 11.02.01](../11-release/11.02-packaging/11.02.01-desktop.md), commit
  `efe9078`, introduced packaged sibling-asset discovery with a relative source
  fallback that Bevy also resolved beside the executable.
- [Task 07.04.07](../07-local-client/07.04-menu-system/07.04.07-validation.md)
  found the failure during its clean post-gate native startup check.

## Reproduction

1. Build or run the client from the repository without copying `assets` into
   `target/debug`.
2. Observe Bevy request
   `target/debug/assets/fonts/NotoSansSymbols2-Regular.ttf`.
3. Observe the readable ASCII fallback instead of bundled chess glyphs.

## Expected behavior

Source builds load repository assets while packaged builds load assets installed
beside their executable.

## Actual behavior

The relative development fallback is interpreted relative to the executable,
so it points at a nonexistent `target/debug/assets` tree.

## Impact

Developer and playtest builds do not display the required Unicode chess pieces
unless assets are manually copied into the build directory.

## Dependencies

- 01.03.01, 11.02.01, 07.04.07.

## Acceptance criteria

- Packaged sibling assets retain precedence.
- Source builds resolve a complete repository asset tree without copying it.
- The bundled chess font loads in a native source-build startup.
