# Fog-of-war rules contract

This contract defines the optional Crownlines hidden-information variant. It is
the shared reference for core state, seat projections, local presentation, and
online protocol work. Until those later boundaries are implemented, shipped
scenarios remain perfect-information games.

## Scenario configuration

Fog is disabled when `rules.fog` is omitted or explicitly `None`. An enabled
scenario uses the independently versioned rules block:

```ron
fog: Some((schema_version: 1, vision_radius: 3)),
```

`vision_radius` is a positive whole-square radius. It must be no greater than
`max(board.width, board.height) - 1`; validation rejects zero, values which
cannot fit the board, arithmetic-overflow-sized values, and unsupported nested
schema versions. Radius 3 is the first tuning baseline, not a hard-coded
default. The optional field is omitted from canonical serialization when fog is
disabled, preserving existing perfect-information scenario hashes.

The nested version is the extension point for later visibility models. A
line-of-sight or blocker-aware model must use a new version or an explicitly
versioned replacement; it must not reinterpret version 1.

## Deterministic visibility

Visibility is independent for North and South. For player `p`, radius `r`, a
living friendly piece at `a`, and board coordinate `b`:

```text
distance(a, b) = max(abs(a.x - b.x), abs(a.y - b.y))
b is Visible(p) iff any living piece owned by p has distance(a, b) <= r
```

This is Chebyshev distance: every piece sees a square area, clipped to the
authored board. Terrain, pieces, Keeps, settlements, promotion sites,
fortifications, walls, rivers, and all other edges do not block vision in
version 1. Implementations calculate with absolute differences rather than
coordinate addition, avoiding edge overflow. Coordinate collections are
deduplicated and ordered by canonical `Coord` order: `x`, then `y`.

Each seat classifies every coordinate as exactly one of:

- `Undiscovered`: never in that seat's visible set.
- `Explored`: visible at least once, but outside current vision.
- `Visible`: inside current vision now. Visibility also permanently explores
  the coordinate.

If `V(p)` is current visibility and `E(p)` is durable exploration, an update is
`E'(p) = E(p) union V(p)`. A coordinate is `Visible` when in `V`, `Explored`
when in `E'` but not `V`, and otherwise `Undiscovered`.

## Knowledge classification

| Fact | Seat disclosure |
| --- | --- |
| Scenario identity, board dimensions, rules, active player | Always public. |
| Terrain at a coordinate | Revealed when the coordinate is first visible; permanent thereafter. Omitted terrain means Open and follows the same rule. |
| Settlement, promotion-site, Keep-tile, or fortification identity/location | Revealed when its coordinate is first visible; permanent thereafter. |
| Edge kind and static gate/wall relationship | Revealed permanently when either endpoint is first visible. It reveals no other fact about the unseen endpoint. |
| Friendly pieces | Always current and visible. Every living friendly piece sees its own coordinate. |
| Enemy piece identity, kind, and coordinate | Current only while its coordinate is visible; no last-known ghost remains afterward. |
| Settlement owner, founder, governors, continuity, establishment, production, placement support, or contested state | Current only while that settlement coordinate is visible. |
| Promotion candidate, continuity, batch eligibility, and choice details | Current only to its owner or while the candidate's coordinate is visible. Another seat may see only the public fact that the active player is resolving a private mandatory choice. |
| Check, active seat, clocks, draw offer/state, resignation, terminal outcome, winner, and exact outcome reason | Always public, even when their cause is hidden. |

Static knowledge is a fact about authored board geometry. Dynamic ownership,
progress, piece, and queue facts never become permanent knowledge merely
because their coordinate was explored. A seat projection must omit the fact,
not substitute a stale or sentinel value that implies it still exists.

## Update boundaries

- At match creation, both exploration sets begin empty. Construct the complete
  canonical position, calculate starting visibility, merge it into exploration,
  and only then produce the first seat view.
- After an accepted action, finish the complete canonical transition first,
  including movement/capture, castling endpoints, promotion, production,
  Pawn placement, settlement transfer, realm cycles, clocks, mandatory-choice
  changes, check, and outcome. Then recalculate visibility from surviving final
  piece coordinates and merge exploration before emitting a view.
- Movement paths and transient intermediate squares do not grant vision.
  Captured pieces provide no post-transition vision. A promoted piece sees from
  the Pawn's final square. A produced Pawn grants vision only after its placement
  action creates it on the board.
- A rejected intent changes neither canonical state nor exploration, consumes no
  turn, and is reported only to its submitting seat through the opaque rejection
  described below.
- Rematch creates fresh match identities and empty exploration before applying
  the new starting visibility.
- Save/load and reconnect restore or deterministically replay durable exploration,
  validate it against the board, and recompute only current visibility from the
  restored canonical pieces. Replay at revision zero follows match creation;
  each later accepted record follows the same post-transition boundary.

## Action-result inference boundary

The authoritative reducer continues to use complete canonical state. A seat may
submit an intent naming its own piece and destination or mandatory choice. It
receives either an accepted seat-safe snapshot/events response or one stable
opaque `illegal_intent` rejection. Detailed blockers, attack lines, check
causes, and canonical legal alternatives are not returned. Rejections are not
broadcast to the other seat and do not consume a turn.

The design accepts the limited inference produced by trying an intent and
learning whether it was accepted. It does not permit clients, overlays,
diagnostics, exports, transition logs, or protocol payloads to receive canonical
legal-move lists or unredacted action results as a shortcut. Any client preview
must be derived from the viewing seat's projection and must not claim that an
unseen route is legal.
