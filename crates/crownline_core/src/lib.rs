//! Deterministic Crownlines rules, scenario definitions, and canonical match state.
//!
//! This crate deliberately has no dependency on Bevy, networking, filesystems, or
//! wall-clock time. Hosts supply scenarios, actions, and elapsed time explicitly.

pub mod clock;
pub mod journal;
pub mod persistence;
pub mod rules;
pub mod scenario;
pub mod state;

pub use clock::{
    ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES, advance_clock,
    apply_timed_action, start_clocks,
};
pub use journal::{
    ActionJournal, AppendOutcome, IdempotencyKey, JournalError, JournalRecord, ReplayDivergence,
};
pub use persistence::{
    AtomicSaveStorage, AtomicWriteStage, MAX_PERSISTED_BYTES, PersistenceError,
    SAVE_FORMAT_VERSION, SNAPSHOT_FORMAT_VERSION, SaveEnvelope, SaveReader, SnapshotEnvelope,
    write_bytes_atomically, write_save_atomically,
};
pub use rules::{
    AttackLine, BlockedGovernanceLine, GovernanceBlocker, GovernanceReport, LegalMove,
    MoveInspection, MoveKind, MoveUnavailability, Transition, TransitionEvent, apply_action,
    attack_lines_on, governance_report, inspect_move, is_in_check, legal_mandatory_choice_actions,
    legal_moves, migrate_promotion_eligibility, pawn_placement_squares, realm_control_score,
    validate_promotion_eligibility,
};
pub use scenario::{PromotionUnlockRules, ScenarioDefinition, ScenarioError, ScenarioHashError};
pub use state::{Action, MatchState, PromotionEligibility, RealmControlScore, TransitionError};
