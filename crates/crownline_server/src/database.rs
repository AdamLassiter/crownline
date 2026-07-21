use std::{path::Path, time::Duration};

use rusqlite::{Connection, Transaction};
use thiserror::Error;

const CURRENT_SCHEMA_VERSION: i64 = 1;
const MAX_SNAPSHOT_BYTES: i64 = 4 * 1024 * 1024;
const MAX_ACTION_BYTES: i64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    Full,
    Normal,
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema {found} is newer than supported schema {supported}")]
    NewerSchema { found: i64, supported: i64 },
}

pub struct Database {
    connection: Connection,
}

impl Database {
    /// Opens a configured `SQLite` file and runs forward migrations once.
    ///
    /// # Errors
    ///
    /// Returns a SQLite/configuration error or rejects a database from a newer build.
    pub fn open(path: impl AsRef<Path>, durability: Durability) -> Result<Self, DatabaseError> {
        let connection = Connection::open(path)?;
        Self::configure(&connection, durability)?;
        let mut database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    /// Opens an isolated in-memory database with the same constraints as production.
    ///
    /// # Errors
    ///
    /// Returns a SQLite/configuration or migration error.
    pub fn open_in_memory() -> Result<Self, DatabaseError> {
        let connection = Connection::open_in_memory()?;
        Self::configure(&connection, Durability::Full)?;
        let mut database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    /// Returns the current migration version.
    ///
    /// # Errors
    ///
    /// Returns a `SQLite` query error.
    pub fn schema_version(&self) -> Result<i64, DatabaseError> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    fn configure(connection: &Connection, durability: Durability) -> Result<(), DatabaseError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(
            None,
            "synchronous",
            match durability {
                Durability::Full => "FULL",
                Durability::Normal => "NORMAL",
            },
        )?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<(), DatabaseError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_unix_millis INTEGER NOT NULL CHECK(applied_unix_millis >= 0)
            );",
        )?;
        let found: i64 = self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        if found > CURRENT_SCHEMA_VERSION {
            return Err(DatabaseError::NewerSchema {
                found,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if found < 1 {
            let transaction = self.connection.transaction()?;
            migration_001(&transaction)?;
            transaction.execute(
                "INSERT INTO schema_migrations(version, name, applied_unix_millis)
                 VALUES(1, 'initial_authoritative_schema', unixepoch('subsec') * 1000)",
                [],
            )?;
            transaction.commit()?;
        }
        Ok(())
    }
}

fn migration_001(transaction: &Transaction<'_>) -> Result<(), rusqlite::Error> {
    transaction.execute_batch(&format!(
        "CREATE TABLE rooms (
            code TEXT PRIMARY KEY CHECK(length(code) = 6),
            scenario_id TEXT NOT NULL,
            scenario_hash TEXT NOT NULL CHECK(length(scenario_hash) = 64),
            lifecycle TEXT NOT NULL CHECK(lifecycle IN ('waiting_for_opponent', 'waiting_for_ready', 'playing', 'finished')),
            base_minutes INTEGER CHECK(base_minutes BETWEEN 1 AND 180),
            increment_seconds INTEGER CHECK(increment_seconds BETWEEN 0 AND 60),
            created_unix_millis INTEGER NOT NULL CHECK(created_unix_millis >= 0),
            updated_unix_millis INTEGER NOT NULL CHECK(updated_unix_millis >= created_unix_millis),
            CHECK((base_minutes IS NULL) = (increment_seconds IS NULL))
        );

        CREATE TABLE seats (
            room_code TEXT NOT NULL REFERENCES rooms(code) ON DELETE CASCADE,
            player TEXT NOT NULL CHECK(player IN ('north', 'south')),
            display_name TEXT NOT NULL CHECK(length(trim(display_name)) BETWEEN 1 AND 24),
            token_hash BLOB NOT NULL UNIQUE CHECK(length(token_hash) = 32),
            ready INTEGER NOT NULL DEFAULT 0 CHECK(ready IN (0, 1)),
            PRIMARY KEY(room_code, player)
        ) WITHOUT ROWID;

        CREATE TABLE matches (
            match_id TEXT PRIMARY KEY CHECK(length(match_id) = 36),
            room_code TEXT NOT NULL REFERENCES rooms(code) ON DELETE RESTRICT,
            revision INTEGER NOT NULL CHECK(revision >= 0),
            state_hash TEXT NOT NULL CHECK(length(state_hash) = 64),
            current_snapshot_revision INTEGER NOT NULL CHECK(current_snapshot_revision = revision),
            clock_anchor_unix_millis INTEGER CHECK(clock_anchor_unix_millis >= 0),
            deadline_unix_millis INTEGER CHECK(deadline_unix_millis >= clock_anchor_unix_millis),
            outcome_json TEXT,
            started_unix_millis INTEGER NOT NULL CHECK(started_unix_millis >= 0),
            updated_unix_millis INTEGER NOT NULL CHECK(updated_unix_millis >= started_unix_millis),
            UNIQUE(room_code, match_id),
            FOREIGN KEY(match_id, current_snapshot_revision)
                REFERENCES match_snapshots(match_id, revision)
                DEFERRABLE INITIALLY DEFERRED
        );

        CREATE TABLE match_snapshots (
            match_id TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
            revision INTEGER NOT NULL CHECK(revision >= 0),
            state_hash TEXT NOT NULL CHECK(length(state_hash) = 64),
            snapshot_json BLOB NOT NULL CHECK(length(snapshot_json) BETWEEN 1 AND {MAX_SNAPSHOT_BYTES}),
            clock_anchor_unix_millis INTEGER CHECK(clock_anchor_unix_millis >= 0),
            deadline_unix_millis INTEGER CHECK(deadline_unix_millis >= clock_anchor_unix_millis),
            outcome_json TEXT,
            created_unix_millis INTEGER NOT NULL CHECK(created_unix_millis >= 0),
            PRIMARY KEY(match_id, revision)
        ) WITHOUT ROWID;

        CREATE TABLE actions (
            match_id TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
            revision_before INTEGER NOT NULL CHECK(revision_before >= 0),
            revision_after INTEGER NOT NULL CHECK(revision_after = revision_before + 1),
            idempotency_key BLOB NOT NULL CHECK(length(idempotency_key) = 16),
            actor TEXT NOT NULL CHECK(actor IN ('north', 'south')),
            received_unix_millis INTEGER NOT NULL CHECK(received_unix_millis >= 0),
            decided_unix_millis INTEGER NOT NULL CHECK(decided_unix_millis >= received_unix_millis),
            action_json BLOB NOT NULL CHECK(length(action_json) BETWEEN 1 AND {MAX_ACTION_BYTES}),
            events_json BLOB NOT NULL CHECK(length(events_json) BETWEEN 1 AND {MAX_ACTION_BYTES}),
            state_hash TEXT NOT NULL CHECK(length(state_hash) = 64),
            PRIMARY KEY(match_id, revision_after),
            UNIQUE(match_id, idempotency_key),
            FOREIGN KEY(match_id, revision_after)
                REFERENCES match_snapshots(match_id, revision)
                DEFERRABLE INITIALLY DEFERRED
        ) WITHOUT ROWID;

        CREATE INDEX rooms_lifecycle_updated_idx ON rooms(lifecycle, updated_unix_millis);
        CREATE INDEX matches_room_idx ON matches(room_code);
        CREATE INDEX matches_outcome_idx ON matches(outcome_json);
        CREATE INDEX actions_match_revision_idx ON actions(match_id, revision_after);"
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::params;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn fresh_file_migrates_once_and_enables_declared_durability_pragmas() {
        let path = std::env::temp_dir().join(format!("crownline-{}.sqlite3", Uuid::new_v4()));
        {
            let mut database = Database::open(&path, Durability::Full).unwrap();
            assert_eq!(database.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
            database.migrate().unwrap();
            let migrations: i64 = database
                .connection()
                .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(migrations, 1);
            assert_eq!(
                database
                    .connection()
                    .pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                database
                    .connection()
                    .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
                    .unwrap(),
                2
            );
        }
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    #[test]
    fn foreign_keys_uniqueness_revision_and_secret_hash_constraints_hold() {
        let database = Database::open_in_memory().unwrap();
        let connection = database.connection();
        connection.execute(
            "INSERT INTO rooms(code, scenario_id, scenario_hash, lifecycle, created_unix_millis, updated_unix_millis)
             VALUES('ABC234', 'scenario', ?1, 'waiting_for_ready', 1, 1)",
            ["a".repeat(64)],
        ).unwrap();
        connection.execute(
            "INSERT INTO seats(room_code, player, display_name, token_hash) VALUES('ABC234', 'north', 'Ada', ?1)",
            [vec![7_u8; 32]],
        ).unwrap();
        assert!(connection.execute(
            "INSERT INTO seats(room_code, player, display_name, token_hash) VALUES('ABC234', 'south', 'Grace', ?1)",
            [vec![7_u8; 32]],
        ).is_err());
        assert!(connection.execute(
            "INSERT INTO seats(room_code, player, display_name, token_hash) VALUES('MISSING', 'south', 'Grace', ?1)",
            [vec![8_u8; 32]],
        ).is_err());

        let match_id = Uuid::new_v4().to_string();
        connection.execute_batch("BEGIN DEFERRED").unwrap();
        connection.execute(
            "INSERT INTO matches(match_id, room_code, revision, state_hash, current_snapshot_revision, started_unix_millis, updated_unix_millis)
             VALUES(?1, 'ABC234', 0, ?2, 0, 1, 1)",
            params![&match_id, "b".repeat(64)],
        ).unwrap();
        connection.execute(
            "INSERT INTO match_snapshots(match_id, revision, state_hash, snapshot_json, created_unix_millis)
             VALUES(?1, 0, ?2, '{}', 1)",
            params![&match_id, "b".repeat(64)],
        ).unwrap();
        connection.execute_batch("COMMIT").unwrap();
        assert!(
            connection
                .execute(
                    "UPDATE matches SET revision = 1 WHERE match_id = ?1",
                    [&match_id],
                )
                .is_err()
        );
    }

    #[test]
    fn newer_schema_is_rejected_without_applying_changes() {
        let mut database = Database::open_in_memory().unwrap();
        database.connection().execute(
            "INSERT INTO schema_migrations(version, name, applied_unix_millis) VALUES(99, 'future', 1)",
            [],
        ).unwrap();
        assert!(matches!(
            database.migrate(),
            Err(DatabaseError::NewerSchema { found: 99, .. })
        ));
        assert_eq!(
            database
                .connection()
                .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
