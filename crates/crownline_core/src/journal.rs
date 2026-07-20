//! Append-only accepted-action journals and deterministic replay.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    rules::{Transition, TransitionEvent, apply_action},
    scenario::{Player, SCENARIO_SCHEMA_VERSION, ScenarioDefinition},
    state::{Action, MatchState, TransitionError},
};

pub const JOURNAL_FORMAT_VERSION: u16 = 1;
pub const MAX_JOURNAL_RECORDS: usize = 100_000;
pub const MAX_JOURNAL_BYTES: usize = 16 * 1024 * 1024;

/// Fixed-size opaque request identity; it cannot contain tokens or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IdempotencyKey(pub [u8; 16]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalRecord {
    pub actor: Player,
    pub revision_before: u64,
    pub revision_after: u64,
    pub idempotency_key: IdempotencyKey,
    pub action: Action,
    pub events: Vec<TransitionEvent>,
    pub state_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionJournal {
    pub format_version: u16,
    pub application_version: String,
    pub scenario_schema_version: u16,
    pub scenario_id: String,
    pub initial_state_hash: String,
    pub records: Vec<JournalRecord>,
}

impl ActionJournal {
    /// Starts a journal for the deterministic initial state of `scenario`.
    ///
    /// # Errors
    ///
    /// Returns scenario/state construction or integrity errors.
    pub fn new(
        application_version: impl Into<String>,
        scenario: &ScenarioDefinition,
    ) -> Result<Self, JournalError> {
        let application_version = application_version.into();
        if application_version.trim().is_empty() {
            return Err(JournalError::MissingApplicationVersion);
        }
        let initial = MatchState::from_scenario(scenario).map_err(JournalError::Transition)?;
        Ok(Self {
            format_version: JOURNAL_FORMAT_VERSION,
            application_version,
            scenario_schema_version: scenario.schema_version,
            scenario_id: scenario.id.clone(),
            initial_state_hash: hash(&initial)?,
            records: Vec::new(),
        })
    }

    /// Applies and records one accepted action, or identifies an earlier request
    /// with the same idempotency key without applying it again.
    ///
    /// # Errors
    ///
    /// Returns compatibility, capacity, state-alignment, legality, or integrity errors.
    pub fn append(
        &mut self,
        scenario: &ScenarioDefinition,
        state: &MatchState,
        idempotency_key: IdempotencyKey,
        action: &Action,
    ) -> Result<AppendOutcome, JournalError> {
        self.validate_header(scenario)?;
        if let Some(record) = self
            .records
            .iter()
            .find(|record| record.idempotency_key == idempotency_key)
        {
            return Ok(AppendOutcome::Duplicate {
                accepted_revision: record.revision_after,
                state_hash: record.state_hash.clone(),
            });
        }
        if self.records.len() >= MAX_JOURNAL_RECORDS {
            return Err(JournalError::TooManyRecords {
                maximum: MAX_JOURNAL_RECORDS,
            });
        }
        self.ensure_tail_matches(state)?;
        let transition = apply_action(scenario, state, action).map_err(JournalError::Transition)?;
        let state_hash = hash(&transition.state)?;
        self.records.push(JournalRecord {
            actor: action_actor(action),
            revision_before: state.revision,
            revision_after: transition.state.revision,
            idempotency_key,
            action: action.clone(),
            events: transition.events.clone(),
            state_hash,
        });
        Ok(AppendOutcome::Accepted(Box::new(transition)))
    }

    /// Replays accepted actions from the scenario's initial state.
    ///
    /// Duplicate keys are verified against the current replay state but never
    /// applied twice.
    ///
    /// # Errors
    ///
    /// Returns compatibility errors or the first divergent recorded revision.
    pub fn replay(&self, scenario: &ScenarioDefinition) -> Result<MatchState, JournalError> {
        self.validate_header(scenario)?;
        let mut state = MatchState::from_scenario(scenario).map_err(JournalError::Transition)?;
        let actual_initial = hash(&state)?;
        if actual_initial != self.initial_state_hash {
            return Err(divergence(
                state.revision,
                "initial state hash",
                self.initial_state_hash.clone(),
                actual_initial,
            ));
        }

        let mut applied = BTreeSet::new();
        for record in &self.records {
            if !applied.insert(record.idempotency_key) {
                let actual = hash(&state)?;
                if record.revision_after != state.revision || record.state_hash != actual {
                    return Err(divergence(
                        record.revision_after,
                        "duplicate idempotency record",
                        record.state_hash.clone(),
                        actual,
                    ));
                }
                continue;
            }
            if record.actor != action_actor(&record.action)
                || record.revision_before != state.revision
            {
                return Err(divergence(
                    record.revision_after,
                    "record actor or starting revision",
                    record.state_hash.clone(),
                    hash(&state)?,
                ));
            }
            let Ok(transition) = apply_action(scenario, &state, &record.action) else {
                return Err(divergence(
                    record.revision_after,
                    "recorded action is no longer legal",
                    record.state_hash.clone(),
                    hash(&state)?,
                ));
            };
            let actual_hash = hash(&transition.state)?;
            if record.revision_after != transition.state.revision
                || record.events != transition.events
                || record.state_hash != actual_hash
            {
                return Err(divergence(
                    record.revision_after,
                    "transition metadata or resulting state",
                    record.state_hash.clone(),
                    actual_hash,
                ));
            }
            state = transition.state;
        }
        Ok(state)
    }

