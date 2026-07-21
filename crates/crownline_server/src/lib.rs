//! Authoritative Crownlines server services.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use crownline_protocol::{
    CreateRoomRequest, CreateRoomResponse, ErrorCode, HealthResponse, JoinRoomRequest,
    JoinRoomResponse, PROTOCOL_VERSION, ServerMessage, ServiceStatus,
};
use tokio::sync::Mutex;

pub mod rooms;

use rooms::{RoomError, RoomService, ScenarioCatalog};

pub type SharedRooms = Arc<Mutex<RoomService>>;
type HttpError = (StatusCode, Json<ServerMessage>);

pub fn app() -> Router {
    let rooms = Arc::new(Mutex::new(RoomService::new(ScenarioCatalog::installed())));
    Router::new()
        .route("/health", get(health))
        .route("/rooms", post(create_room))
        .route("/rooms/join", post(join_room))
        .with_state(rooms)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        protocol_version: PROTOCOL_VERSION,
        status: ServiceStatus::Ok,
    })
}

async fn create_room(
    State(rooms): State<SharedRooms>,
    Json(request): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>, HttpError> {
    rooms
        .lock()
        .await
        .create(request)
        .map(|created| Json(created.response))
        .map_err(http_error)
}

async fn join_room(
    State(rooms): State<SharedRooms>,
    Json(request): Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, HttpError> {
    rooms
        .lock()
        .await
        .join(request)
        .map(|joined| Json(joined.response))
        .map_err(http_error)
}

fn http_error(error: RoomError) -> HttpError {
    let (status, code, message) = match error {
        RoomError::InvalidRequest | RoomError::UnknownScenario => (
            StatusCode::BAD_REQUEST,
            ErrorCode::InvalidRequest,
            "The room request is invalid.",
        ),
        RoomError::NotFound => (
            StatusCode::NOT_FOUND,
            ErrorCode::RoomNotFound,
            "The room was not found.",
        ),
        RoomError::Full => (
            StatusCode::CONFLICT,
            ErrorCode::RoomFull,
            "The room is full.",
        ),
        RoomError::Unauthorized => (
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthorized,
            "The seat credential is invalid.",
        ),
        RoomError::WrongPhase => (
            StatusCode::CONFLICT,
            ErrorCode::InvalidRequest,
            "The room is not in the required phase.",
        ),
        RoomError::CodeSpaceExhausted => (
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::Internal,
            "A room code is temporarily unavailable.",
        ),
    };
    (
        status,
        Json(ServerMessage::Error {
            protocol_version: PROTOCOL_VERSION,
            code,
            message: message.to_owned(),
            retryable: status.is_server_error(),
            snapshot: None,
        }),
    )
}
