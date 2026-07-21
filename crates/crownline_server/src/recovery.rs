use std::time::{SystemTime, UNIX_EPOCH};

use crownline_core::{ClockSettings, scenario::Player};
use crownline_protocol::MatchSnapshot;
use rusqlite::params;
use thiserror::Error;
use tracing::error;
use uuid::Uuid;

use crate::{
    authority::{AuthoritativeMatch, PreparedAuthorityTransition},
    database::{Database, DatabaseError},
    rooms::ScenarioCatalog,
};

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("SQLite persistence operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("authority persistence JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("match is not registered")]
    MissingMatch,
    #[error("server wall clock is invalid")]
    InvalidWallClock,
    #[error("canonical state could not be hashed")]
    InvalidState,
}

pub struct RestoredMatch {
    pub match_id: Uuid,
    pub authority: AuthoritativeMatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantinedMatch {
    pub match_id: String,
    pub reason_code: &'static str,
}

pub struct RestoreReport {
    pub matches: Vec<RestoredMatch>,
    pub quarantined: Vec<QuarantinedMatch>,
}

pub struct MatchRepository {
    database: Database,
}

impl MatchRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Registers a started room and its initial authority image atomically.
    ///
    /// # Errors
    ///
    /// Returns a serialization or constrained `SQLite` transaction error.
    pub fn register_match(
        &mut self,
        room_code: &str,
        scenario_hash: &str,
        clock_settings: Option<ClockSettings>,
        image: &PreparedAuthorityTransition,
    ) -> Result<(), RecoveryError> {
        let bytes = serde_json::to_vec(image)?;
        let state_hash = image
            .state
            .canonical_hash()
            .map_err(|_| RecoveryError::InvalidState)?;
        let outcome = encode_outcome(&image.state)?;
        let transaction = self.database.connection_mut().transaction()?;
        transaction.execute(
            "INSERT INTO rooms(code, scenario_id, scenario_hash, lifecycle, base_minutes, increment_seconds, created_unix_millis, updated_unix_millis)
             VALUES(?1, ?2, ?3, 'playing', ?4, ?5, ?6, ?6)",
            params![
                room_code,
                image.state.scenario_id,
                scenario_hash,
                clock_settings.map(|settings| settings.base_minutes),
                clock_settings.map(|settings| settings.increment_seconds),
                image.received_unix_millis,
            ],
        )?;
        transaction.execute(
            "INSERT INTO matches(match_id, room_code, revision, state_hash, current_snapshot_revision,
                                  clock_anchor_unix_millis, deadline_unix_millis, outcome_json,
                                  started_unix_millis, updated_unix_millis)
             VALUES(?1, ?2, ?3, ?4, ?3, ?5, ?6, ?7, ?8, ?9)",
            params![
                image.match_id.to_string(),
                room_code,
                image.state.revision,
                state_hash,
                image.clock.as_ref().map(|clock| clock.anchor_unix_millis),
                image.clock.as_ref().and_then(|clock| clock.deadline_unix_millis),
                outcome,
                image.received_unix_millis,
                image.decided_unix_millis,
            ],
        )?;
        insert_snapshot(&transaction, image, &bytes)?;
        transaction.commit()?;
        Ok(())
    }

    /// Commits snapshot, journal action, clock/deadline, revision, and outcome together.
    ///
    /// # Errors
    ///
    /// Returns a serialization, missing-match, or constrained `SQLite` transaction error.
    pub fn commit_transition(
        &mut self,
        transition: &PreparedAuthorityTransition,
    ) -> Result<MatchSnapshot, RecoveryError> {
        let bytes = serde_json::to_vec(transition)?;
        let state_hash = transition
            .state
            .canonical_hash()
            .map_err(|_| RecoveryError::InvalidState)?;
        let outcome = encode_outcome(&transition.state)?;
        let transaction = self.database.connection_mut().transaction()?;
        insert_snapshot(&transaction, transition, &bytes)?;
        if let Some(record) = transition.journal.records.last()
            && record.revision_after == transition.state.revision
        {
            let action = serde_json::to_vec(&record.action)?;
            let events = serde_json::to_vec(&record.events)?;
            transaction.execute(
                "INSERT INTO actions(match_id, revision_before, revision_after, idempotency_key,
                                     actor, received_unix_millis, decided_unix_millis,
                                     action_json, events_json, state_hash)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    transition.match_id.to_string(),
                    record.revision_before,
                    record.revision_after,
                    record.idempotency_key.0.as_slice(),
                    player_label(record.actor),
                    transition.received_unix_millis,
                    transition.decided_unix_millis,
                    action,
                    events,
                    record.state_hash,
                ],
            )?;
        }
        let changed = transaction.execute(
            "UPDATE matches
             SET revision = ?2, state_hash = ?3, current_snapshot_revision = ?2,
                 clock_anchor_unix_millis = ?4, deadline_unix_millis = ?5,
                 outcome_json = ?6, updated_unix_millis = ?7
             WHERE match_id = ?1",
            params![
                transition.match_id.to_string(),
                transition.state.revision,
                state_hash,
                transition
                    .clock
                    .as_ref()
                    .map(|clock| clock.anchor_unix_millis),
                transition
                    .clock
                    .as_ref()
                    .and_then(|clock| clock.deadline_unix_millis),
                outcome,
                transition.decided_unix_millis,
            ],
        )?;
        if changed != 1 {
            return Err(RecoveryError::MissingMatch);
        }
        let scenario_hash: String = transaction.query_row(
            "SELECT r.scenario_hash FROM matches m JOIN rooms r ON r.code = m.room_code WHERE m.match_id = ?1",
            [transition.match_id.to_string()],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(snapshot_from_transition(
            transition,
            scenario_hash,
            state_hash,
        ))
    }

    /// Restores all healthy unfinished matches and quarantines corrupt rows independently.
    ///
    /// # Errors
    ///
    /// Returns only database-level failures; individual corrupt matches are reported and skipped.
    pub fn restore_active(
        &mut self,
        catalog: &ScenarioCatalog,
    ) -> Result<RestoreReport, RecoveryError> {
        let rows = {
            let mut statement = self.database.connection().prepare(
                "SELECT m.match_id, m.revision, m.state_hash, s.snapshot_json,
                        r.scenario_id, r.scenario_hash
                 FROM matches m
                 JOIN rooms r ON r.code = m.room_code
                 JOIN match_snapshots s
                   ON s.match_id = m.match_id AND s.revision = m.current_snapshot_revision
                 LEFT JOIN quarantined_matches q ON q.match_id = m.match_id
                 WHERE m.outcome_json IS NULL AND q.match_id IS NULL
                 ORDER BY m.match_id",
            )?;
            statement
                .query_map([], |row| {
                    Ok(RawRestoreRow {
                        match_id: row.get(0)?,
                        revision: row.get(1)?,
                        state_hash: row.get(2)?,
                        snapshot_json: row.get(3)?,
                        scenario_id: row.get(4)?,
                        scenario_hash: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut report = RestoreReport {
            matches: Vec::new(),
            quarantined: Vec::new(),
        };
        for row in rows {
            match restore_row(catalog, &row) {
                Ok(restored) => report.matches.push(restored),
                Err(reason_code) => {
                    error!(match_id = %row.match_id, reason_code, "quarantined corrupt match");
                    self.quarantine(&row.match_id, reason_code)?;
                    report.quarantined.push(QuarantinedMatch {
                        match_id: row.match_id,
                        reason_code,
                    });
                }
            }
        }
        Ok(report)
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    fn quarantine(&mut self, match_id: &str, reason_code: &str) -> Result<(), RecoveryError> {
        self.database.connection().execute(
            "INSERT OR REPLACE INTO quarantined_matches(match_id, reason_code, quarantined_unix_millis)
             VALUES(?1, ?2, ?3)",
            params![match_id, reason_code, now_unix_millis()?],
        )?;
        Ok(())
    }
}

struct RawRestoreRow {
    match_id: String,
    revision: u64,
    state_hash: String,
    snapshot_json: Vec<u8>,
    scenario_id: String,
    scenario_hash: String,
}

fn restore_row(
    catalog: &ScenarioCatalog,
    row: &RawRestoreRow,
) -> Result<RestoredMatch, &'static str> {
    let match_id = Uuid::parse_str(&row.match_id).map_err(|_| "invalid_match_id")?;
    let installed = catalog
        .get(&row.scenario_id)
        .ok_or("scenario_not_installed")?;
    if installed.hash != row.scenario_hash {
        return Err("scenario_hash_mismatch");
    }
    let persisted: PreparedAuthorityTransition =
        serde_json::from_slice(&row.snapshot_json).map_err(|_| "snapshot_json_invalid")?;
    if persisted.match_id != match_id || persisted.state.revision != row.revision {
        return Err("snapshot_revision_mismatch");
    }
    let actual_hash = persisted
        .state
        .canonical_hash()
        .map_err(|_| "snapshot_hash_failed")?;
    if actual_hash != row.state_hash {
        return Err("snapshot_hash_mismatch");
    }
    let authority = AuthoritativeMatch::restore(installed.definition.clone(), persisted)
        .map_err(|_| "journal_replay_mismatch")?;
    Ok(RestoredMatch {
        match_id,
        authority,
    })
}

fn insert_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    transition: &PreparedAuthorityTransition,
    bytes: &[u8],
) -> Result<(), RecoveryError> {
    let state_hash = transition
        .state
        .canonical_hash()
        .map_err(|_| RecoveryError::InvalidState)?;
    transaction.execute(
        "INSERT INTO match_snapshots(match_id, revision, state_hash, snapshot_json,
                                     clock_anchor_unix_millis, deadline_unix_millis,
                                     outcome_json, created_unix_millis)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            transition.match_id.to_string(),
            transition.state.revision,
            state_hash,
            bytes,
            transition
                .clock
                .as_ref()
                .map(|clock| clock.anchor_unix_millis),
            transition
                .clock
                .as_ref()
                .and_then(|clock| clock.deadline_unix_millis),
            encode_outcome(&transition.state)?,
            transition.decided_unix_millis,
        ],
    )?;
    Ok(())
}

fn snapshot_from_transition(
    transition: &PreparedAuthorityTransition,
    scenario_hash: String,
    state_hash: String,
) -> MatchSnapshot {
    MatchSnapshot {
        match_id: transition.match_id,
        revision: transition.state.revision,
        scenario_id: transition.state.scenario_id.clone(),
        scenario_hash,
        state_hash,
        state: transition.state.clone(),
        room_state: crownline_protocol::ConnectionState::Connected,
    }
}

fn encode_outcome(state: &crownline_core::MatchState) -> Result<Option<String>, serde_json::Error> {
    state
        .outcome
        .map(|outcome| serde_json::to_string(&outcome))
        .transpose()
}

const fn player_label(player: Player) -> &'static str {
    match player {
        Player::North => "north",
        Player::South => "south",
    }
}

