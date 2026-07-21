//! Bounded, versioned wire messages shared by Crownlines clients and servers.

use crownline_core::{
    Action, ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES, MatchState,
    scenario::Player,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use uuid::Uuid;

/// Protocol version supported by this build.
pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_HTTP_REQUEST_BYTES: usize = 8 * 1024;
pub const MAX_CLIENT_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_PLAYER_NAME_CHARS: usize = 24;
pub const ROOM_CODE_CHARS: usize = 6;
pub const MAX_SCENARIO_ID_CHARS: usize = 64;
pub const MAX_RECONNECT_TOKEN_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthRequest {
    pub protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub protocol_version: u16,
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Ok,
    Degraded,
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
    pub reconnect_token: String,
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
    pub reconnect_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Ready {
        protocol_version: u16,
        match_id: Uuid,
    },
    Action {
        protocol_version: u16,
        request: ActionRequest,
    },
    Draw {
        protocol_version: u16,
        match_id: Uuid,
        expected_revision: u64,
        idempotency_key: Uuid,
        command: DrawCommand,
    },
    Rematch {
        protocol_version: u16,
        match_id: Uuid,
        idempotency_key: Uuid,
        command: RematchCommand,
    },
}

impl ClientMessage {
    pub const fn protocol_version(&self) -> u16 {
        match self {
            Self::Ready {
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
    pub match_id: Uuid,
    pub expected_revision: u64,
    pub idempotency_key: Uuid,
    pub action: Action,
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
        match_id: Uuid,
        idempotency_key: Uuid,
        revision: u64,
    },
    Error {
        protocol_version: u16,
        code: ErrorCode,
        message: String,
        retryable: bool,
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
    pub state: MatchState,
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
    use crownline_core::{ClockSettings, scenario::Player};

    use super::*;

    #[test]
    fn example_http_json_round_trips_and_validates_limits() {
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
        let messages = [
            ClientMessage::Ready {
                protocol_version: PROTOCOL_VERSION,
                match_id,
            },
            ClientMessage::Draw {
                protocol_version: PROTOCOL_VERSION,
                match_id,
                expected_revision: 4,
                idempotency_key: Uuid::nil(),
                command: DrawCommand::Offer,
            },
            ClientMessage::Rematch {
                protocol_version: PROTOCOL_VERSION,
                match_id,
                idempotency_key: Uuid::nil(),
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
        let future = br#"{"type":"ready","protocol_version":99,"match_id":"00000000-0000-0000-0000-000000000000"}"#;
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
                match_id: Uuid::nil(),
                expected_revision: 0,
                idempotency_key: Uuid::nil(),
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
}
