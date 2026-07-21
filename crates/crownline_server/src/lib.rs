//! Authoritative Crownlines server services.

use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, post},
};
use crownline_protocol::{
    CreateRoomRequest, CreateRoomResponse, ErrorCode, HealthResponse, JoinRoomRequest,
    JoinRoomResponse, PROTOCOL_VERSION, ServerMessage, ServiceStatus,
};
use tokio::sync::Mutex;

pub mod actors;
pub mod authority;
pub mod database;
pub mod limits;
pub mod recovery;
pub mod rooms;

use limits::{LimitKind, RequestLimiter, ServerLimits};
use recovery::{MatchRepository, RecoveryError};
use rooms::{RoomError, RoomService, ScenarioCatalog};

pub type SharedRooms = Arc<Mutex<RoomService>>;
type HttpError = (StatusCode, Json<ServerMessage>);

#[derive(Clone)]
struct AppState {
    rooms: SharedRooms,
    limiter: Arc<Mutex<RequestLimiter>>,
    limits: ServerLimits,
    _repository: Arc<Mutex<MatchRepository>>,
    _restored_authorities: Arc<Mutex<BTreeMap<uuid::Uuid, authority::AuthoritativeMatch>>>,
}

pub fn app() -> Router {
    app_with_limits(ServerLimits::default())
}

/// Builds an ephemeral server using an in-memory database.
///
/// # Panics
///
/// Panics only if this process cannot initialize a fresh in-memory `SQLite` database.
pub fn app_with_limits(limits: ServerLimits) -> Router {
    let database = database::Database::open_in_memory().expect("in-memory SQLite must open");
    app_with_repository(limits, MatchRepository::new(database))
        .expect("fresh in-memory repository must restore")
}

/// Builds the server around a durable database after validating active matches.
///
/// # Errors
///
/// Returns database, migration, or startup recovery errors.
pub fn app_with_database(
    limits: ServerLimits,
    path: impl AsRef<Path>,
    durability: database::Durability,
) -> Result<Router, RecoveryError> {
    let database = database::Database::open(path, durability)?;
    app_with_repository(limits, MatchRepository::new(database))
}

fn app_with_repository(
    limits: ServerLimits,
    mut repository: MatchRepository,
) -> Result<Router, RecoveryError> {
    let catalog = ScenarioCatalog::installed();
    let restored = repository.restore_active(&catalog)?;
    tracing::info!(
        restored = restored.matches.len(),
        quarantined = restored.quarantined.len(),
        "startup match recovery complete"
    );
    let restored_authorities = restored
        .matches
        .into_iter()
        .map(|restored| (restored.match_id, restored.authority))
        .collect();
    let rooms = Arc::new(Mutex::new(
        RoomService::new(catalog).with_max_rooms(limits.max_rooms),
    ));
    let state = AppState {
        rooms,
        limiter: Arc::new(Mutex::new(RequestLimiter::default())),
        limits,
        _repository: Arc::new(Mutex::new(repository)),
        _restored_authorities: Arc::new(Mutex::new(restored_authorities)),
    };
    Ok(Router::new()
        .route("/health", get(health))
        .route("/rooms", post(create_room))
        .route("/rooms/join", post(join_room))
        .layer(DefaultBodyLimit::max(limits.max_http_body_bytes))
        .with_state(state))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        protocol_version: PROTOCOL_VERSION,
        status: ServiceStatus::Ok,
    })
}

async fn create_room(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(request): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>, HttpError> {
    check_limit(
        &state,
        LimitKind::Create,
        peer.ip().to_string(),
        state.limits.create_per_ip_per_minute,
    )
    .await?;
    let mut rooms = state.rooms.lock().await;
    rooms.expire_idle_pregame(Instant::now(), state.limits.pregame_idle_timeout);
    rooms
        .create(request)
        .map(|created| Json(created.response))
        .map_err(http_error)
}

async fn join_room(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(request): Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, HttpError> {
    check_limit(
        &state,
        LimitKind::Join,
        peer.ip().to_string(),
        state.limits.join_per_ip_per_minute,
    )
    .await?;
    check_limit(
        &state,
        LimitKind::RoomOperation,
        rooms::normalize_code(&request.room_code),
        state.limits.operations_per_room_per_minute,
    )
    .await?;
    let mut rooms = state.rooms.lock().await;
    rooms.expire_idle_pregame(Instant::now(), state.limits.pregame_idle_timeout);
    rooms
        .join(request)
        .map(|joined| Json(joined.response))
        .map_err(http_error)
}

async fn check_limit(
    state: &AppState,
    kind: LimitKind,
    scope: String,
    maximum: u32,
) -> Result<(), HttpError> {
    let mut limiter = state.limiter.lock().await;
    let now = Instant::now();
    limiter.discard_expired(now);
    if limiter.check(kind, scope, maximum, now) {
        Ok(())
    } else {
        Err(http_error(RoomError::RateLimited))
    }
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
        RoomError::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::RateLimited,
            "Too many credential attempts.",
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
