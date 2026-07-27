# Crownlines player guide

Crownlines is a deterministic two-player strategy game: pieces move and capture
like chess, while Pawns claim settlements and seek fixed promotion sites. The
realm layer changes the value of position; it does not add random combat,
resources or unit statistics. Most scenarios use perfect information; The
Veiled Crossing is an optional fog-of-war variant.

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

1. Start Crownlines. Home opens first; choose **New Local Match**.
2. Select the North and South name fields and type distinct names. Choose each
   seat's Human or AI controller and use **Swap sides** if needed.
3. Use **Previous scenario** or **Next scenario** to choose a map. The standard
   20x20 **Crownlines** map is the default; **The First Crossing** is the
   shortest learning map.
4. Matches are untimed by default. Use **Toggle clock**, then the base and
   increment controls to configure 1-180 base minutes and a 0-60 second
   increment.
5. Choose **Start Local Match**. Use the arrow keys to place visible board
   focus, Enter to select a piece and confirm a highlighted destination,
   Escape to release focus, and `H` to Hold. Pointer selection is also
   available.
6. Open **Rules & Legend** from Home or the Match menu at any time. The optional
   `F1` accelerator opens the same help.

Tab and Shift-Tab traverse visible enabled menu controls, Enter or Space
activates the focused control, and Escape goes back. A pointer action captured
by a menu or information panel never falls through to the board.

## Guided lessons and AI challenges

Choose **Guided Play** from Home. The browser provides visible category,
lesson, resume/start, reset, and navigation controls; Tab/Shift-Tab and
Enter/Space provide the equivalent keyboard path. Guided play uses normal
movement, realm, promotion, check, and outcome rules; its scenarios do not
appear in competitive local or online setup.

The first guided scenario is initially unlocked. Each later scenario unlocks
only after the preceding scenario is completed. You can browse a locked
scenario to inspect its category and prerequisite, but its Start and Resume
controls remain disabled and outside keyboard focus until that prerequisite is
recorded.

The objective panel explains the current stage. Press `J` to reveal the next
progressive hint and `T` to retry the stage. Reset requires two explicit
requests. Progress, retries, hint counts, and best action/turn counts are stored
locally in a separate atomic `guided-progress.json`; ordinary save slots are not
used or deleted. Apprentice, Steward, and Warden are deterministic local AI
effort profiles, not validated human skill ratings.

## Quick start: private online room

An operator must first provide the `wss://` server address. Hosting instructions
are in [`server-operations.md`](server-operations.md).

1. Choose **Online Play** from Home, edit the server address and player name,
   then choose **Host private room**. Select the scenario and clock with the
   visible controls and choose **Create room**.
2. Send the six-character room code to the other player. Choose whether
   **Include server address** is enabled, then use **Copy invitation**.
   Invitations never contain the seat credential.
3. The other player chooses **Online Play**, then **Join with code**, enters the
   same server address, name, and room code, and chooses **Join room**.
4. Both players choose **Ready**. The server starts only after both seats are
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

After a terminal outcome, visible controls request/accept or decline a rematch
and leave the finished room. The optional `R`, `N`, and `L` accelerators invoke
those same actions. A rematch creates a fresh revision-zero match with the same
room setup.

## Controls

| Context | Pointer and keyboard controls |
| --- | --- |
| Menus | Select any visible control; Tab/Shift-Tab navigate enabled controls; Enter/Space activate; Escape back. |
| Local setup | Visible name, controller, side, scenario, clock, and start controls. Optional accelerators: `X`, PageUp/PageDown, `C`, `-`/`+`, `,`/`.`, F7/F8, and `F2`. Character accelerators pause while editing text. |
| Guided browser/play | Visible browser and objective controls; PageUp/PageDown choose; Enter start/resume; `J` hint; `T` retry; Escape leave/back. |
| Board | Arrows focus; Enter select/move; Escape release; `H` Hold; Shift + configured camera key (defaults: W/A/S/D pan, Q/E zoom, `F` reset); mouse wheel/drag also work. |
| Mandatory promotion | `1` Queen, `2` Rook, `3` Bishop, `4` Knight. The same four buttons are clickable; locked choices report their required score without submitting. |
| Mandatory Pawn placement | Arrows cycle only legal adjacent squares; Enter confirms; Escape returns focus to the required choice. |
| Match | `P` opens the Match menu; visible controls resume, save/load, open settings/rules, draw, resign, or return Home. `Q`, `D`, `Y`, and `N` remain optional online accelerators; `I` collapses panels. |
| Saves | The Match menu shows all three slots as Empty, Valid, or Unreadable and confirms overwrite/load where required. |
| Playtest evidence | `F8` explicitly exports the current local match's name-free structured record; nothing is uploaded. |
| Help | `F1` open/close; `1`-`5` sections; Escape close. |
| Online lobby | Visible Host, Join, scenario, clock, invitation, Ready, and Leave controls; Tab/Shift-Tab traverse fields and controls. |
| Online recovery | `T` retry; `X` cancel retry; `F` forget seat. |

