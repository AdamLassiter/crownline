# Bug 018: Scenario replacement queues duplicate entity despawns

## Status

- [x] Done

## Resolution

- The rendering update is now one explicit ordered chain. `rebuild_changed_scenario` runs first and an `ApplyDeferred` boundary commits its recursive teardown before piece, settlement, overlay, motion, and transition systems query the projection.
- The scenario-replacement regression test installs Bevy's panic error handler, so a duplicate deferred despawn or any other command error fails the test instead of being reduced to a warning.
- `./scripts/check.sh` passed with 257 tests passing and the two scheduled-only tests ignored.
- A live 800x480 Xephyr pass loaded a valid different-scenario save at revision 4. The rebuilt board, pieces, terrain/features, coordinate labels, side panels, and bottom mandatory-choice region remained visible and correct, with no `Entity despawned` warning.

## Linked tasks and introducing commits

- [Task 07.03.01](../07-local-client/07.03-match-lifecycle/07.03.01-setup-end.md), commit `65f0226`, added scenario-ID replacement that despawns all `ScenarioVisual` entities while piece and settlement projection systems can queue despawns for the same entities in the same update.

## Reproduction

1. Start the desktop client with warning logs visible.
2. Load a valid local save whose scenario differs from the currently displayed scenario.
3. Observe the first update after the canonical scenario and state are replaced.

## Expected behavior

Scenario replacement removes the previous projection once, rebuilds the new scenario, and emits no ECS command errors.

## Actual behavior

Bevy reports repeated `Entity despawned: The entity ... is invalid` command warnings. The scenario-boundary system queues despawns for every `ScenarioVisual`, while other projection systems still query the pre-flush world and queue additional despawns for stale pieces or settlements.

## Impact

The loaded game currently remains playable, but expected scenario replacement produces noisy engine errors and depends on Bevy tolerating duplicate deferred commands. This can conceal a real projection failure and makes save/load diagnostics unreliable.

## Dependencies

- 07.03.01, 06.01.01, 06.01.02, 06.01.03, 07.03.03.

## Acceptance criteria

- Scenario replacement has one explicit owner for removing old projection entities.
- Piece, settlement, overlay, and transition projection systems do not queue commands against entities already scheduled for scenario teardown.
- Starting a different scenario and loading a different-scenario save produce no `Entity despawned` warnings.
- Headless coverage exercises the same multi-system update that previously emitted duplicate commands.
- A live save/load pass confirms the rebuilt board, pieces, features, labels, and bottom interaction region remain correct.
