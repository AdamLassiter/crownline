# Bug 007: Restored matches lose authenticated room seats

## Status

- [x] Done

## Linked task and introducing commit

- Task: [08.04.02 Commit and restore matches](../08-online-server/08.04-operations/08.04.02-recovery.md)
- Introduced by: `1025fdc` (`feat(server): commit and restore matches [Task 08.04.02]`)
- Exposed by: [08.03.03 Broadcast snapshots and recover connections](../08-online-server/08.03-authority/08.03.03-broadcast.md)

## Reproduction

Create and start an authenticated two-seat room, persist its authority state, restart the server, and attempt to reconnect with either issued token.

## Expected behavior

The room, both display seats, and their hashed reconnect credentials are reconstructed with the restored canonical match.

## Actual behavior

Task 08.04.02 wrote the match and snapshot but no seat rows. Startup retained restored authorities in a new empty `RoomService`, so no reconnect token could resolve to a seat after restart.

## Impact

Healthy unfinished matches survived on disk but became unreachable to both players after any server restart.

## Resolution

Persist both bounded display seats and fixed-size token hashes in the match-registration transaction. Recovery now validates both seat rows, returns a complete room record with each authority, and reconstructs started rooms before the router accepts traffic. Raw reconnect tokens remain issuance-only and never enter persistence.

## Dependencies

- Task 08.04.02.

## Acceptance criteria

- Both seat rows and token hashes commit atomically with initial match registration.
- Startup reconstructs the room and maps each original token to its original seat.
- Invalid/missing seat records quarantine only the affected match.
- Raw reconnect tokens are never stored.
