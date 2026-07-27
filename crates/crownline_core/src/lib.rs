//! Deterministic Crownlines rules, scenario definitions, and canonical match state.
//!
//! This crate deliberately has no dependency on Bevy, networking, filesystems, or
//! wall-clock time. Hosts supply scenarios, actions, and elapsed time explicitly.

pub mod clock;
pub mod guided;
pub mod journal;
pub mod persistence;
pub mod projection;
pub mod rules;
pub mod scenario;
pub mod state;

pub use clock::{
    ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES, advance_clock,
    apply_timed_action, start_clocks,
};
pub use guided::{
    GUIDED_SCHEMA_VERSION, GuidedAiConfig, GuidedAiMode, GuidedCompletion, GuidedContent,
    GuidedEventPredicate, GuidedKind, GuidedPredicate, GuidedPredicateContext, GuidedReplyNode,
    GuidedStage, GuidedStart, ObjectiveResult,
};
pub use journal::{
    ActionJournal, AppendOutcome, IdempotencyKey, JournalError, JournalRecord, ReplayDivergence,
};
pub use persistence::{
    AtomicSaveStorage, AtomicWriteStage, MAX_PERSISTED_BYTES, PersistenceError,
    SAVE_FORMAT_VERSION, SNAPSHOT_FORMAT_VERSION, SaveEnvelope, SaveReader, SnapshotEnvelope,
    write_bytes_atomically, write_save_atomically,
};
pub use projection::{
    GovernanceState, HoverView, KnownEdge, KnownSquare, PLAYER_VIEW_SCHEMA_VERSION, PlayerEvent,
    PlayerIntentError, PlayerView, SettlementDynamicView, SettlementView, StaticOwnedSiteView,
    StaticSiteView, ViewMandatoryChoice, ViewPiece, ViewTurnPhase, apply_player_intent,
    project_events, project_player_view,
};
pub use rules::{
    AttackLine, BlockedGovernanceLine, GovernanceBlocker, GovernanceReport, LegalMove,
    MoveInspection, MoveKind, MoveUnavailability, Transition, TransitionEvent, apply_action,
    attack_lines_on, governance_report, inspect_move, is_in_check, legal_mandatory_choice_actions,
    legal_moves, migrate_promotion_eligibility, pawn_placement_squares, realm_control_score,
    validate_promotion_eligibility,
};
pub use scenario::{
    FOG_RULES_SCHEMA_VERSION, FogRules, FogScenarioVariant, PromotionUnlockRules,
    SCENARIO_VARIANT_SCHEMA_VERSION, ScenarioDefinition, ScenarioError, ScenarioHashError,
};
pub use state::{
    Action, ExplorationState, MatchState, PromotionEligibility, RealmControlScore, TransitionError,
    VisibilityCache, VisibilityState, update_exploration, validate_exploration, visibility_at,
    visible_coordinates,
};
