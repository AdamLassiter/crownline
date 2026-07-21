# Bug 003: Repetition records the pre-realm command state

## Status

- [x] Done

## Linked task and introducing commit

- Task: [03.01.02 Implement piece pseudo-legal movement](../03-rules-geometry/03.01-movement/03.01.02-piece-moves.md)
- Introduced by: `1316135` (`feat(core): implement piece movement [Task 03.01.02]`)
- Exposed by: [04.02.03 Implement Pawn production](../04-realm-systems/04.02-settlements/04.02.03-production.md)

## Reproduction

Complete a command that starts an owner turn where a settlement develops or queues a mandatory realm choice, then inspect `repetition_counts` using the final state's repetition key.

## Expected behavior

The repetition entry is recorded after every deterministic turn-start realm effect and represents the final canonical position presented for the new turn.

## Actual behavior

`finish_command` recorded repetition immediately after switching players, before settlement transfer, continuity, development, and mandatory-choice processing. The stored key could therefore describe a transient state that was never offered for player input.

## Impact

Threefold repetition could miss identical playable positions or count transient pre-realm positions, producing an incorrect automatic draw.

## Resolution

Moved repetition recording to the outer transactional reducer after all non-terminal owner-turn realm effects. Checkmate still resolves before repetition, and mandatory-choice actions do not create extra turn-position entries.

## Dependencies

- Tasks 03.01.02 and 04.01.02.

## Acceptance criteria

- A turn with settlement realm effects records the final state's repetition key.
- No pre-realm transient key is counted for that command.
- Checkmate retains precedence over repetition.
- Workspace formatting, Clippy, and tests pass.
