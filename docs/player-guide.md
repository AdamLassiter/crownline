# Crownlines player guide

Crownlines is a deterministic two-player strategy game: pieces move and capture
like chess, while Pawns claim settlements and seek fixed promotion sites. The
realm layer changes the value of position; it does not add random combat,
resources, hidden information, or unit statistics.

## Requirements and installation

The initial desktop release supports 64-bit Linux, Windows, and macOS. Use the
archive whose platform and architecture match your computer. Minimum supported
environment:

- A current 64-bit Linux, Windows 10 or later, or macOS 12 or later desktop.
- A current graphics driver with Vulkan (Linux), Direct3D 12 (Windows), or Metal
  (macOS) support suitable for Bevy/wgpu.
- 4 GiB RAM, 500 MiB free storage, a keyboard, and an 800x480 display. A
  1280x720 or larger display is recommended for the standard and large maps.
- Internet or LAN connectivity only for private online rooms. Remote servers
  must offer TLS (`wss://`); plaintext `ws://` is accepted only on loopback.

Download the release archive and its checksum from the same release page,
verify the checksum, extract the complete archive, and run `crownline`
(`crownline.exe` on Windows). Keep the binary and `assets` directory together.
Signing/notarization status and any platform-specific launch instructions are
listed in that release's notes. Source builds use the pinned Rust toolchain and
are documented separately in the repository README.

The archive includes the Noto Sans Symbols 2 chess font. Its provenance and
license are in [`assets/fonts/README.md`](../assets/fonts/README.md) and
[`assets/fonts/OFL.txt`](../assets/fonts/OFL.txt). Crownlines is licensed under
MIT OR Apache-2.0; release archives include the applicable license and notices.

## Quick start: local match

1. Start Crownlines. The local setup screen opens first.
2. Use Tab to focus the North and South name fields and type distinct names.
3. Use PageUp/PageDown to choose a scenario. The standard 20x20 **Crownlines**
   map is the default; **The First Crossing** is the shortest learning map.
4. Matches are untimed by default. Press `C` to enable a clock, `-`/`+` to set
   1-180 base minutes, and `,`/`.` to set a 0-60 second increment.
5. Press `F2` to start. Use the arrow keys to place visible board focus, Enter
   to select a piece and confirm a highlighted destination, Escape to release
   focus, and `H` to Hold.
6. Press `F1` at any time for the rules and complete board legend. Keys `1`-`5`
   select its sections and Escape closes it.

Click selection is also available. A click captured by a menu or information
panel never falls through to the board.

## Quick start: private online room

An operator must first provide the `wss://` server address. Hosting instructions
are in [`server-operations.md`](server-operations.md).

1. From local setup, press `F3`, then `H` to host. Tab through server address and
   player name, select scenario/clock as above, and press Enter.
2. Send the six-character room code to the other player. Press `A` before `C`
   if the copied invitation should also include the server address. Invitations
   never contain the seat credential.
3. The other player starts Crownlines, presses `F3`, then `J`, enters the same
   server address, name, and room code, and presses Enter.
4. Both players press `R` when ready. The server starts only after both seats are
   present and ready; it owns legal actions, clocks, revisions, and outcomes.

Online commands lock while awaiting the server. Do not submit a second intent;
the status line reports pending, reconnecting, rejected, or connected state.
`T` retries immediately, `X` cancels a retry, and `F` forgets the saved seat.

### Reconnect and rematch

The client stores only server/room/match/seat metadata in settings and stores
the high-entropy seat credential in the operating-system credential service.
If that service is unavailable, it uses a user-private local credential file
(mode `0600` on Unix). On restart the client automatically attempts the saved
seat, and the server sends a complete authoritative snapshot. Ordinary network
loss retries with bounded backoff; clocks continue on the server.

Do not share settings, credential files, or screenshots containing private
operational information. A room code is safe to invite with, but it does not
replace the secret seat credential. `F` permanently forgets the local seat; it
cannot be recovered from the room code.

