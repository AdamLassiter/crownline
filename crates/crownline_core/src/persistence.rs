//! Versioned, integrity-checked persistence envelopes.
//!
//! Filesystem ownership stays with the host. The atomic-write trait only fixes
//! the required operation order so a host cannot replace a valid save before
//! the temporary payload has been read back and validated.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    rules::{migrate_promotion_eligibility, validate_promotion_eligibility},
    scenario::{SCENARIO_SCHEMA_VERSION, ScenarioDefinition},
    state::{MatchState, TransitionError, validate_exploration},
};

pub const SAVE_FORMAT_VERSION: u16 = 2;
pub const SNAPSHOT_FORMAT_VERSION: u16 = 2;
pub const MAX_PERSISTED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveEnvelope {
    pub format_version: u16,
    pub application_version: String,
    pub scenario_schema_version: u16,
    pub scenario_id: String,
    pub state_hash: String,
    pub state: MatchState,
}

impl SaveEnvelope {
    /// Builds and validates a current local-save envelope.
    ///
    /// # Errors
    ///
    /// Returns an error when state integrity or envelope metadata is invalid.
    pub fn new(
        application_version: impl Into<String>,
        state: MatchState,
    ) -> Result<Self, PersistenceError> {
        let envelope = Self {
            format_version: SAVE_FORMAT_VERSION,
            application_version: application_version.into(),
            scenario_schema_version: SCENARIO_SCHEMA_VERSION,
            scenario_id: state.scenario_id.clone(),
            state_hash: state.canonical_hash().map_err(PersistenceError::State)?,
            state,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    /// Serializes the envelope as bounded JSON.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid metadata, state, or excessive output size.
    pub fn to_json(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        encode_bounded(self)
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        validate_payload(
            self.format_version,
            SAVE_FORMAT_VERSION,
            &self.application_version,
            self.scenario_schema_version,
            &self.scenario_id,
            &self.state_hash,
            &self.state,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEnvelope {
    pub format_version: u16,
    pub application_version: String,
    pub scenario_schema_version: u16,
    pub scenario_id: String,
    pub revision: u64,
    pub state_hash: String,
    pub state: MatchState,
}

impl SnapshotEnvelope {
    /// Builds an authoritative snapshot envelope.
    ///
    /// # Errors
    ///
    /// Returns an error when state integrity or envelope metadata is invalid.
    pub fn new(
        application_version: impl Into<String>,
        state: MatchState,
    ) -> Result<Self, PersistenceError> {
        let envelope = Self {
            format_version: SNAPSHOT_FORMAT_VERSION,
            application_version: application_version.into(),
            scenario_schema_version: SCENARIO_SCHEMA_VERSION,
            scenario_id: state.scenario_id.clone(),
            revision: state.revision,
            state_hash: state.canonical_hash().map_err(PersistenceError::State)?,
            state,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    /// Serializes the snapshot as bounded JSON.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid metadata, state, or excessive output size.
    pub fn to_json(&self) -> Result<Vec<u8>, PersistenceError> {
        self.validate()?;
        encode_bounded(self)
    }

    /// Reads and validates a current authoritative snapshot.
    ///
    /// # Errors
    ///
    /// Returns recoverable format, compatibility, and integrity errors.
    pub fn from_json(bytes: &[u8]) -> Result<Self, PersistenceError> {
        check_size(bytes)?;
        let envelope: Self = serde_json::from_slice(bytes)
            .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
        envelope.validate()?;
        Ok(envelope)
    }

    /// Reads a snapshot and migrates the promotion snapshot added in format 2.
    ///
    /// # Errors
    ///
    /// Returns recoverable format, scenario, migration, and integrity errors.
    pub fn from_json_with_scenario(
        bytes: &[u8],
        scenario: &ScenarioDefinition,
    ) -> Result<Self, PersistenceError> {
        check_size(bytes)?;
        let mut value: Value = serde_json::from_slice(bytes)
            .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
        let source_version = declared_version(&value)?;
        if source_version == SNAPSHOT_FORMAT_VERSION {
            let envelope = Self::from_json(bytes)?;
            validate_envelope_scenario(&envelope.scenario_id, scenario)?;
            validate_promotion_eligibility(scenario, &envelope.state)
                .map_err(PersistenceError::State)?;
            validate_exploration(scenario, &envelope.state).map_err(PersistenceError::State)?;
            return Ok(envelope);
        }
        if source_version != 1 {
            return Err(PersistenceError::UnsupportedSnapshotVersion {
                found: source_version,
                current: SNAPSHOT_FORMAT_VERSION,
            });
        }
        reject_legacy_fog_migration(source_version, scenario)?;
        set_version_fields(&mut value, SNAPSHOT_FORMAT_VERSION)?;
        let mut envelope: Self = serde_json::from_value(value)
            .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
        validate_envelope_scenario(&envelope.scenario_id, scenario)?;
        migrate_promotion_eligibility(scenario, &mut envelope.state)
            .map_err(|error| migration_error(source_version, &error))?;
        validate_exploration(scenario, &envelope.state).map_err(PersistenceError::State)?;
        envelope.state_hash = envelope
            .state
            .canonical_hash()
            .map_err(PersistenceError::State)?;
        envelope.validate()?;
        Ok(envelope)
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        validate_payload(
            self.format_version,
            SNAPSHOT_FORMAT_VERSION,
            &self.application_version,
            self.scenario_schema_version,
            &self.scenario_id,
            &self.state_hash,
            &self.state,
        )?;
        if self.revision != self.state.revision {
            return Err(PersistenceError::RevisionMismatch {
                envelope: self.revision,
                state: self.state.revision,
            });
        }
        Ok(())
    }
}

type Migration = fn(Value) -> Result<Value, String>;

/// A save decoder with an explicit source-version migration registry.
#[derive(Default)]
pub struct SaveReader {
    migrations: BTreeMap<u16, Migration>,
}

impl SaveReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a migration whose input version is exactly `source_version`.
    pub fn register_migration(&mut self, source_version: u16, migration: Migration) {
        self.migrations.insert(source_version, migration);
    }

    /// Decodes a save, applying only a migration registered for its declared
    /// source version.
    ///
    /// # Errors
    ///
    /// Returns recoverable syntax, compatibility, migration, and integrity errors.
    pub fn read(&self, bytes: &[u8]) -> Result<SaveEnvelope, PersistenceError> {
        check_size(bytes)?;
        let mut value: Value = serde_json::from_slice(bytes)
            .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
        let source_version = declared_version(&value)?;
        if source_version != SAVE_FORMAT_VERSION {
            let migration = self.migrations.get(&source_version).ok_or(
                PersistenceError::UnsupportedSaveVersion {
                    found: source_version,
                    current: SAVE_FORMAT_VERSION,
                },
            )?;
            value = migration(value).map_err(|message| PersistenceError::MigrationFailed {
                source_version,
                message,
            })?;
        }
        let envelope: SaveEnvelope = serde_json::from_value(value)
            .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
        envelope.validate()?;
        Ok(envelope)
    }

    /// Reads a save and migrates the promotion snapshot added in format 2.
    ///
    /// # Errors
    ///
    /// Returns recoverable format, scenario, migration, and integrity errors.
    pub fn read_with_scenario(
        &self,
        bytes: &[u8],
        scenario: &ScenarioDefinition,
    ) -> Result<SaveEnvelope, PersistenceError> {
        check_size(bytes)?;
        let mut value: Value = serde_json::from_slice(bytes)
            .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
        let source_version = declared_version(&value)?;
        if source_version == SAVE_FORMAT_VERSION {
            let envelope = self.read(bytes)?;
            validate_envelope_scenario(&envelope.scenario_id, scenario)?;
            validate_promotion_eligibility(scenario, &envelope.state)
                .map_err(PersistenceError::State)?;
            validate_exploration(scenario, &envelope.state).map_err(PersistenceError::State)?;
            return Ok(envelope);
        }
        if source_version != 1 {
            return self.read(bytes);
        }
        reject_legacy_fog_migration(source_version, scenario)?;
        set_version_fields(&mut value, SAVE_FORMAT_VERSION)?;
        let mut envelope: SaveEnvelope = serde_json::from_value(value)
            .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
        validate_envelope_scenario(&envelope.scenario_id, scenario)?;
        migrate_promotion_eligibility(scenario, &mut envelope.state)
            .map_err(|error| migration_error(source_version, &error))?;
        validate_exploration(scenario, &envelope.state).map_err(PersistenceError::State)?;
        envelope.state_hash = envelope
            .state
            .canonical_hash()
            .map_err(PersistenceError::State)?;
        envelope.validate()?;
        Ok(envelope)
    }
}

/// Host-provided operations for a recoverable atomic save replacement.
pub trait AtomicSaveStorage {
    /// Writes a new payload without altering the current save.
    ///
    /// # Errors
    ///
    /// Returns a host-specific message when temporary storage cannot be written.
    fn write_temporary(&mut self, bytes: &[u8]) -> Result<(), String>;

    /// Reads back the complete temporary payload for validation.
    ///
    /// # Errors
    ///
    /// Returns a host-specific message when the temporary payload cannot be read.
    fn read_temporary(&mut self) -> Result<Vec<u8>, String>;

    /// Atomically replaces the current save with the validated temporary one.
    ///
    /// # Errors
    ///
    /// Returns a host-specific message when replacement cannot be completed.
    fn replace_with_temporary(&mut self) -> Result<(), String>;

    /// Best-effort cleanup after a read or validation failure.
    fn discard_temporary(&mut self);
}

/// Writes, reads back, validates, and only then replaces the previous save.
///
/// # Errors
///
/// Returns the failed operation stage. Validation failure always discards the
/// temporary payload without requesting replacement.
pub fn write_save_atomically<S: AtomicSaveStorage>(
    storage: &mut S,
    envelope: &SaveEnvelope,
) -> Result<(), PersistenceError> {
    let bytes = envelope.to_json()?;
    write_bytes_atomically(storage, &bytes, |temporary| {
        SaveReader::new().read(temporary).map(|_| ())
    })
}

/// Writes arbitrary bounded host payload bytes and validates the read-back
/// value before replacing an existing file.
///
/// # Errors
///
/// Returns the failed atomic stage or the validator's recoverable error.
pub fn write_bytes_atomically<S, V>(
    storage: &mut S,
    bytes: &[u8],
    validate: V,
) -> Result<(), PersistenceError>
where
    S: AtomicSaveStorage,
    V: FnOnce(&[u8]) -> Result<(), PersistenceError>,
{
    check_size(bytes)?;
    storage
        .write_temporary(bytes)
        .map_err(|message| atomic_error(AtomicWriteStage::WriteTemporary, message))?;
    let temporary = match storage.read_temporary() {
        Ok(bytes) => bytes,
        Err(message) => {
            storage.discard_temporary();
            return Err(atomic_error(AtomicWriteStage::ReadTemporary, message));
        }
    };
    if let Err(error) = validate(&temporary) {
        storage.discard_temporary();
        return Err(PersistenceError::TemporaryValidation(Box::new(error)));
    }
    storage
        .replace_with_temporary()
        .map_err(|message| atomic_error(AtomicWriteStage::Replace, message))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicWriteStage {
    WriteTemporary,
    ReadTemporary,
    Replace,
}

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("persisted JSON is corrupt or truncated: {0}")]
    MalformedJson(String),
    #[error("persisted payload is {size} bytes; maximum is {maximum}")]
    TooLarge { size: usize, maximum: usize },
    #[error("persisted payload does not declare a numeric format_version")]
    MissingFormatVersion,
    #[error("save format {found} is unsupported; current format is {current}")]
    UnsupportedSaveVersion { found: u16, current: u16 },
    #[error("snapshot format {found} is unsupported; current format is {current}")]
    UnsupportedSnapshotVersion { found: u16, current: u16 },
    #[error("persistence format {found} does not match expected format {expected}")]
    WrongFormatVersion { found: u16, expected: u16 },
    #[error("migration from save format {source_version} failed: {message}")]
    MigrationFailed {
        source_version: u16,
        message: String,
    },
    #[error("application_version must not be empty")]
    MissingApplicationVersion,
    #[error("scenario schema {found} is unsupported; current schema is {current}")]
    UnsupportedScenarioVersion { found: u16, current: u16 },
    #[error("envelope scenario {envelope:?} does not match state scenario {state:?}")]
    ScenarioMismatch { envelope: String, state: String },
    #[error("persisted state failed validation: {0}")]
    State(TransitionError),
    #[error("persisted state integrity check failed")]
    IntegrityMismatch,
    #[error("snapshot revision {envelope} does not match state revision {state}")]
    RevisionMismatch { envelope: u64, state: u64 },
    #[error("atomic save failed during {stage:?}: {message}")]
    AtomicWrite {
        stage: AtomicWriteStage,
        message: String,
    },
    #[error("temporary save failed validation: {0}")]
    TemporaryValidation(Box<PersistenceError>),
}

fn validate_payload(
    format_version: u16,
    expected_format: u16,
    application_version: &str,
    scenario_schema_version: u16,
    scenario_id: &str,
    state_hash: &str,
    state: &MatchState,
) -> Result<(), PersistenceError> {
    if format_version != expected_format {
        return Err(PersistenceError::WrongFormatVersion {
            found: format_version,
            expected: expected_format,
        });
    }
    if application_version.trim().is_empty() {
        return Err(PersistenceError::MissingApplicationVersion);
    }
    if scenario_schema_version != SCENARIO_SCHEMA_VERSION {
        return Err(PersistenceError::UnsupportedScenarioVersion {
            found: scenario_schema_version,
            current: SCENARIO_SCHEMA_VERSION,
        });
    }
    if scenario_id != state.scenario_id {
        return Err(PersistenceError::ScenarioMismatch {
            envelope: scenario_id.to_owned(),
            state: state.scenario_id.clone(),
        });
    }
    state
        .validate_invariants()
        .map_err(PersistenceError::State)?;
    let actual_hash = state.canonical_hash().map_err(PersistenceError::State)?;
    if state_hash != actual_hash {
        return Err(PersistenceError::IntegrityMismatch);
    }
    Ok(())
}

fn encode_bounded<T: Serialize>(value: &T) -> Result<Vec<u8>, PersistenceError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| PersistenceError::MalformedJson(error.to_string()))?;
    check_size(&bytes)?;
    Ok(bytes)
}

fn check_size(bytes: &[u8]) -> Result<(), PersistenceError> {
    if bytes.len() > MAX_PERSISTED_BYTES {
        return Err(PersistenceError::TooLarge {
            size: bytes.len(),
            maximum: MAX_PERSISTED_BYTES,
        });
    }
    Ok(())
}

fn declared_version(value: &Value) -> Result<u16, PersistenceError> {
    value
        .get("format_version")
        .and_then(Value::as_u64)
        .and_then(|version| u16::try_from(version).ok())
        .ok_or(PersistenceError::MissingFormatVersion)
}

fn set_version_fields(value: &mut Value, format_version: u16) -> Result<(), PersistenceError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| PersistenceError::MalformedJson("envelope must be an object".to_owned()))?;
    object.insert("format_version".to_owned(), Value::from(format_version));
    object.insert(
        "scenario_schema_version".to_owned(),
        Value::from(SCENARIO_SCHEMA_VERSION),
    );
    Ok(())
}

fn validate_envelope_scenario(
    envelope_scenario: &str,
    scenario: &ScenarioDefinition,
) -> Result<(), PersistenceError> {
    scenario
        .validate()
        .map_err(|errors| PersistenceError::State(TransitionError::InvalidScenario(errors)))?;
    if scenario.schema_version != SCENARIO_SCHEMA_VERSION || envelope_scenario != scenario.id {
        return Err(PersistenceError::ScenarioMismatch {
            envelope: envelope_scenario.to_owned(),
            state: scenario.id.clone(),
        });
    }
    Ok(())
}

fn migration_error(source_version: u16, error: &TransitionError) -> PersistenceError {
    PersistenceError::MigrationFailed {
        source_version,
        message: error.to_string(),
    }
}

fn reject_legacy_fog_migration(
    source_version: u16,
    scenario: &ScenarioDefinition,
) -> Result<(), PersistenceError> {
    if scenario.rules.fog.is_some() {
        return Err(PersistenceError::MigrationFailed {
            source_version,
            message: "legacy state has no exploration history for a fog-enabled scenario"
                .to_owned(),
        });
    }
    Ok(())
}

fn atomic_error(stage: AtomicWriteStage, message: String) -> PersistenceError {
    PersistenceError::AtomicWrite { stage, message }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::{
        rules::apply_action,
        scenario::{
            ArmySetup, BoardSize, Coord, Deployment, FOG_RULES_SCHEMA_VERSION, FogRules, PieceKind,
            Player, ScenarioDefinition, ScenarioMetadata, ScenarioRules,
        },
        state::{
            Action, ClockState, MandatoryChoice, MatchOutcome, OutcomeReason, PieceId, PieceOrigin,
            PromotionEligibility, TurnPhase,
        },
    };

    use super::*;

    fn persistence_scenario() -> ScenarioDefinition {
        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "persistence-test".to_owned(),
            metadata: ScenarioMetadata {
                name: "Persistence test".to_owned(),
                description: String::new(),
                expected_minutes: (1, 2),
                is_default: false,
            },
            board: BoardSize {
                width: 8,
                height: 8,
            },
            terrain: BTreeMap::new(),
            edges: BTreeMap::new(),
            deployments: vec![
                Deployment {
                    player: Player::North,
                    kind: PieceKind::King,
                    at: Coord::new(4, 0),
                },
                Deployment {
                    player: Player::South,
                    kind: PieceKind::King,
                    at: Coord::new(4, 7),
                },
                Deployment {
                    player: Player::South,
                    kind: PieceKind::Pawn,
                    at: Coord::new(1, 1),
                },
                Deployment {
                    player: Player::North,
                    kind: PieceKind::Rook,
                    at: Coord::new(7, 0),
                },
            ],
            settlements: Vec::new(),
            promotion_sites: Vec::new(),
            keeps: Vec::new(),
            fortifications: Vec::new(),
            castling_routes: Vec::new(),
            rules: ScenarioRules {
                army_setup: ArmySetup::Custom,
                ..ScenarioRules::default()
            },
            guided: None,
        }
    }

