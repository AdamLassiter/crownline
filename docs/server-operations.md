# Crownlines server operations

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
