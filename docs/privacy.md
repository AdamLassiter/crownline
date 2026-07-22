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

No consented playtest export or other local file is uploaded automatically.
