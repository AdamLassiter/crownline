# Bug 005: Local interaction affordance queries conflict at runtime

## Status

- [x] Done

## Linked task and introducing commit

- Task: [07.01.01 Implement selection and confirmation](../07-local-client/07.01-interaction/07.01.01-selection.md)
- Introduced by: `0adb64c` (`feat(client): implement selection and confirmation [Task 07.01.01]`)
- Exposed by: [07.01.03 Implement mandatory-choice controls](../07-local-client/07.01-interaction/07.01.03-choices.md)

## Reproduction

Start an app containing `BoardRenderingPlugin` and `LocalInteractionPlugin`, then run the first update containing `sync_interaction_affordances`.

## Expected behavior

The keyboard-focus marker and interaction-help text update as independent presentation entities.

## Actual behavior

Bevy rejected the system with ECS error B0001 because both queries requested mutable `Transform` access and their filters did not prove that the entity sets were disjoint.

## Impact

The desktop client panicked on its first update before local board interaction became usable.

## Resolution

Added reciprocal `Without` filters to make the focus-marker and help-text query sets statically disjoint. Added a headless regression test that runs the combined rendering and local-interaction plugins through an update.

## Dependencies

- Task 07.01.01.

## Acceptance criteria

- The combined rendering and local-interaction plugins complete an update without an ECS query conflict.
- Exactly one keyboard-focus affordance is spawned.
- Workspace formatting, Clippy, and tests pass.
