# Bug 024: Online GUI systems conflict during startup

## Status

- [x] Done

## Resolution

- The online lobby and online lifecycle roots now explicitly exclude their
  descendant control marker components, while each control query excludes its
  corresponding root.
- These complementary `With`/`Without` filters prove the mutable
  `Visibility` accesses are disjoint to Bevy without changing which entities
  either system owns.
- A native startup run now reaches the Home menu at both 800x480 and 1280x800
  instead of panicking while the first update schedule is initialized.

## Linked tasks and introducing commits

- [Task 07.04.04](../07-local-client/07.04-menu-system/07.04.04-guided-online.md),
  commit `1be0001`, added separate mutable lobby-root and lobby-control
  visibility queries without disjoint filters.
- [Task 07.04.06](../07-local-client/07.04-menu-system/07.04.06-match-menus.md),
  commit `8124969`, repeated the conflicting query shape for online lifecycle
  controls.
- [Task 07.04.07](../07-local-client/07.04-menu-system/07.04.07-validation.md)
  found and resolves the startup failure during required live validation.

## Reproduction

1. Start the native client after the online GUI controls are registered.
2. Allow Bevy to initialize the first `Update` schedule.
3. Observe Bevy error B0001 before the Home menu can render.

## Expected behavior

The client starts normally and both online surfaces can update root and control
visibility independently.

## Actual behavior

Bevy rejects the schedule because two queries in each affected system may
mutably access `Visibility` on the same entity.

## Impact

The native client cannot reach any game mode.

## Dependencies

- 07.04.04, 07.04.06, 07.04.07.

## Acceptance criteria

- The complete client starts without Bevy query-conflict panics.
- Lobby and lifecycle roots retain independent visibility from their controls.
- Both required live menu viewport checks reach a rendered, interactive Home
  menu.
