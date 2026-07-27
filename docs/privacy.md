# Crownlines privacy statement

Crownlines 0.1 does not include analytics, advertising, crash reporting,
telemetry, player accounts, public matchmaking, or an operator-controlled client
tracking identifier. The project does not send data anywhere unless a player
explicitly joins a configured private server.

## Local data

The client stores settings, local player display names, three optional save
slots, local match state/history, and saved online-seat metadata in the operating
system's per-user Crownlines configuration/data directories. A saved online seat
contains server address, room code, match ID, seat, and a random credential
locator. The actual reconnect credential is stored in the operating-system
credential service when available; fallback files are user-private and mode
`0600` on Unix.

Local saves and settings remain under the player's control. Deleting them removes
the local data, but deleting a reconnect credential also permanently removes
that client's ability to prove ownership of the seat.

## Online data

Joining or hosting a private room sends the configured server the player display
name, selected scenario/clock settings, room code, protocol messages, and the
seat credential needed for authentication. The authoritative server persists
display names, match state/actions, clocks, room metadata, and a cryptographic
hash of each credential. It does not persist raw reconnect tokens.

For fog-enabled rooms, the server still retains complete canonical match state
and exploration for authority, recovery, and replay. Protocol 3 authenticates a
seat before producing any match view and sends that client only its projection
and projection hash; it does not send the canonical state or canonical hash.
North and South payloads are independently constructed. Operators must treat
the canonical database and backups as hidden match data even though connected
clients receive redacted views.

Server operators control their own database, logs, backups, retention, network
address, and legal obligations. Ask the operator before sending information you
do not want retained. Crownlines logs are designed not to contain raw reconnect
credentials, but logs and backups should still be treated as private operational
data.

## Explicit system access

- Copying a room invitation uses the clipboard only after the player presses the
  copy command. Invitation text contains the room code and optionally server
  address, never the reconnect credential.
- Network access goes only to the player-configured Crownlines server. Remote
  connections require TLS; loopback development may use plaintext.
- The client reads/writes only its bundled assets and platform-specific settings,
  save, and credential locations during normal operation.

Pressing `F8` during a local match explicitly writes a structured playtest JSON
to the platform data directory. It contains application/scenario identity,
canonical action/event/hash records, timing and balance counts, outcome, and
blank qualitative-review fields. It excludes player names, credentials, room
codes, server addresses, settings, and save contents. No playtest export or
other local file is uploaded automatically.
During an active fog match this export is blocked because it would reveal both
seats' hidden truth. After the match ends, `F8` is an explicit full-truth replay
export intended for joint review by both players.
