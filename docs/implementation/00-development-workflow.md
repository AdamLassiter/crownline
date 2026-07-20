# Development workflow

Implementation is managed through the task files in this directory.

## Task lifecycle

1. Select the earliest unblocked task and mark it **In progress**.
2. Implement only that task's coherent change, including tests and documentation.
3. Add implementation notes and verification evidence to the task file.
4. Mark the task **Done** only when every acceptance criterion is satisfied.
5. Commit the implementation, tests, and task update together using a message that includes the task ID, for example `feat(core): add scenario schema [Task 02.01.01]`.

Each task receives its own commit. Story and epic status may be updated in the final task commit that completes them. Partial foundations shared by later tasks must be called out as partial and must not mark those later tasks done.

## Bugs

When implementation or verification discovers a defect in completed work:

1. Add a numerically indexed Markdown issue under `docs/implementation/bugs/`.
2. Link the bug to the task and introducing commit.
3. Record reproduction steps, expected/actual behavior, impact, dependencies, and acceptance criteria.
4. Fix it in a dedicated commit whose message includes both the bug ID and originating task ID.

An unmet acceptance criterion on an in-progress task is remaining task work, not a bug. A regression or contradiction in a task already marked Done is a bug.

## Status format

Issue files use one of:

- `- [ ] Not started`
- `- [ ] In progress`
- `- [x] Done`
- `- [ ] Blocked` with the blocking dependency named directly below it

