# Accessibility and readability audit

This audit records the stable visual semantics and input/layout invariants used
by the desktop client. It was run on 2026-07-22 against the worktree based on
`6358cb7`, Rust 1.95.0, and Bevy 0.19.0.

Run the automated evidence with:

```sh
cargo test -p crownline
cargo clippy -p crownline --all-targets --all-features -- -D warnings
```

The audit uses semantic entity/style inspection and a monochrome luminance
simulation rather than pixel snapshots. This makes the gate independent of GPU,
font rasterizer, and anti-aliasing differences while checking the actual marks,
text, layout bounds, and reducer input paths shipped by the client.

## Non-hue visual semantics

Hue is supplementary. Removing hue leaves the following independent cues:

| Information | Non-hue cue |
| --- | --- |
| Board parity | Alternating light/dark luminance on every terrain. |
| Terrain | Open is unmarked; Forest, Mountain, and Road carry `F`, `M`, and `R` corner marks. Their light/dark palettes also remain separated in simulated monochrome. |
| Piece kind | Distinct Unicode King, Queen, Rook, Bishop, Knight, and Pawn silhouettes. |
| Piece owner | North uses a pale glyph on an upright dark plate; South uses a dark glyph on a rotated pale plate. |
| Keep owner | Every inset Keep tile carries an explicit `N` or `S`. |
| Settlement owner | The ring carries `·`, `N`, or `S`; South's ring is additionally rotated. |
| Promotion and fortification | Promotion sites use a crossed mark; fortifications use `T`. |
| Legal/attack/check state | All 12 overlay meanings have unique symbols, including `•`, `×`, `!`, `G`, `#`, gained/lost signs, selection, and illegal destination. |
| Progress and warnings | Settlement ownership, founder, governors, blockers, establishment/production fractions, readiness, clocks, low-clock warning, check, phase, and selected piece are present as text. |

Regression tests enumerate every terrain and overlay symbol, inspect both owner
styles, verify Keep/settlement marks, enforce monochrome terrain separation, and
check the textual check/clock/progress surfaces.

## Keyboard-only operation

Every match and menu transition has a visible keyboard path:

- Setup uses Tab-visible editable fields, `X` color/name swap, PageUp/PageDown
  scenarios, `C` clock toggle, `-`/`+` base time, `,`/`.` increment, `F2` local
  start, and `F3` online.
- Online menus display `H` host, `J` join, Tab fields, Enter submit, Escape back,
  `R` ready, `C` invitation copy, and `A` address inclusion.
- Board focus is a visible square driven by arrows; Enter selects/confirms, Escape
  releases, and `H` holds. Mandatory placement cycles only legal squares; mandatory
  promotion exposes numbered `1`-`4` choices and queue position.
- Match lifecycle exposes `P` pause/resume, `Q` then Enter/Escape resignation,
  `D` draw offer, `Y`/`N` response, `R` rematch, `F5` save, `F6` slot, and `F9`
  load. `I` collapses/expands information panels.
- `F1` opens help, `1`-`5` navigates every help section, Escape closes it, and
  the active section receives a distinct selected background in addition to its
  heading.

Focused tests cover editable/tab controls, board-focus clamping and release,
selection/confirmation, forced choices, lifecycle confirmations, online ready
gating, panel collapse, and help keyboard navigation with a visible selected
state. Pointer capture remains a separate tested path and cannot activate the
board through UI.

## Scale and common resolutions

`ui_scale` is installed as Bevy's global `UiScale`; Bug 008 records the prior
missing connection. Configuration accepts 0.75-2.5 only when the selected window
still provides at least 800x480 logical UI pixels. The audit matrix covers:

| Window | UI scale | Logical UI area |
| --- | ---: | ---: |
| 640x480 | 0.75 | 853x640 |
| 800x600 | 1.0 | 800x600 |
| 1280x720 | 1.25 | 1024x576 |
| 1920x1080 | 2.0 | 960x540 |
| 2560x1440 | 2.5 | 1024x576 |

An impossible combination such as 640x480 at 2.5 is rejected with the required
logical-size message instead of silently clipping. Setup, pause/outcome, online
lobby, help content, and information panels use bounded or vertical-scroll
surfaces; panel minimum width no longer forces left/right overlap at the audited
logical minimum. Board fitting remains independently tested at 800x600,
1280x720, 1920x1080, and 2560x1440 for every authored board size.

## Reduced motion

Reduced motion skips piece interpolation and captured/promotion retirement
ghosts. Camera movement is direct and has no forced tween or shake. Canonical
entities still update immediately, and transition feedback remains as ordered,
static notices plus the durable history log. A combined regression proves zero
tweens/ghosts while establishment, Pawn production, and turn-start feedback is
retained in canonical event order.

## Result

All Task 10.03.02 criteria pass. Future visual changes must preserve these
semantic/state tests; a stable regression should be added whenever a new visual
meaning, control, scale boundary, or motion path is introduced.