    fn state_with_variants() -> MatchState {
        let scenario = persistence_scenario();
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let pawn = state
            .pieces
            .values()
            .find(|piece| piece.kind == PieceKind::Pawn)
            .map(|piece| piece.id)
            .unwrap();
        state.pieces.get_mut(&pawn).unwrap().origin = PieceOrigin::Promoted { from: PieceId(42) };
        state.promotion_candidates.insert(pawn, 3);
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![
                MandatoryChoice::Promote {
                    pawn,
                    site_index: 0,
                    eligibility: PromotionEligibility::default(),
                },
                MandatoryChoice::PlacePawn {
                    settlement_index: 0,
                    legal_squares: BTreeSet::from([Coord::new(2, 2), Coord::new(3, 2)]),
                },
            ],
        };
        state.clocks = Some(ClockState {
            north_millis: 10_000,
            south_millis: 9_000,
            increment_millis: 100,
        });
        state.outstanding_draw_offer = Some(Player::North);
        state
    }

    #[test]
    fn save_and_snapshot_round_trip_pending_state_variants() {
        let state = state_with_variants();
        let save = SaveEnvelope::new("0.1.0-test", state.clone()).unwrap();
        let decoded = SaveReader::new().read(&save.to_json().unwrap()).unwrap();
        assert_eq!(decoded, save);

        let snapshot = SnapshotEnvelope::new("0.1.0-test", state).unwrap();
        let decoded = SnapshotEnvelope::from_json(&snapshot.to_json().unwrap()).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn fog_save_and_snapshot_preserve_exploration_and_reject_missing_history() {
        let mut scenario = persistence_scenario();
        scenario.rules.fog = Some(FogRules {
            schema_version: FOG_RULES_SCHEMA_VERSION,
            vision_radius: 1,
        });
        let state = MatchState::from_scenario(&scenario).unwrap();
        let king = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        let state = apply_action(
            &scenario,
            &state,
            &Action::Move {
                player: Player::South,
                piece: king,
                to: Coord::new(4, 6),
            },
        )
        .unwrap()
        .state;

        let save = SaveEnvelope::new("0.1.0-test", state.clone()).unwrap();
        let decoded = SaveReader::new()
            .read_with_scenario(&save.to_json().unwrap(), &scenario)
            .unwrap();
        assert_eq!(decoded.state.exploration, state.exploration);

        let snapshot = SnapshotEnvelope::new("0.1.0-test", state.clone()).unwrap();
        let decoded =
            SnapshotEnvelope::from_json_with_scenario(&snapshot.to_json().unwrap(), &scenario)
                .unwrap();
        assert_eq!(decoded.state.exploration, state.exploration);

        let mut missing = serde_json::to_value(save).unwrap();
        missing["state"]
            .as_object_mut()
            .unwrap()
            .remove("exploration");
        let missing_state: MatchState = serde_json::from_value(missing["state"].clone()).unwrap();
        missing["state_hash"] = Value::from(missing_state.canonical_hash().unwrap());
        assert!(matches!(
            SaveReader::new()
                .read_with_scenario(&serde_json::to_vec(&missing).unwrap(), &scenario,),
            Err(PersistenceError::State(
                TransitionError::ExplorationModeMismatch
            ))
        ));

        missing["format_version"] = Value::from(1);
        missing["scenario_schema_version"] = Value::from(1);
        assert!(matches!(
            SaveReader::new()
                .read_with_scenario(&serde_json::to_vec(&missing).unwrap(), &scenario,),
            Err(PersistenceError::MigrationFailed {
                source_version: 1,
                ..
            })
        ));
    }

    #[test]
    fn format_one_save_and_snapshot_migrate_one_shared_promotion_batch() {
        let scenario = persistence_scenario();
        let state = state_with_variants();

        let legacy_value = |mut value: Value| {
            value["format_version"] = Value::from(1);
            value["scenario_schema_version"] = Value::from(1);
            value["state_hash"] = Value::from("legacy-state-hash");
            let queue = value
                .pointer_mut("/state/phase/resolving_choices/queue")
                .and_then(Value::as_array_mut)
                .expect("fixture has a choice queue");
            let second_promotion = queue[0].clone();
            queue.push(second_promotion);
            for choice in queue {
                if let Some(promote) = choice.get_mut("promote").and_then(Value::as_object_mut) {
                    promote.remove("eligibility");
                }
            }
            value
        };

        let save = SaveEnvelope::new("0.1.0-test", state.clone()).unwrap();
        let legacy_save = legacy_value(serde_json::to_value(save).unwrap());
        let migrated = SaveReader::new()
            .read_with_scenario(&serde_json::to_vec(&legacy_save).unwrap(), &scenario)
            .unwrap();
        assert_eq!(migrated.format_version, SAVE_FORMAT_VERSION);
        assert_eq!(migrated.scenario_schema_version, SCENARIO_SCHEMA_VERSION);
        let TurnPhase::ResolvingChoices { queue } = &migrated.state.phase else {
            panic!("migration preserves choices");
        };
        let snapshots = queue
            .iter()
            .filter_map(|choice| match choice {
                MandatoryChoice::Promote { eligibility, .. } => Some(eligibility),
                MandatoryChoice::PlacePawn { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0], snapshots[1]);
        assert_eq!(snapshots[0], &PromotionEligibility::default());
        assert_eq!(
            migrated.state_hash,
            migrated.state.canonical_hash().unwrap()
        );

        let snapshot = SnapshotEnvelope::new("0.1.0-test", state).unwrap();
        let legacy_snapshot = legacy_value(serde_json::to_value(snapshot).unwrap());
        let migrated = SnapshotEnvelope::from_json_with_scenario(
            &serde_json::to_vec(&legacy_snapshot).unwrap(),
            &scenario,
        )
        .unwrap();
        assert_eq!(migrated.format_version, SNAPSHOT_FORMAT_VERSION);
        assert_eq!(
            migrated.state_hash,
            migrated.state.canonical_hash().unwrap()
        );
    }

    #[test]
    fn terminal_state_round_trips_with_integrity_metadata() {
        let mut state = state_with_variants();
        state.phase = TurnPhase::Command;
        state.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        let envelope = SaveEnvelope::new("0.1.0-test", state).unwrap();
        let decoded = SaveReader::new()
            .read(&envelope.to_json().unwrap())
            .unwrap();
        assert_eq!(decoded.state_hash, decoded.state.canonical_hash().unwrap());
        assert_eq!(decoded.state.outcome, envelope.state.outcome);
    }

    #[test]
    fn corrupt_truncated_and_unsupported_saves_are_recoverable_errors() {
        let reader = SaveReader::new();
        assert!(matches!(
            reader.read(br#"{"format_version":1,"state":"#),
            Err(PersistenceError::MalformedJson(_))
        ));
        assert!(matches!(
            reader.read(br#"{"format_version":"one"}"#),
            Err(PersistenceError::MissingFormatVersion)
        ));
        assert!(matches!(
            reader.read(br#"{"format_version":99}"#),
            Err(PersistenceError::UnsupportedSaveVersion {
                found: 99,
                current: SAVE_FORMAT_VERSION
            })
        ));
    }

    #[test]
    fn migrations_are_selected_by_declared_source_version() {
        fn migrate_zero(mut value: Value) -> Result<Value, String> {
            if !value.is_object() {
                return Err("legacy envelope must be an object".to_owned());
            }
            value["format_version"] = Value::from(SAVE_FORMAT_VERSION);
            Ok(value)
        }

        let envelope = SaveEnvelope::new("0.1.0-test", state_with_variants()).unwrap();
        let mut old: Value = serde_json::from_slice(&envelope.to_json().unwrap()).unwrap();
        old["format_version"] = Value::from(0);
        let bytes = serde_json::to_vec(&old).unwrap();
        assert!(matches!(
            SaveReader::new().read(&bytes),
            Err(PersistenceError::UnsupportedSaveVersion { found: 0, .. })
        ));

        let mut reader = SaveReader::new();
        reader.register_migration(0, migrate_zero);
        assert_eq!(reader.read(&bytes).unwrap(), envelope);
    }

    #[derive(Default)]
    struct FakeStorage {
        current: Vec<u8>,
        temporary: Vec<u8>,
        corrupt_temporary: bool,
        replaced: bool,
        discarded: bool,
    }

    impl AtomicSaveStorage for FakeStorage {
        fn write_temporary(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.temporary = if self.corrupt_temporary {
                b"truncated".to_vec()
            } else {
                bytes.to_vec()
            };
            Ok(())
        }

        fn read_temporary(&mut self) -> Result<Vec<u8>, String> {
            Ok(self.temporary.clone())
        }

        fn replace_with_temporary(&mut self) -> Result<(), String> {
            self.current.clone_from(&self.temporary);
            self.replaced = true;
            Ok(())
        }

        fn discard_temporary(&mut self) {
            self.temporary.clear();
            self.discarded = true;
        }
    }

    #[test]
    fn atomic_write_validates_temporary_before_replacement() {
        let envelope = SaveEnvelope::new("0.1.0-test", state_with_variants()).unwrap();
        let previous = b"previous-valid-save".to_vec();
        let mut failed = FakeStorage {
            current: previous.clone(),
            corrupt_temporary: true,
            ..FakeStorage::default()
        };
        assert!(matches!(
            write_save_atomically(&mut failed, &envelope),
            Err(PersistenceError::TemporaryValidation(_))
        ));
        assert_eq!(failed.current, previous);
        assert!(!failed.replaced);
        assert!(failed.discarded);

        let mut successful = FakeStorage::default();
        write_save_atomically(&mut successful, &envelope).unwrap();
        assert!(successful.replaced);
        assert_eq!(
            SaveReader::new().read(&successful.current).unwrap(),
            envelope
        );
    }
}