After a terminal outcome, `R` requests or accepts a rematch, `N` declines, and
`L` leaves the finished room. A rematch creates a fresh revision-zero match with
the same room setup.

## Controls

| Context | Keyboard controls |
| --- | --- |
| Local setup | Tab edit names; `X` swap assignments; PageUp/PageDown scenario; `C`, `-`/`+`, `,`/`.` clock; `F2` start; `F3` online. |
| Board | Arrows focus; Enter select/move; Escape release; `H` Hold; Shift + configured camera key (defaults: W/A/S/D pan, Q/E zoom, `F` reset); mouse wheel/drag also work. |
| Mandatory promotion | `1` Queen, `2` Rook, `3` Bishop, `4` Knight. The same four buttons are clickable; locked choices report their required score without submitting. |
| Mandatory Pawn placement | Arrows cycle only legal adjacent squares; Enter confirms; Escape returns focus to the required choice. |
| Match | `P` pause/resume; `Q` resign then Enter/Escape; `D` offer draw; `Y` accept; `N` decline; `I` panels. |
| Saves | `F5` save; `F6` cycle slots 1-3; `F9` load. |
| Playtest evidence | `F8` explicitly exports the current local match's name-free structured record; nothing is uploaded. |
| Help | `F1` open/close; `1`-`5` sections; Escape close. |
| Online lobby | `H` host; `J` join; Tab fields; Enter submit; Escape back; `R` ready; `A` invitation address; `C` copy. |
| Online recovery | `T` retry; `X` cancel retry; `F` forget seat. |

Camera bindings can be changed in `settings.ron` but always require Shift, so
they remain distinct from lifecycle/gameplay keys. Those keys are fixed in the
initial release and are always printed in their relevant surface.

## Rules summary

- King, Queen, Rook, Bishop, Knight, and Pawn use ordinary chess movement.
  Pins and check apply. Castling is allowed only on the scenario's authored
  clear, unmoved, unattacked route. Every shipped scenario enables initial Pawn
  double-step and en passant.
- A turn completes one legal Move or Hold after all mandatory choices. Hold
  preserves occupancy and advances turn-boundary processes, but is unavailable
  in check or during a mandatory choice.
- Forest may be entered but stops a Queen/Rook/Bishop ray beyond that tile.
  Mountain cannot be entered. River and Wall edges block crossings; Bridge,
  Ford, and Gate edges reopen them. Knights leap over intervening terrain and
  edge barriers. Roads are visual/open-route map design, not extra movement.
- A Pawn claims a neutral settlement by ending on it. The Pawn is its founder.
  A friendly King, Queen, Rook, or Bishop governs the site through an unblocked
  geometric attack line; Knights and Pawns do not govern.
- With founder present, no enemy occupant, and continuous governance, all
  shipped scenarios establish after three owner-turn cycles. Interruption pauses
  rather than resets progress. An established settlement produces after three
  further eligible cycles and queues mandatory placement of one adjacent Pawn.
  A site supports only one produced Pawn until that Pawn is captured or promotes.
- A Pawn surviving on a promotion site becomes a mandatory promotion choice.
  **The First Crossing** requires one surviving cycle; **Crownlines** and **The
  Three Theatres** require two. Its owner's current control score is: 1 per
  owned settlement, +1 when that settlement is currently governed, and +2 when
  it is established. Knight is always available; Bishop unlocks at 2, Rook at
  4, and Queen at 8 in every shipped scenario.
- Examples: a promotion rush with no settlement control offers only Knight; one
  owned and governed settlement scores 2 and adds Bishop; establishing it
  scores 4 and adds Rook; two owned, governed, established settlements score 8
  and add Queen. The HUD marks every fixed choice READY or LOCKED, gives its
  threshold, and shows the score breakdown and next unlock.
- Promotion control is current, not a lifetime total. Losing governance or a
  settlement can relock a later promotion. Transfers and establishment complete
  before the owner-turn score is captured. All promotions made ready together
  share one frozen batch snapshot, so resolving the first cannot unlock the
  second and later live board changes do not alter choices already displayed.