fn now_unix_millis() -> Result<u64, RecoveryError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RecoveryError::InvalidWallClock)?
        .as_millis();
    u64::try_from(millis).map_err(|_| RecoveryError::InvalidWallClock)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crownline_core::Action;

    use crate::{
        actors::{CommandTiming, MatchExecutor},
        database::Durability,
    };

    use super::*;

    fn fixture(
        clock: Option<ClockSettings>,
        started: SystemTime,
    ) -> (ScenarioCatalog, AuthoritativeMatch, String) {
        let catalog = ScenarioCatalog::installed();
        let installed = catalog.get("crownlines-standard").unwrap();
        let authority =
            AuthoritativeMatch::new(Uuid::new_v4(), installed.definition.clone(), clock, started)
                .unwrap();
        let hash = installed.hash.clone();
        (catalog, authority, hash)
    }

    #[test]
    fn snapshot_journal_clock_and_revision_commit_in_one_transaction() {
        let started = UNIX_EPOCH.checked_add(Duration::from_mins(500)).unwrap();
        let settings = ClockSettings {
            base_minutes: 5,
            increment_seconds: 2,
        };
        let (_catalog, mut authority, scenario_hash) = fixture(Some(settings), started);
        let initial = authority.persistence_image(started).unwrap();
        let mut repository = MatchRepository::new(Database::open_in_memory().unwrap());
        repository
            .register_match("ABC234", &scenario_hash, Some(settings), &initial)
            .unwrap();
        let received = started.checked_add(Duration::from_secs(4)).unwrap();
        authority
            .execute(
                Uuid::new_v4(),
                Player::South,
                &Action::Hold {
                    player: Player::South,
                },
                CommandTiming {
                    received_at: received,
                    decided_at: received.checked_add(Duration::from_millis(1)).unwrap(),
                },
            )
            .unwrap();
        let transition = authority.take_prepared_transition().unwrap();
        repository.commit_transition(&transition).unwrap();
        let connection = repository.database().connection();
        let persisted: (u64, u64, u64) = connection
            .query_row(
                "SELECT m.revision, COUNT(DISTINCT a.revision_after), COUNT(DISTINCT s.revision)
                 FROM matches m
                 LEFT JOIN actions a ON a.match_id = m.match_id
                 LEFT JOIN match_snapshots s ON s.match_id = m.match_id
                 WHERE m.match_id = ?1 GROUP BY m.match_id",
                [transition.match_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(persisted, (1, 1, 2));
    }

    #[test]
    fn failed_action_insert_rolls_back_its_snapshot_and_match_revision() {
        let started = UNIX_EPOCH.checked_add(Duration::from_secs(35_000)).unwrap();
        let (_catalog, mut authority, scenario_hash) = fixture(None, started);
        let initial = authority.persistence_image(started).unwrap();
        let mut repository = MatchRepository::new(Database::open_in_memory().unwrap());
        repository
            .register_match("CDE345", &scenario_hash, None, &initial)
            .unwrap();
        let received = started.checked_add(Duration::from_secs(1)).unwrap();
        authority
            .execute(
                Uuid::new_v4(),
                Player::South,
                &Action::Hold {
                    player: Player::South,
                },
                CommandTiming {
                    received_at: received,
                    decided_at: received,
                },
            )
            .unwrap();
        let mut invalid = authority.take_prepared_transition().unwrap();
        invalid.decided_unix_millis = invalid.received_unix_millis - 1;
        assert!(repository.commit_transition(&invalid).is_err());
        let connection = repository.database().connection();
        let revision: u64 = connection
            .query_row(
                "SELECT revision FROM matches WHERE match_id = ?1",
                [invalid.match_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let snapshots: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM match_snapshots WHERE match_id = ?1",
                [invalid.match_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, 0);
        assert_eq!(snapshots, 1);
    }

    #[test]
    fn restart_restores_identical_hash_revision_and_effective_clock() {
        let started = UNIX_EPOCH.checked_add(Duration::from_secs(40_000)).unwrap();
        let settings = ClockSettings {
            base_minutes: 5,
            increment_seconds: 0,
        };
        let (catalog, mut authority, scenario_hash) = fixture(Some(settings), started);
        let initial = authority.persistence_image(started).unwrap();
        let mut repository = MatchRepository::new(Database::open_in_memory().unwrap());
        repository
            .register_match("DEF567", &scenario_hash, Some(settings), &initial)
            .unwrap();
        let first_receipt = started.checked_add(Duration::from_secs(10)).unwrap();
        authority
            .execute(
                Uuid::new_v4(),
                Player::South,
                &Action::Hold {
                    player: Player::South,
                },
                CommandTiming {
                    received_at: first_receipt,
                    decided_at: first_receipt,
                },
            )
            .unwrap();
        repository
            .commit_transition(&authority.take_prepared_transition().unwrap())
            .unwrap();
        let report = repository.restore_active(&catalog).unwrap();
        assert!(report.quarantined.is_empty());
        let mut restored = report.matches.into_iter().next().unwrap().authority;
        assert_eq!(restored.snapshot(), authority.snapshot());

        let after_downtime = first_receipt.checked_add(Duration::from_secs(20)).unwrap();
        let action = Action::Hold {
            player: Player::North,
        };
        let idempotency = Uuid::new_v4();
        let timing = CommandTiming {
            received_at: after_downtime,
            decided_at: after_downtime,
        };
        let uninterrupted = authority
            .execute(idempotency, Player::North, &action, timing)
            .unwrap();
        let recovered = restored
            .execute(idempotency, Player::North, &action, timing)
            .unwrap();
        assert_eq!(recovered.revision, uninterrupted.revision);
        assert_eq!(recovered.state_hash, uninterrupted.state_hash);
        assert_eq!(recovered.state.clocks, uninterrupted.state.clocks);
    }

    #[test]
    fn corrupt_match_is_quarantined_without_blocking_healthy_restore() {
        let started = UNIX_EPOCH.checked_add(Duration::from_secs(50_000)).unwrap();
        let (catalog, first, hash) = fixture(None, started);
        let (_, second, _) = fixture(None, started);
        let mut repository = MatchRepository::new(Database::open_in_memory().unwrap());
        repository
            .register_match(
                "GHJ678",
                &hash,
                None,
                &first.persistence_image(started).unwrap(),
            )
            .unwrap();
        repository
            .register_match(
                "KLM789",
                &hash,
                None,
                &second.persistence_image(started).unwrap(),
            )
            .unwrap();
        repository.database().connection().execute(
            "UPDATE match_snapshots SET snapshot_json = CAST('{broken' AS BLOB) WHERE match_id = ?1",
            [first.snapshot().match_id.to_string()],
        ).unwrap();
        let report = repository.restore_active(&catalog).unwrap();
        assert_eq!(report.matches.len(), 1);
        assert_eq!(report.matches[0].match_id, second.snapshot().match_id);
        assert_eq!(report.quarantined.len(), 1);
        assert_eq!(report.quarantined[0].reason_code, "snapshot_json_invalid");
    }

    #[test]
    fn file_restart_reopens_the_same_committed_state() {
        let path =
            std::env::temp_dir().join(format!("crownline-recovery-{}.sqlite3", Uuid::new_v4()));
        let started = UNIX_EPOCH.checked_add(Duration::from_mins(1_000)).unwrap();
        let (catalog, authority, hash) = fixture(None, started);
        let expected = authority.snapshot();
        {
            let database = Database::open(&path, Durability::Full).unwrap();
            let mut repository = MatchRepository::new(database);
            repository
                .register_match(
                    "NPQ234",
                    &hash,
                    None,
                    &authority.persistence_image(started).unwrap(),
                )
                .unwrap();
        }
        {
            let database = Database::open(&path, Durability::Full).unwrap();
            let mut repository = MatchRepository::new(database);
            let report = repository.restore_active(&catalog).unwrap();
            assert_eq!(report.matches.len(), 1);
            assert_eq!(report.matches[0].authority.snapshot(), expected);
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }
}