Settings is divided into Display, Accessibility, Controls, and Online tabs.
Changes preview where useful but persist only after **Apply**; **Cancel**
restores the prior scale, motion preference, and configuration. Camera bindings
always require Shift, so they remain distinct from lifecycle/gameplay keys.
Read-only information uses slate panes, editable text uses green-tinted fields,
and interactive buttons use blue states. Back, Cancel, and Quit share a brown
exit treatment. Quit acts immediately after selection; match abandonment,
resignation, overwrite, load replacement, and credential deletion retain their
own confirmations where data or match state would change.

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
| The Veiled Crossing | 16x16 | 35-55 min | The First Crossing with scenario-tuned radius-3 fog of war and secure local/online seat views. |
| Crownlines | 20x20 | 60-90 min | Default battlefield with four river crossings, tower walls, distributed settlements, and contested central heights. |
| The Three Theatres | 24x24 | 105-135 min | Long-form western, central, and eastern fronts divided by paired rivers and multiple crossings. |

These times are design targets pending the final structured balance record; they
are not turn limits.

### Fog of war

On The Veiled Crossing, every friendly piece currently sees all squares within
Chebyshev radius 3. `?` squares are fully undiscovered and reveal neither board
parity nor terrain. Dim terrain (or `·` for open ground) is explored static
knowledge outside current vision. Normal terrain is currently visible. Static
terrain, sites, Keeps, fortifications, and discovered edges remain known;
enemy pieces and settlement ownership/progress disappear immediately outside
vision with no last-known ghost. Check, whose turn it is, clocks, draw state,
and the exact outcome remain public even when their cause is hidden.

In local hot-seat fog play, the board is replaced by an opaque curtain whenever
control changes. Pass the device, then the named player presses Enter. Board
input and both local clocks remain paused through the handoff. Online servers
send only the authenticated seat's projection. An active fog match cannot be
exported with `F8`; after the terminal outcome, explicit export may contain the
complete replay truth for joint review.

## Saves and clocks

Local play has three atomic save slots exposed from the Match menu. Saving
writes the selected slot through a validated temporary file before replacement;
occupied slots require confirmation and a failed write preserves the prior
save. Loading validates file versions, embedded scenario, canonical state, and
hash before changing the match and requires confirmation while a match is
active. Loading does not charge offline time. Unsupported or corrupt saves
remain unchanged and produce a recoverable message. Compatibility details are
in [`compatibility.md`](compatibility.md).

For local clocks, pause/setup/outcome states stop charging. The active clock
continues during mandatory choices. Time is charged before action validation;
expiration at the deadline wins, and increment is added only after an accepted
Move or Hold. Online clocks are server-owned and continue through disconnects.

## Accessibility

- Information never relies on hue alone: terrain has F/M/R marks, fog has
  ?/dim-or-dot/normal states, Keeps N/S,
  settlements -/N/S, owner plates differ in contrast and rotation, and every
  overlay meaning has a unique symbol plus legend text.
- Keyboard-only play covers setup, online rooms, board focus, choices, match
  controls, saves, help, confirmations, and reconnect controls.
- Set UI scale from 0.75 through 2.5 in **Settings > Accessibility**. Window
  dimensions divided by scale must provide at least 800x480 logical pixels;
  unsupported combinations are rejected with an actionable message. Scaled
  menus and panels scroll.
- Enable **Reduced motion** to remove piece interpolation and retirement ghosts
  while retaining immediate results and ordered static feedback.

The reproducible evidence matrix is in
[`accessibility-audit.md`](accessibility-audit.md).

## Known exclusions

The initial release has no online AI, public matchmaking, spectators, player
accounts, factions/asymmetric rules, scenario editor, campaign, web client, or
mobile client. Competitive play is two-player hot-seat or private-room play;
AI is limited to local guided practice and challenges. It also has
no random combat, unit health, resource inventory, worker units, technology
tree or separate economic victory. Hidden information exists only in scenarios
that explicitly enable the fog rules block.

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