    /// Serializes the bounded journal as JSON.
    ///
    /// # Errors
    ///
    /// Returns a typed size, format, or serialization error.
    pub fn to_json(&self) -> Result<Vec<u8>, JournalError> {
        self.validate_static()?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| JournalError::MalformedJson(error.to_string()))?;
        check_size(&bytes)?;
        Ok(bytes)
    }

    /// Parses a bounded journal. Scenario-specific validation occurs at replay.
    ///
    /// # Errors
    ///
    /// Returns typed corruption, version, metadata, or capacity errors.
    pub fn from_json(bytes: &[u8]) -> Result<Self, JournalError> {
        check_size(bytes)?;
        let journal: Self = serde_json::from_slice(bytes)
            .map_err(|error| JournalError::MalformedJson(error.to_string()))?;
        journal.validate_static()?;
        Ok(journal)
    }

    fn ensure_tail_matches(&self, state: &MatchState) -> Result<(), JournalError> {
        let (expected_revision, expected_hash) = self
            .records
            .last()
            .map_or((0, self.initial_state_hash.as_str()), |record| {
                (record.revision_after, record.state_hash.as_str())
            });
        let actual_hash = hash(state)?;
        if state.revision != expected_revision || actual_hash != expected_hash {
            return Err(JournalError::StateDoesNotMatchTail {
                expected_revision,
                actual_revision: state.revision,
                expected_hash: expected_hash.to_owned(),
                actual_hash,
            });
        }
        Ok(())
    }

    fn validate_header(&self, scenario: &ScenarioDefinition) -> Result<(), JournalError> {
        self.validate_static()?;
        if scenario.schema_version != self.scenario_schema_version {
            return Err(JournalError::ScenarioVersionMismatch {
                expected: self.scenario_schema_version,
                actual: scenario.schema_version,
            });
        }
        if scenario.id != self.scenario_id {
            return Err(JournalError::ScenarioMismatch {
                expected: self.scenario_id.clone(),
                actual: scenario.id.clone(),
            });
        }
        Ok(())
    }

    fn validate_static(&self) -> Result<(), JournalError> {
        if self.format_version != JOURNAL_FORMAT_VERSION {
            return Err(JournalError::UnsupportedVersion {
                found: self.format_version,
                current: JOURNAL_FORMAT_VERSION,
            });
        }
        if self.application_version.trim().is_empty() {
            return Err(JournalError::MissingApplicationVersion);
        }
        if self.scenario_schema_version != SCENARIO_SCHEMA_VERSION {
            return Err(JournalError::UnsupportedScenarioVersion {
                found: self.scenario_schema_version,
                current: SCENARIO_SCHEMA_VERSION,
            });
        }
        if self.records.len() > MAX_JOURNAL_RECORDS {
            return Err(JournalError::TooManyRecords {
                maximum: MAX_JOURNAL_RECORDS,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendOutcome {
    Accepted(Box<Transition>),
    Duplicate {
        accepted_revision: u64,
        state_hash: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayDivergence {
    pub revision: u64,
    pub reason: &'static str,
    pub expected_hash: String,
    pub actual_hash: String,
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("journal JSON is corrupt or truncated: {0}")]
    MalformedJson(String),
    #[error("journal is {size} bytes; maximum is {maximum}")]
    TooLarge { size: usize, maximum: usize },
    #[error("journal format {found} is unsupported; current format is {current}")]
    UnsupportedVersion { found: u16, current: u16 },
    #[error("application_version must not be empty")]
    MissingApplicationVersion,
    #[error("scenario schema {found} is unsupported; current schema is {current}")]
    UnsupportedScenarioVersion { found: u16, current: u16 },
    #[error("journal scenario {expected:?} does not match {actual:?}")]
    ScenarioMismatch { expected: String, actual: String },
    #[error("journal scenario schema {expected} does not match {actual}")]
    ScenarioVersionMismatch { expected: u16, actual: u16 },
    #[error("journal reached its record limit of {maximum}")]
    TooManyRecords { maximum: usize },
    #[error("state does not match journal tail at revision {expected_revision}")]
    StateDoesNotMatchTail {
        expected_revision: u64,
        actual_revision: u64,
        expected_hash: String,
        actual_hash: String,
    },
    #[error("action transition failed: {0}")]
    Transition(TransitionError),
    #[error("replay diverged at revision {}: {}", .0.revision, .0.reason)]
    Divergence(ReplayDivergence),
}

fn action_actor(action: &Action) -> Player {
    match *action {
        Action::Move { player, .. }
        | Action::Hold { player }
        | Action::ChoosePromotion { player, .. }
        | Action::PlacePawn { player, .. }
        | Action::Resign { player }
        | Action::OfferDraw { player }
        | Action::RespondToDraw { player, .. } => player,
    }
}

fn hash(state: &MatchState) -> Result<String, JournalError> {
    state.canonical_hash().map_err(JournalError::Transition)
}

fn divergence(
    revision: u64,
    reason: &'static str,
    expected_hash: String,
    actual_hash: String,
) -> JournalError {
    JournalError::Divergence(ReplayDivergence {
        revision,
        reason,
        expected_hash,
        actual_hash,
    })
}

fn check_size(bytes: &[u8]) -> Result<(), JournalError> {
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(JournalError::TooLarge {
            size: bytes.len(),
            maximum: MAX_JOURNAL_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::scenario::{
        ArmySetup, BoardSize, Coord, Deployment, PieceKind, ScenarioMetadata, ScenarioRules,
    };

    use super::*;

    fn scenario() -> ScenarioDefinition {
        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "journal-test".to_owned(),
            metadata: ScenarioMetadata {
                name: "Journal test".to_owned(),
                description: String::new(),
                expected_minutes: (1, 2),
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
        }
    }

    #[test]
    fn accepted_actions_replay_to_recorded_final_hash() {
        let scenario = scenario();
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let mut journal = ActionJournal::new("0.1.0-test", &scenario).unwrap();

        let first = journal
            .append(
                &scenario,
                &state,
                IdempotencyKey([1; 16]),
                &Action::Hold {
                    player: Player::South,
                },
            )
            .unwrap();
        let AppendOutcome::Accepted(transition) = first else {
            panic!("first action must be accepted");
        };
        state = transition.state;
        let second = journal
            .append(
                &scenario,
                &state,
                IdempotencyKey([2; 16]),
                &Action::Hold {
                    player: Player::North,
                },
            )
            .unwrap();
        let AppendOutcome::Accepted(transition) = second else {
            panic!("second action must be accepted");
        };
        state = transition.state;

        let encoded = journal.to_json().unwrap();
        let decoded = ActionJournal::from_json(&encoded).unwrap();
        let replayed = decoded.replay(&scenario).unwrap();
        assert_eq!(replayed, state);
        assert_eq!(
            decoded.records.last().unwrap().state_hash,
            replayed.canonical_hash().unwrap()
        );
    }

    #[test]
    fn duplicate_idempotency_key_is_not_applied_twice() {
        let scenario = scenario();
        let initial = MatchState::from_scenario(&scenario).unwrap();
        let mut journal = ActionJournal::new("0.1.0-test", &scenario).unwrap();
        let outcome = journal
            .append(
                &scenario,
                &initial,
                IdempotencyKey([7; 16]),
                &Action::Hold {
                    player: Player::South,
                },
            )
            .unwrap();
        let AppendOutcome::Accepted(transition) = outcome else {
            panic!("first action must be accepted");
        };
        let state = transition.state;

        assert!(matches!(
            journal
                .append(
                    &scenario,
                    &state,
                    IdempotencyKey([7; 16]),
                    &Action::Resign {
                        player: Player::North,
                    },
                )
                .unwrap(),
            AppendOutcome::Duplicate {
                accepted_revision: 1,
                ..
            }
        ));
        assert_eq!(journal.records.len(), 1);

        journal.records.push(journal.records[0].clone());
        assert_eq!(journal.replay(&scenario).unwrap(), state);
    }

    #[test]
    fn replay_reports_first_divergent_revision_and_hashes() {
        let scenario = scenario();
        let initial = MatchState::from_scenario(&scenario).unwrap();
        let mut journal = ActionJournal::new("0.1.0-test", &scenario).unwrap();
        journal
            .append(
                &scenario,
                &initial,
                IdempotencyKey([9; 16]),
                &Action::Hold {
                    player: Player::South,
                },
            )
            .unwrap();
        journal.records[0].state_hash = "tampered-expected-hash".to_owned();

        let error = journal.replay(&scenario).unwrap_err();
        let JournalError::Divergence(details) = error else {
            panic!("tampering must produce replay divergence");
        };
        assert_eq!(details.revision, 1);
        assert_eq!(details.expected_hash, "tampered-expected-hash");
        assert_ne!(details.expected_hash, details.actual_hash);
    }
}
