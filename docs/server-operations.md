# Crownlines server operations

## Container deployment

Build the multi-stage image with `docker build -t crownline-server .`, or start
the supplied single-service deployment with `docker compose up --build -d`.
The final image contains the release server, CA certificates, and the health
probe client, but no Rust compiler, source tree, or build cache. It runs as the
dedicated unprivileged `crownline` user (UID/GID 10001).

The compose example publishes `127.0.0.1:5000` only. Put an HTTPS reverse proxy
on the same host and proxy both HTTP and WebSocket upgrades to that address.
Set `CROWNLINE_PUBLIC_URL` to the externally visible `https://` URL; it is
validated and logged at startup for deployment diagnostics, while
`CROWNLINE_BIND` controls the actual listening socket. Do not expose an
unencrypted public endpoint because reconnect tokens authorize seats.

The named `crownline-data` volume is mounted at `/var/lib/crownline`, matching
the image defaults. If using a bind mount instead, create it as UID/GID 10001
with mode `0700`. Keep the database, `-wal`, and `-shm` files on that same local
filesystem.

Relevant environment variables are:

- `CROWNLINE_BIND` (image default `0.0.0.0:5000`).
- `CROWNLINE_PUBLIC_URL` (optional external `http://` or `https://` URL).
- `CROWNLINE_DATABASE_PATH` (image default
  `/var/lib/crownline/crownline.sqlite3`).
- `CROWNLINE_DATABASE_DURABILITY` (`full` or `normal`).
- `CROWNLINE_LOG_FORMAT` (`json` in the image, or `pretty`).
- `RUST_LOG` for module-level filtering.
- `CROWNLINE_SHUTDOWN_SECONDS` (1-300, default 15) for bounded connection drain.
- The bounded request, room, connection, and idle-lobby settings named in
  `crates/crownline_server/src/limits.rs`.

Logs go to stdout/stderr and should be collected by the container runtime. They
do not contain raw reconnect credentials. The image declares `SIGTERM` as its
stop signal. On shutdown the listener stops accepting, active requests and
WebSockets drain for the configured bound, remaining connections close, and
dropping the application closes the SQLite connection. Set the orchestrator
grace period above `CROWNLINE_SHUTDOWN_SECONDS`; the compose example allows 20
seconds for the default 15-second drain.

## Health checks

- `GET /health/live` reports process/event-loop liveness and does not touch the
  database.
- `GET /health/ready` (also `GET /health`) reports liveness and database
  readiness as separate JSON fields. It returns HTTP 503 when the schema or
  quick integrity query is unavailable.

The container health check uses the readiness endpoint. Load balancers should
use readiness for traffic routing and liveness only for restart decisions.

## SQLite storage

The server opens `CROWNLINE_DATABASE_PATH` (default `crownline.sqlite3`) with
foreign keys and WAL journaling enabled. `CROWNLINE_DATABASE_DURABILITY` accepts
`full` (default) or `normal`. Production deployments should put the database and
its WAL files on the same persistent volume and should use `full` unless measured
write throughput requires the weaker durability tradeoff.

Startup applies forward migrations and validates every unfinished match before
the server accepts traffic. A corrupt match is quarantined and reported by match
ID and reason code; healthy matches continue to restore. Never edit migration or
match tables manually while the server is running.

## Backup

Use SQLite's online backup command so the main database and WAL form one
consistent image. Do not copy only the live `.sqlite3` file while WAL mode is
active.

```sh
sqlite3 crownline.sqlite3 ".timeout 5000" ".backup crownline-backup.sqlite3"
sqlite3 crownline-backup.sqlite3 "PRAGMA integrity_check;"
```

Keep backups outside the live volume, restrict them like the primary database,
and record the application version that created each backup. Backups contain
hashed reconnect credentials, match state, and player display names, but never
raw reconnect tokens.

## Restore

1. Stop the server and retain the current database plus `-wal`/`-shm` files as a
   rollback copy.
2. Run `PRAGMA integrity_check` against the selected backup.
3. Copy the backup to the configured database path using the server user's
   ownership and restrictive permissions. Remove stale WAL/SHM files belonging
   to the replaced database.
4. Start the same or a newer compatible server build. Startup migrations and
   canonical replay validation must complete before traffic is accepted.
5. Confirm the health endpoint and recovery logs, then test an authenticated
   reconnect to an unfinished match.

Never restore a database produced by a newer schema into an older server build;
startup rejects it without applying changes.

## Upgrade

1. Build or pull the candidate image and review its migration and release notes.
2. Take and integrity-check an online backup before replacing the running image.
3. Send `SIGTERM` and wait for the bounded graceful drain to finish. Do not run
   old and new containers against the same SQLite volume concurrently.
4. Start the candidate with the existing volume. Forward migrations and match
   replay validation finish before the listener is created.
5. Require `/health/ready` to return HTTP 200, inspect migration/recovery logs,
   and test an authenticated reconnect before restoring public traffic.
6. To roll back across a schema change, stop the candidate and restore the
   pre-upgrade backup; never open a newer schema with an older binary.

Release CI scans locked dependencies and the built runtime image for high and
critical known vulnerabilities. Review base-image updates and scan exceptions
as part of every release rather than suppressing unfixed findings silently.
