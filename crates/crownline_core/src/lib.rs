//! Deterministic Crownlines rules, scenario definitions, and canonical match state.
//!
//! This crate deliberately has no dependency on Bevy, networking, filesystems, or
//! wall-clock time. Hosts supply scenarios, actions, and elapsed time explicitly.

pub mod journal;
pub mod persistence;
pub mod rules;
pub mod scenario;
pub mod state;

pub use journal::{
    ActionJournal, AppendOutcome, IdempotencyKey, JournalError, JournalRecord, ReplayDivergence,
};
pub use persistence::{
    AtomicSaveStorage, AtomicWriteStage, PersistenceError, SaveEnvelope, SaveReader,
    SnapshotEnvelope, write_save_atomically,
};
pub use rules::{
    LegalMove, MoveKind, Transition, TransitionEvent, apply_action, is_in_check, legal_moves,
};
pub use scenario::{ScenarioDefinition, ScenarioError};
pub use state::{Action, MatchState, TransitionError};
