//! Versioned wire messages shared by Crownlines clients and servers.

use crownline_core::{Action, MatchState};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Protocol version supported by this build.
pub const PROTOCOL_VERSION: u16 = 1;

/// A client request to apply a canonical action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub protocol_version: u16,
    pub match_id: Uuid,
    pub expected_revision: u64,
    pub idempotency_key: Uuid,
    pub action: Action,
}

/// An authoritative state snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchSnapshot {
    pub protocol_version: u16,
    pub match_id: Uuid,
    pub revision: u64,
    pub state: MatchState,
}
