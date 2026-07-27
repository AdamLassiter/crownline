# Bug 021: Guided crossing objectives miss sliding-piece paths

## Status

- [x] Done

## Resolution

- Guided crossing predicates now inspect every orthogonal or diagonal unit boundary traversed by a canonical move instead of treating a multi-square move's endpoints as one edge.
- Diagonal steps use the same four component boundaries as canonical movement blocking, while Knight jumps deliberately do not claim to cross intervening edges.
- Movement-pack reachability coverage exercises Rook crossings through a Bridge and a linked projected Wall.

## Linked task and introducing commit

- [Task 14.01.01](../14-guided-scenarios/14.01-framework/14.01.01-guided-schema.md), commit `fe1b386`, introduced the declarative `CrossEdge` event predicate with endpoint-only matching.

## Reproduction

1. Author a guided objective requiring a Rook to cross a Bridge edge.
2. Move the Rook two or more squares so the Bridge lies between the start and destination.
3. Evaluate the accepted `PieceMoved` event against `CrossEdge`.

## Expected behavior

The objective matches any edge actually traversed along the legal movement path.

## Actual behavior

The predicate constructs one non-adjacent edge from the move endpoints, which cannot equal the authored unit Bridge edge, and remains in progress.

## Impact

Crossing lessons and challenges cannot observe normal multi-square slider crossings and may appear impossible despite a correct move.

## Dependencies

- 14.01.01.

## Acceptance criteria

- Orthogonal and diagonal slider paths match each traversed authored edge kind.
- Adjacent crossings remain unchanged.
- Knight jumps do not report crossing intervening barriers.
