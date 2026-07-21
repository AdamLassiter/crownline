//! Authoritative Crownlines server services.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex as StdMutex},
    time::Instant,
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use crownline_protocol::{
    CreateRoomRequest, CreateRoomResponse, ErrorCode, HealthResponse, JoinRoomRequest,
    JoinRoomResponse, PROTOCOL_VERSION, ServerMessage, ServiceStatus,
};
use tokio::sync::Mutex;

pub mod actors;
pub mod authority;
pub mod connections;
pub mod database;
pub mod limits;
pub mod recovery;
pub mod rooms;

use crate::actors::MatchExecutor as _;
use actors::{DEFAULT_ACTOR_QUEUE_CAPACITY, MatchActorRegistry};
use connections::{AuthorityLoader, SnapshotHub};
use limits::{ConnectionRegistry, LimitKind, RequestLimiter, ServerLimits};
use recovery::{MatchRepository, RecoveryError};
use rooms::{RoomError, RoomService, ScenarioCatalog};

pub type SharedRooms = Arc<Mutex<RoomService>>;
type HttpError = (StatusCode, Json<ServerMessage>);

#[derive(Clone)]
struct AppState {
    rooms: SharedRooms,
    limiter: Arc<Mutex<RequestLimiter>>,
    limits: ServerLimits,
    repository: Arc<StdMutex<MatchRepository>>,
    authorities: Arc<StdMutex<BTreeMap<uuid::Uuid, authority::AuthoritativeMatch>>>,
    actors: Arc<MatchActorRegistry>,
    hub: Arc<SnapshotHub>,
    connections: Arc<Mutex<ConnectionRegistry>>,
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
    let mut room_service = RoomService::new(catalog.clone()).with_max_rooms(limits.max_rooms);
    let mut restored_authorities = BTreeMap::new();
    let hub = Arc::new(SnapshotHub::default());
    for restored in restored.matches {
        let snapshot = restored.authority.snapshot();
        let state = snapshot.state.clone();
        room_service
            .restore_started_room(restored.room, state)
            .map_err(|_| RecoveryError::InvalidState)?;
        hub.register(snapshot);
        restored_authorities.insert(restored.match_id, restored.authority);
    }
    let repository = Arc::new(StdMutex::new(repository));
    let authorities = Arc::new(StdMutex::new(restored_authorities));
    let loader = Arc::new(AuthorityLoader::new(
        Arc::clone(&authorities),
        Arc::clone(&repository),
        Arc::clone(&hub),
        catalog,
    ));
    let actors = Arc::new(MatchActorRegistry::new(
        loader,
        DEFAULT_ACTOR_QUEUE_CAPACITY,
    ));
    let rooms = Arc::new(Mutex::new(room_service));
    let state = AppState {
        rooms,
        limiter: Arc::new(Mutex::new(RequestLimiter::default())),
        limits,
        repository,
        authorities,
        actors,
        hub,
        connections: Arc::new(Mutex::new(ConnectionRegistry::default())),
    };
    Ok(Router::new()
        .route("/health", get(health))
        .route("/rooms", post(create_room))
        .route("/rooms/join", post(join_room))
        .route("/ws", get(websocket_upgrade))
        .layer(DefaultBodyLimit::max(limits.max_http_body_bytes))
        .with_state(state))
}

async fn websocket_upgrade(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    websocket: WebSocketUpgrade,
) -> Response {
    if !state
        .connections
        .lock()
        .await
        .try_open(peer.ip(), &state.limits)
    {
        return http_error(RoomError::RateLimited).into_response();
    }
    let rooms = Arc::clone(&state.rooms);
    let hub = Arc::clone(&state.hub);
    let connections = Arc::clone(&state.connections);
    let repository = Arc::clone(&state.repository);
    let authorities = Arc::clone(&state.authorities);
    let actors = Arc::clone(&state.actors);
    websocket
        .max_message_size(crownline_protocol::MAX_CLIENT_MESSAGE_BYTES)
        .max_frame_size(crownline_protocol::MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            connections::serve_socket(
                socket,
                rooms,
                hub,
                connections,
                repository,
                authorities,
                actors,
                peer.ip(),
            )
        })
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
    let created = rooms.create(request).map_err(http_error)?;
    state.hub.register_room(created.response.match_id);
    Ok(Json(created.response))
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
    let joined = rooms.join(request).map_err(http_error)?;
    state.hub.publish_room_state(
        joined.response.match_id,
        crownline_protocol::ConnectionState::WaitingForReady,
        None,
    );
    Ok(Json(joined.response))
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
