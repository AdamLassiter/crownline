//! Bounded, versioned wire messages shared by Crownlines clients and servers.

use std::collections::{BTreeMap, VecDeque};

use crownline_core::{
    Action, ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES, MatchState,
    scenario::Player,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use uuid::Uuid;

/// Protocol version supported by this build.
pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_HTTP_REQUEST_BYTES: usize = 8 * 1024;
pub const MAX_CLIENT_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_PLAYER_NAME_CHARS: usize = 24;
pub const ROOM_CODE_CHARS: usize = 6;
pub const MAX_SCENARIO_ID_CHARS: usize = 64;
pub const MAX_RECONNECT_TOKEN_CHARS: usize = 256;
pub const MAX_CACHED_MUTATIONS_PER_MATCH: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthRequest {
    pub protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub protocol_version: u16,
    pub status: ServiceStatus,
    pub liveness: ServiceStatus,
    pub database: ServiceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Ok,
    Degraded,
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRoomRequest {
    pub protocol_version: u16,
    pub player_name: String,
    pub scenario_id: String,
    pub clock: Option<ClockSettings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRoomResponse {
    pub protocol_version: u16,
    pub match_id: Uuid,
    pub room_code: String,
    pub seat: Player,
    pub reconnect_token: ReconnectToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRoomRequest {
    pub protocol_version: u16,
    pub player_name: String,
    pub room_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRoomResponse {
    pub protocol_version: u16,
    pub match_id: Uuid,
    pub seat: Player,
    pub reconnect_token: ReconnectToken,
}

/// A seat secret that serializes for issuance but always redacts debug output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReconnectToken(String);

impl ReconnectToken {
    pub fn issued(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ReconnectToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReconnectToken([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Authenticate {
        protocol_version: u16,
        room_code: String,
        reconnect_token: ReconnectToken,
    },
    Ready {
        protocol_version: u16,
        context: MutationContext,
    },
    Action {
        protocol_version: u16,
        request: ActionRequest,
    },
    Draw {
        protocol_version: u16,
        context: MutationContext,
        command: DrawCommand,
    },
    Rematch {
        protocol_version: u16,
        context: MutationContext,
        command: RematchCommand,
    },
    Leave {
        protocol_version: u16,
        context: MutationContext,
    },
}

impl ClientMessage {
    pub const fn protocol_version(&self) -> u16 {
        match self {
            Self::Authenticate {
                protocol_version, ..
            }
            | Self::Ready {
                protocol_version, ..
            }
            | Self::Action {
                protocol_version, ..
            }
            | Self::Draw {
                protocol_version, ..
            }
            | Self::Rematch {
                protocol_version, ..
            }
            | Self::Leave {
                protocol_version, ..
            } => *protocol_version,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawCommand {
    Offer,
    Accept,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RematchCommand {
    Request,
    Accept,
    Decline,
}

/// A client request to apply one canonical gameplay action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub context: MutationContext,
    pub action: Action,
}

/// Revision and retry identity required for every state-changing request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationContext {
    pub match_id: Uuid,
    pub expected_revision: u64,
    pub idempotency_key: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Snapshot {
        protocol_version: u16,
        snapshot: Box<MatchSnapshot>,
    },
    Acknowledgement {
        protocol_version: u16,
        result: Box<MutationResult>,
    },
    Error {
        protocol_version: u16,
        code: ErrorCode,
        message: String,
        retryable: bool,
        snapshot: Option<Box<MatchSnapshot>>,
    },
    ConnectionState {
        protocol_version: u16,
        match_id: Uuid,
        state: ConnectionState,
    },
    RematchState {
        protocol_version: u16,
        match_id: Uuid,
        state: RematchState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchSnapshot {
    pub match_id: Uuid,
    pub revision: u64,
    pub scenario_id: String,
    pub scenario_hash: String,
    pub state_hash: String,
    pub state: MatchState,
    pub room_state: ConnectionState,
    pub rematch_state: Option<RematchState>,
}

/// The exact cacheable result returned for an accepted mutation and its retries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationResult {
    pub match_id: Uuid,
    pub idempotency_key: Uuid,
    pub snapshot: MatchSnapshot,
}

/// Outcome of checking revision/idempotency before invoking a mutation closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationResolution<E> {
    Accepted {
        result: MutationResult,
        duplicate: bool,
    },
    Stale(MatchSnapshot),
    Rejected(E),
}

/// Bounded per-match cache that makes retries return the original accepted result.
#[derive(Debug, Default)]
pub struct IdempotencyCache {
    entries: BTreeMap<(Uuid, Uuid), MutationResult>,
    insertion_order: VecDeque<(Uuid, Uuid)>,
}

impl IdempotencyCache {
    /// Returns a prior result without invoking `apply`, rejects a stale revision
    /// with the current snapshot, or applies and records a new mutation once.
    pub fn resolve<E, F>(
        &mut self,
        context: MutationContext,
        current: &MatchSnapshot,
        apply: F,
    ) -> MutationResolution<E>
    where
        F: FnOnce() -> Result<MatchSnapshot, E>,
    {
        let key = (context.match_id, context.idempotency_key);
        if let Some(result) = self.entries.get(&key) {
            return MutationResolution::Accepted {
                result: result.clone(),
                duplicate: true,
            };
        }
        if context.match_id != current.match_id || context.expected_revision != current.revision {
            return MutationResolution::Stale(current.clone());
        }
        let snapshot = match apply() {
            Ok(snapshot) => snapshot,
            Err(error) => return MutationResolution::Rejected(error),
        };
        let result = MutationResult {
            match_id: context.match_id,
            idempotency_key: context.idempotency_key,
            snapshot,
        };
        self.entries.insert(key, result.clone());
        self.insertion_order.push_back(key);
        if self.entries.len() > MAX_CACHED_MUTATIONS_PER_MATCH
            && let Some(oldest) = self.insertion_order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        MutationResolution::Accepted {
            result,
            duplicate: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    WaitingForOpponent,
    WaitingForReady,
    Connected,
    OpponentDisconnected,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RematchState {
    Requested,
    Accepted,
    Declined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    IncompatibleProtocol,
    InvalidRequest,
    InvalidRoomCode,
    RoomNotFound,
    RoomFull,
    Unauthorized,
    StaleRevision,
    DuplicateRequest,
    InvalidAction,
    RateLimited,
    Internal,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("message is {actual} bytes; limit is {maximum}")]
    MessageTooLarge { actual: usize, maximum: usize },
    #[error("malformed message: {0}")]
    Malformed(String),
    #[error("protocol version {found} is unsupported; expected {expected}")]
    IncompatibleVersion { found: u16, expected: u16 },
    #[error("player name must contain 1-{MAX_PLAYER_NAME_CHARS} non-control characters")]
    InvalidPlayerName,
    #[error(
        "room code must contain exactly {ROOM_CODE_CHARS} uppercase ASCII characters or digits"
    )]
    InvalidRoomCode,
    #[error("scenario ID must contain 1-{MAX_SCENARIO_ID_CHARS} safe characters")]
    InvalidScenarioId,
    #[error("clock configuration is outside supported bounds")]
    InvalidClock,
    #[error("reconnect token is malformed")]
    InvalidReconnectToken,
    #[error("snapshot metadata does not match canonical state: {0}")]
    InvalidSnapshot(String),
}

/// Decodes a JSON HTTP body after enforcing the request-size boundary.
///
/// # Errors
///
/// Returns a bounded malformed-message error when the body is too large or invalid JSON.
pub fn decode_http_request<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolError> {
    decode_bounded(bytes, MAX_HTTP_REQUEST_BYTES)
}

/// Decodes and validates the common envelope of an inbound WebSocket message.
///
/// # Errors
///
/// Returns an error for excessive size, malformed JSON, incompatible versions,
/// or draw commands disguised as ordinary gameplay actions.
pub fn decode_client_message(bytes: &[u8]) -> Result<ClientMessage, ProtocolError> {
    let message: ClientMessage = decode_bounded(bytes, MAX_CLIENT_MESSAGE_BYTES)?;
    validate_version(message.protocol_version())?;
    if let ClientMessage::Authenticate {
        room_code,
        reconnect_token,
        ..
    } = &message
    {
        validate_room_code(room_code)?;
        let token = reconnect_token.expose();
        if token.is_empty()
            || token.len() > MAX_RECONNECT_TOKEN_CHARS
            || token.chars().any(char::is_control)
        {
            return Err(ProtocolError::InvalidReconnectToken);
        }
    }
    if let ClientMessage::Action { request, .. } = &message
        && matches!(
            request.action,
            Action::OfferDraw { .. } | Action::RespondToDraw { .. }
        )
    {
        return Err(ProtocolError::Malformed(
            "draw commands must use the draw message".to_owned(),
        ));
    }
    Ok(message)
}

/// Validates the version carried by a health request.
///
/// # Errors
///
/// Returns [`ProtocolError::IncompatibleVersion`] for a different version.
pub fn validate_health_request(request: &HealthRequest) -> Result<(), ProtocolError> {
    validate_version(request.protocol_version)
}

/// Validates all bounded create-room fields.
///
/// # Errors
///
/// Returns the first incompatible or invalid field classification.
pub fn validate_create_room(request: &CreateRoomRequest) -> Result<(), ProtocolError> {
    validate_version(request.protocol_version)?;
    validate_player_name(&request.player_name)?;
    validate_scenario_id(&request.scenario_id)?;
    if request.clock.is_some_and(|clock| {
        !(MIN_BASE_MINUTES..=MAX_BASE_MINUTES).contains(&clock.base_minutes)
            || clock.increment_seconds > MAX_INCREMENT_SECONDS
    }) {
        return Err(ProtocolError::InvalidClock);
    }
    Ok(())
}

/// Validates all bounded join-room fields.
///
/// # Errors
///
/// Returns the first incompatible or invalid field classification.
pub fn validate_join_room(request: &JoinRoomRequest) -> Result<(), ProtocolError> {
    validate_version(request.protocol_version)?;
    validate_player_name(&request.player_name)?;
    validate_room_code(&request.room_code)
}

/// Validates a player-facing name without normalizing it.
///
/// # Errors
///
/// Returns [`ProtocolError::InvalidPlayerName`] for empty, overlong, or control-character input.
pub fn validate_player_name(name: &str) -> Result<(), ProtocolError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_PLAYER_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ProtocolError::InvalidPlayerName);
    }
    Ok(())
}

/// Validates the public, fixed-width room-code representation.
///
/// # Errors
///
/// Returns [`ProtocolError::InvalidRoomCode`] unless the code is six uppercase ASCII letters or digits.
pub fn validate_room_code(code: &str) -> Result<(), ProtocolError> {
    if code.len() != ROOM_CODE_CHARS
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return Err(ProtocolError::InvalidRoomCode);
    }
    Ok(())
}

/// Validates redundant synchronization metadata against canonical state.
///
/// # Errors
///
/// Returns [`ProtocolError::InvalidSnapshot`] for revision, scenario, or hash drift.
pub fn validate_snapshot(snapshot: &MatchSnapshot) -> Result<(), ProtocolError> {
    if snapshot.revision != snapshot.state.revision {
        return Err(ProtocolError::InvalidSnapshot(
            "revision does not match state".to_owned(),
        ));
    }
    if snapshot.scenario_id != snapshot.state.scenario_id {
        return Err(ProtocolError::InvalidSnapshot(
            "scenario ID does not match state".to_owned(),
        ));
    }
    let state_hash = snapshot
        .state
        .canonical_hash()
        .map_err(|error| ProtocolError::InvalidSnapshot(error.to_string()))?;
    if snapshot.state_hash != state_hash {
        return Err(ProtocolError::InvalidSnapshot(
            "state hash does not match state".to_owned(),
        ));
    }
    if snapshot.scenario_hash.is_empty() {
        return Err(ProtocolError::InvalidSnapshot(
            "scenario hash is missing".to_owned(),
        ));
    }
    Ok(())
}

/// Builds the stable stale-revision response with the current authoritative snapshot.
pub fn stale_revision_message(snapshot: MatchSnapshot) -> ServerMessage {
    ServerMessage::Error {
        protocol_version: PROTOCOL_VERSION,
        code: ErrorCode::StaleRevision,
        message: "The match changed; the current state has been restored.".to_owned(),
        retryable: true,
        snapshot: Some(Box::new(snapshot)),
    }
}

fn validate_scenario_id(id: &str) -> Result<(), ProtocolError> {
    if id.is_empty()
        || id.chars().count() > MAX_SCENARIO_ID_CHARS
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ProtocolError::InvalidScenarioId);
    }
    Ok(())
}

fn validate_version(found: u16) -> Result<(), ProtocolError> {
    if found != PROTOCOL_VERSION {
        return Err(ProtocolError::IncompatibleVersion {
            found,
            expected: PROTOCOL_VERSION,
        });
    }
    Ok(())
}

fn decode_bounded<T: DeserializeOwned>(bytes: &[u8], maximum: usize) -> Result<T, ProtocolError> {
    if bytes.len() > maximum {
        return Err(ProtocolError::MessageTooLarge {
            actual: bytes.len(),
            maximum,
        });
    }
    serde_json::from_slice(bytes).map_err(|error| ProtocolError::Malformed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use crownline_core::{
        ClockSettings, PromotionEligibility, RealmControlScore,
        scenario::{Player, PromotionUnlockRules},
        state::{MandatoryChoice, PromotionKind, TurnPhase},
    };

    use super::*;

    #[test]
    fn example_http_json_round_trips_and_validates_limits() {
        let health = HealthResponse {
            protocol_version: PROTOCOL_VERSION,
            status: ServiceStatus::Ok,
            liveness: ServiceStatus::Ok,
            database: ServiceStatus::NotChecked,
        };
        assert_eq!(
            serde_json::from_str::<HealthResponse>(&serde_json::to_string(&health).unwrap())
                .unwrap(),
            health
        );

        let create = CreateRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Ada".to_owned(),
            scenario_id: "standard".to_owned(),
            clock: Some(ClockSettings {
                base_minutes: 30,
                increment_seconds: 5,
            }),
        };
        let bytes = serde_json::to_vec(&create).unwrap();
        let decoded: CreateRoomRequest = decode_http_request(&bytes).unwrap();
        assert_eq!(decoded, create);
        assert_eq!(validate_create_room(&decoded), Ok(()));

        let join = JoinRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Grace".to_owned(),
            room_code: "A7B9C2".to_owned(),
        };
        assert_eq!(
            serde_json::from_str::<JoinRoomRequest>(&serde_json::to_string(&join).unwrap())
                .unwrap(),
            join
        );
        assert_eq!(validate_join_room(&join), Ok(()));
    }

    #[test]
    fn websocket_examples_round_trip_for_every_message_family() {
        let match_id = Uuid::nil();
        let context = MutationContext {
            match_id,
            expected_revision: 4,
            idempotency_key: Uuid::nil(),
        };
        let messages = [
            ClientMessage::Authenticate {
                protocol_version: PROTOCOL_VERSION,
                room_code: "A7B9C2".to_owned(),
                reconnect_token: ReconnectToken::issued("a".repeat(64)),
            },
            ClientMessage::Ready {
                protocol_version: PROTOCOL_VERSION,
                context,
            },
            ClientMessage::Draw {
                protocol_version: PROTOCOL_VERSION,
                context,
                command: DrawCommand::Offer,
            },
            ClientMessage::Rematch {
                protocol_version: PROTOCOL_VERSION,
                context,
                command: RematchCommand::Request,
            },
        ];
        for message in messages {
            let bytes = serde_json::to_vec(&message).unwrap();
            assert_eq!(decode_client_message(&bytes).unwrap(), message);
        }

        let server = ServerMessage::ConnectionState {
            protocol_version: PROTOCOL_VERSION,
            match_id,
            state: ConnectionState::OpponentDisconnected,
        };
        assert_eq!(
            serde_json::from_str::<ServerMessage>(&serde_json::to_string(&server).unwrap())
                .unwrap(),
            server
        );
        assert_eq!(Player::North, Player::North);
    }

    #[test]
    fn unknown_variants_versions_and_oversized_inputs_are_recoverable() {
        let unknown = br#"{"type":"launch_missiles","protocol_version":1}"#;
        assert!(matches!(
            decode_client_message(unknown),
            Err(ProtocolError::Malformed(_))
        ));
        let future = br#"{"type":"ready","protocol_version":99,"context":{"match_id":"00000000-0000-0000-0000-000000000000","expected_revision":0,"idempotency_key":"00000000-0000-0000-0000-000000000000"}}"#;
        assert_eq!(
            decode_client_message(future),
            Err(ProtocolError::IncompatibleVersion {
                found: 99,
                expected: PROTOCOL_VERSION
            })
        );
        let oversized = vec![b' '; MAX_CLIENT_MESSAGE_BYTES + 1];
        assert!(matches!(
            decode_client_message(&oversized),
            Err(ProtocolError::MessageTooLarge { .. })
        ));
    }

    #[test]
    fn room_fields_and_custom_clocks_have_explicit_validation() {
        assert_eq!(
            validate_player_name("\n"),
            Err(ProtocolError::InvalidPlayerName)
        );
        assert_eq!(
            validate_room_code("abc123"),
            Err(ProtocolError::InvalidRoomCode)
        );
        let invalid = CreateRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Ada".to_owned(),
            scenario_id: "../private".to_owned(),
            clock: None,
        };
        assert_eq!(
            validate_create_room(&invalid),
            Err(ProtocolError::InvalidScenarioId)
        );
    }

    #[test]
    fn draw_actions_cannot_hide_in_gameplay_messages() {
        let request = ClientMessage::Action {
            protocol_version: PROTOCOL_VERSION,
            request: ActionRequest {
                context: MutationContext {
                    match_id: Uuid::nil(),
                    expected_revision: 0,
                    idempotency_key: Uuid::nil(),
                },
                action: Action::OfferDraw {
                    player: Player::North,
                },
            },
        };
        assert!(matches!(
            decode_client_message(&serde_json::to_vec(&request).unwrap()),
            Err(ProtocolError::Malformed(_))
        ));
    }

    fn fixture_snapshot() -> MatchSnapshot {
        let scenario: crownline_core::ScenarioDefinition =
            ron::from_str(include_str!("../../../assets/scenarios/standard.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        MatchSnapshot {
            match_id: Uuid::nil(),
            revision: state.revision,
            scenario_id: state.scenario_id.clone(),
            scenario_hash: scenario.canonical_hash().unwrap(),
            state_hash: state.canonical_hash().unwrap(),
            state,
            room_state: ConnectionState::Connected,
            rematch_state: None,
        }
    }

    #[test]
    fn accepted_mutation_result_always_contains_the_full_valid_snapshot() {
        let snapshot = fixture_snapshot();
        assert_eq!(validate_snapshot(&snapshot), Ok(()));
        let result = MutationResult {
            match_id: snapshot.match_id,
            idempotency_key: Uuid::nil(),
            snapshot,
        };
        let acknowledgement = ServerMessage::Acknowledgement {
            protocol_version: PROTOCOL_VERSION,
            result: Box::new(result.clone()),
        };
        let retried: ServerMessage =
            serde_json::from_slice(&serde_json::to_vec(&acknowledgement).unwrap()).unwrap();
        assert_eq!(retried, acknowledgement);
        let ServerMessage::Acknowledgement {
            result: decoded, ..
        } = retried
        else {
            unreachable!();
        };
        assert_eq!(*decoded, result);
    }

    #[test]
    fn network_snapshot_preserves_frozen_promotion_eligibility() {
        let mut snapshot = fixture_snapshot();
        let pawn = *snapshot.state.pieces.keys().next().unwrap();
        let control = RealmControlScore {
            owned_settlements: 2,
            governed_settlements: 2,
            established_settlements: 0,
        };
        let eligibility =
            PromotionEligibility::from_control(control, PromotionUnlockRules::default());
        assert!(eligibility.allows(PromotionKind::Rook));
        assert!(!eligibility.allows(PromotionKind::Queen));
        snapshot.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn,
                site_index: 0,
                eligibility: eligibility.clone(),
            }],
        };
        snapshot.state_hash = snapshot.state.canonical_hash().unwrap();

        let message = ServerMessage::Snapshot {
            protocol_version: PROTOCOL_VERSION,
            snapshot: Box::new(snapshot.clone()),
        };
        let decoded: ServerMessage =
            serde_json::from_slice(&serde_json::to_vec(&message).unwrap()).unwrap();
        assert_eq!(decoded, message);
        let ServerMessage::Snapshot {
            snapshot: decoded, ..
        } = decoded
        else {
            unreachable!();
        };
        assert_eq!(validate_snapshot(&decoded), Ok(()));
        let TurnPhase::ResolvingChoices { queue } = &decoded.state.phase else {
            panic!("promotion phase must survive network decoding");
        };
        let MandatoryChoice::Promote {
            eligibility: decoded,
            ..
        } = &queue[0]
        else {
            panic!("promotion choice must survive network decoding");
        };
        assert_eq!(decoded, &eligibility);
    }

    #[test]
    fn duplicate_idempotency_key_returns_original_result_without_applying_twice() {
        let current = fixture_snapshot();
        let context = MutationContext {
            match_id: current.match_id,
            expected_revision: current.revision,
            idempotency_key: Uuid::new_v4(),
        };
        let mut cache = IdempotencyCache::default();
        let mut applications = 0;
        let first = cache.resolve(context, &current, || {
            applications += 1;
            Ok::<_, ()>(current.clone())
        });
        let second = cache.resolve(context, &current, || {
            applications += 1;
            Ok::<_, ()>(current.clone())
        });
        assert_eq!(applications, 1);
        let MutationResolution::Accepted {
            result: original,
            duplicate: false,
        } = first
        else {
            panic!("first mutation was not accepted");
        };
        let MutationResolution::Accepted {
            result: retried,
            duplicate: true,
        } = second
        else {
            panic!("retry did not return cached result");
        };
        assert_eq!(retried, original);
    }

    #[test]
    fn stale_revision_uses_a_stable_code_and_current_snapshot() {
        let snapshot = fixture_snapshot();
        let message = stale_revision_message(snapshot.clone());
        let ServerMessage::Error {
            code,
            retryable,
            snapshot: Some(current),
            ..
        } = message
        else {
            unreachable!();
        };
        assert_eq!(code, ErrorCode::StaleRevision);
        assert!(retryable);
        assert_eq!(*current, snapshot);
    }

    #[test]
    fn snapshot_rejects_redundant_metadata_drift() {
        let mut snapshot = fixture_snapshot();
        snapshot.revision += 1;
        assert!(matches!(
            validate_snapshot(&snapshot),
            Err(ProtocolError::InvalidSnapshot(_))
        ));
    }
}
