# Crownlines

Crownlines is a deterministic turn-based strategy game combining chess movement
with settlements, promotion sites, and terrain-shaped territorial play. The
authoritative design is in [GDD.md](GDD.md); the ordered delivery backlog begins
at [docs/implementation/00-index.md](docs/implementation/00-index.md).

Players can start with the [player guide](docs/player-guide.md), including local
and private-online quick starts, controls, rules, scenarios, saves, accessibility,
requirements, exclusions, and troubleshooting. Data handling is described in
the [privacy statement](docs/privacy.md).

## Workspace

- `crownline`: Bevy 0.19 desktop client.
- `crownline_core`: deterministic rules and canonical state; no Bevy or I/O.
- `crownline_protocol`: versioned client/server messages.
- `crownline_server`: Tokio/Axum authoritative server.

The project pins Rust 1.95, Bevy 0.19's minimum supported stable compiler.

## Local development

Linux packages commonly required by Bevy:

- Debian/Ubuntu: `g++ pkg-config libx11-dev libasound2-dev libudev-dev`
- Fedora: `gcc-c++ libX11-devel alsa-lib-devel systemd-devel`
- Arch: `base-devel libx11 alsa-lib systemd-libs`

Windows requires the MSVC build tools and Windows SDK. macOS requires Xcode
Command Line Tools.

Run the complete local quality gate:

```sh
./scripts/check.sh
```

Start the desktop shell with `cargo run -p crownline`. Start the server with
`cargo run -p crownline_server`; it listens at `127.0.0.1:5000` by default.
Override that with `CROWNLINE_BIND`, select `CROWNLINE_LOG_FORMAT=json` for JSON
logs, and use `RUST_LOG` for filtering.

Build and run the production container with `docker compose up --build`. The
example binds only `127.0.0.1:5000`; place a TLS-terminating reverse proxy in
front before publishing it. Runtime configuration, health probes, volume
ownership, backup, graceful shutdown, and upgrade steps are documented in the
operations guide.

Database durability, backup, and restore procedures are documented in
[docs/server-operations.md](docs/server-operations.md).
Independent client/server, save, scenario, journal, snapshot, and database
support guarantees are documented in
[docs/compatibility.md](docs/compatibility.md).
