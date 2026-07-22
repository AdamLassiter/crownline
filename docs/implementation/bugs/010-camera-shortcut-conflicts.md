# Bug 010: Camera shortcuts trigger match and reconnect commands

## Status

- [x] Done

## Linked tasks and introducing commits

- Camera bindings: [06.02.02 Implement camera controls](../06-rendering/06.02-camera/06.02.02-camera-controls.md), commit `f1dc1f2`.
- First conflicting command: [07.03.01 Implement setup and match end](../07-local-client/07.03-match-lifecycle/07.03.01-setup-end.md), commit `65f0226`.
- Later conflicting command: [09.01.02 Persist reconnect credentials](../09-online-client/09.01-connection/09.01.02-reconnect.md), commit `25bed7d`.
- Exposed by: [11.01.01 Write player documentation](../11-release/11.01-documentation/11.01.01-player-docs.md).

## Reproduction

During an active local or online match, press the default `Q` camera zoom-out key or `D` pan-right key. During an online match, press the default `F` camera reset key.

## Expected behavior

Camera input is distinguishable from gameplay/lifecycle input. A camera command never opens resignation, offers a draw, or forgets an online seat.

## Actual behavior

The camera system ran in every client flow and read unmodified Q/E/F/W/A/S/D keys. Later lifecycle systems assigned `Q` to resignation, `D` to draw, and `F` to forgetting a seat, so a single key could execute both meanings. Camera input also moved the hidden board behind setup and modal screens.

## Impact

Players could enter a destructive confirmation, send a draw offer, or delete their locally saved reconnect credential while intending only to navigate the camera.

## Resolution

Camera keyboard actions now require either Shift key and run only during local or online play. Plain Q/D/F remain exclusively lifecycle commands; their handlers ignore the Shift-modified camera chord. Mouse wheel and middle/right drag retain their existing pointer-capture boundary.

## Dependencies

- Tasks 06.02.02, 07.03.01, and 09.01.02.

## Acceptance criteria

- Unmodified Q, D, and F cannot activate a camera action.
- Shift-modified Q, D, and F cannot resign, offer a draw, or forget a seat.
- Keyboard camera input is inactive in setup, lobby, pause, confirmation, and outcome flows.
- Mouse camera input remains blocked while UI owns the pointer.