- A match ends by checkmate, timeout, resignation, accepted draw, or automatic
  third repetition of the complete gameplay state. There is no economic victory.

The side panels explain check, phase, clocks, selected piece, settlement owner,
founder, governors, blockers, establishment/production fractions, readiness,
and recent ordered events. Hover previews are non-mutating explanations; they do
not rank or recommend moves.

## Scenarios

| Scenario | Board | Expected time | Purpose |
| --- | ---: | ---: | --- |
| The First Crossing | 16x16 | 30-45 min | Compact symmetric learning battlefield with Keeps, crossings, settlements, terrain, governance, and faster promotion. |
| Crownlines | 20x20 | 60-90 min | Default battlefield with four river crossings, tower walls, distributed settlements, and contested central heights. |
| The Three Theatres | 24x24 | 105-135 min | Long-form western, central, and eastern fronts divided by paired rivers and multiple crossings. |

These times are design targets pending the final structured balance record; they
are not turn limits.

## Saves and clocks

Local play has three atomic save slots. `F5` writes the selected slot through a
validated temporary file before replacement; a failed write preserves the prior
save. `F9` validates file versions, embedded scenario, canonical state, and hash
before changing the match. Loading does not charge offline time. Unsupported or
corrupt saves remain unchanged and produce a recoverable message. Compatibility
details are in [`compatibility.md`](compatibility.md).

For local clocks, pause/setup/outcome states stop charging. The active clock
continues during mandatory choices. Time is charged before action validation;
expiration at the deadline wins, and increment is added only after an accepted
Move or Hold. Online clocks are server-owned and continue through disconnects.

## Accessibility

- Information never relies on hue alone: terrain has F/M/R marks, Keeps N/S,
  settlements -/N/S, owner plates differ in contrast and rotation, and every
  overlay meaning has a unique symbol plus legend text.
- Keyboard-only play covers setup, online rooms, board focus, choices, match
  controls, saves, help, confirmations, and reconnect controls.
- Set `ui_scale` from 0.75 through 2.5 in `settings.ron`. Window dimensions
  divided by scale must provide at least 800x480 logical pixels; unsupported
  combinations are rejected with an actionable message. Scaled panels scroll.
- Set `reduced_motion: true` to remove piece interpolation and retirement ghosts
  while retaining immediate results and ordered static feedback.

The reproducible evidence matrix is in
[`accessibility-audit.md`](accessibility-audit.md).

## Known exclusions

The initial release has no AI opponent, public matchmaking, spectators, player
accounts, factions/asymmetric rules, scenario editor, campaign, web client, or
mobile client. It is two-player hot-seat or private-room play only. It also has
no random combat, unit health, resource inventory, worker units, technology
tree, hidden information, or separate economic victory.

## Troubleshooting

- **Chess pieces are missing:** keep the `assets/fonts` directory beside the
  executable. A readable in-client fallback reports a font-load failure.
- **Settings reset to defaults:** inspect the warning and correct the named field
  in the platform configuration directory's `Crownlines/settings.ron`. Scale and
  window size must retain at least 800x480 logical pixels.
- **Remote server address is rejected:** use the operator's full `wss://`
  address. Plaintext `ws://` is allowed only for `127.0.0.1`/localhost.
- **Versions are incompatible:** install matching client/server release versions;
  protocol mismatch is rejected before a room seat is created or joined.
- **Online match is retrying:** wait for bounded retry or press `T`; use `X` to
  cancel. If the stored credential is unavailable, forget with `F` and ask the
  host for a new room. A room code cannot recreate a lost seat credential.
- **Save/load fails:** the on-screen message names the slot and cause. Check free
  space and user-directory permissions; do not hand-edit the JSON file.
- **Board is obscured:** press `I` to collapse panels, Shift+`F` to reset the
  camera, or choose a larger window/lower valid UI scale.

See the [privacy statement](privacy.md) for local and online data handling.
