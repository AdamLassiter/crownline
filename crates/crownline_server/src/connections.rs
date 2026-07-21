use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::extract::ws::{Message, WebSocket};
use crownline_core::{Action, scenario::Player};
use crownline_protocol::{
    ClientMessage, ConnectionState, DrawCommand, ErrorCode, MatchSnapshot, PROTOCOL_VERSION,
    RematchCommand, RematchState, ServerMessage, decode_client_message, stale_revision_message,
};
use futures_util::StreamExt;
use tokio::sync::{Mutex as AsyncMutex, broadcast, watch};
use tracing::warn;
use uuid::Uuid;

use crate::{
    SharedRooms,
    actors::{
        ActorSubmitError, CommandRejection, CommandTiming, ExecutionError, MatchActorRegistry,
        MatchCommand, MatchCommandResult, MatchExecutor, MatchLoader,
    },
    authority::AuthoritativeMatch,
    limits::ConnectionRegistry,
    recovery::MatchRepository,
    rooms::{RoomPhase, ScenarioCatalog},
};

pub const ROOM_EVENT_CAPACITY: usize = 32;
const AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(10);

struct HubEntry {
    snapshot: watch::Sender<Option<MatchSnapshot>>,
    events: broadcast::Sender<ServerMessage>,
    connected_seats: BTreeSet<Player>,
}

#[derive(Default)]
pub struct SnapshotHub {
    matches: Mutex<BTreeMap<Uuid, HubEntry>>,
}

pub struct MatchSubscription {
    pub snapshots: watch::Receiver<Option<MatchSnapshot>>,
    pub events: broadcast::Receiver<ServerMessage>,
}

impl SnapshotHub {
    pub fn register_room(&self, match_id: Uuid) {
        let mut matches = self
            .matches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        matches.entry(match_id).or_insert_with(|| {
            let (snapshot, _) = watch::channel(None);
            let (events, _) = broadcast::channel(ROOM_EVENT_CAPACITY);
            HubEntry {
                snapshot,
                events,
                connected_seats: BTreeSet::new(),
            }
        });
    }

    pub fn register(&self, snapshot: MatchSnapshot) {
        let mut matches = self
            .matches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = matches.get(&snapshot.match_id) {
            entry.snapshot.send_replace(Some(snapshot));
            return;
        }
        let (snapshot_sender, _) = watch::channel(Some(snapshot.clone()));
        let (event_sender, _) = broadcast::channel(ROOM_EVENT_CAPACITY);
        matches.insert(
            snapshot.match_id,
            HubEntry {
                snapshot: snapshot_sender,
                events: event_sender,
                connected_seats: BTreeSet::new(),
            },
        );
    }

    /// Replaces the latest snapshot without waiting for any connection.
    pub fn publish_committed(&self, snapshot: MatchSnapshot) {
        self.register(snapshot);
    }

    pub fn publish_room_state(
        &self,
        match_id: Uuid,
        connection: ConnectionState,
        rematch: Option<RematchState>,
    ) {
        let matches = self
            .matches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(entry) = matches.get(&match_id) else {
            return;
        };
        let current = { entry.snapshot.borrow().clone() };
        if let Some(mut snapshot) = current {
            snapshot.room_state = connection;
            snapshot.rematch_state = rematch;
            entry.snapshot.send_replace(Some(snapshot));
        }
        let _ = entry.events.send(ServerMessage::ConnectionState {
            protocol_version: PROTOCOL_VERSION,
            match_id,
            state: connection,
        });
        if let Some(state) = rematch {
            let _ = entry.events.send(ServerMessage::RematchState {
                protocol_version: PROTOCOL_VERSION,
                match_id,
                state,
            });
        }
    }

    pub fn set_seat_connected(&self, match_id: Uuid, seat: Player, connected: bool) {
        let mut matches = self
            .matches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(entry) = matches.get_mut(&match_id) else {
            return;
        };
        if connected {
            entry.connected_seats.insert(seat);
        } else {
            entry.connected_seats.remove(&seat);
        }
        let terminal = entry
            .snapshot
            .borrow()
            .as_ref()
            .is_some_and(|snapshot| snapshot.state.outcome.is_some());
        let state = if terminal {
            ConnectionState::Finished
        } else if entry.connected_seats.len() == 2 {
            ConnectionState::Connected
        } else {
            ConnectionState::OpponentDisconnected
        };
        let current = { entry.snapshot.borrow().clone() };
        if let Some(mut snapshot) = current {
            snapshot.room_state = state;
            entry.snapshot.send_replace(Some(snapshot));
        }
        let _ = entry.events.send(ServerMessage::ConnectionState {
            protocol_version: PROTOCOL_VERSION,
            match_id,
            state,
        });
    }

    pub fn subscribe(&self, match_id: Uuid) -> Option<MatchSubscription> {
        let matches = self
            .matches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = matches.get(&match_id)?;
        Some(MatchSubscription {
            snapshots: entry.snapshot.subscribe(),
            events: entry.events.subscribe(),
        })
    }

    pub fn latest(&self, match_id: Uuid) -> Option<MatchSnapshot> {
        self.matches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&match_id)
            .and_then(|entry| entry.snapshot.borrow().clone())
    }
}

pub struct CommittingExecutor {
    authority: AuthoritativeMatch,
    repository: Arc<Mutex<MatchRepository>>,
    hub: Arc<SnapshotHub>,
}

pub struct AuthorityLoader {
    authorities: Arc<Mutex<BTreeMap<Uuid, AuthoritativeMatch>>>,
    repository: Arc<Mutex<MatchRepository>>,
    hub: Arc<SnapshotHub>,
    catalog: ScenarioCatalog,
}

impl AuthorityLoader {
    pub fn new(
        authorities: Arc<Mutex<BTreeMap<Uuid, AuthoritativeMatch>>>,
        repository: Arc<Mutex<MatchRepository>>,
        hub: Arc<SnapshotHub>,
        catalog: ScenarioCatalog,
    ) -> Self {
        Self {
            authorities,
            repository,
            hub,
            catalog,
        }
    }
}

impl MatchLoader for AuthorityLoader {
    fn load(&self, match_id: Uuid) -> Result<Box<dyn MatchExecutor>, String> {
        let authority = if let Some(authority) = self
            .authorities
            .lock()
            .map_err(|_| "authority store lock failed".to_owned())?
            .remove(&match_id)
        {
            authority
        } else {
            self.repository
                .lock()
                .map_err(|_| "repository lock failed".to_owned())?
                .restore_match(match_id, &self.catalog)
                .map_err(|error| error.to_string())?
                .authority
        };
        Ok(Box::new(CommittingExecutor::new(
            authority,
            Arc::clone(&self.repository),
            Arc::clone(&self.hub),
        )))
    }
}

impl CommittingExecutor {
    pub fn new(
        authority: AuthoritativeMatch,
        repository: Arc<Mutex<MatchRepository>>,
        hub: Arc<SnapshotHub>,
    ) -> Self {
        Self {
            authority,
            repository,
            hub,
        }
    }

    fn commit_prepared(&mut self) -> Result<MatchSnapshot, ExecutionError> {
        let prepared = self
            .authority
            .take_prepared_transition()
            .ok_or_else(|| ExecutionError::Fatal("authority produced no transition".to_owned()))?;
        let snapshot = self
            .repository
            .lock()
            .map_err(|_| ExecutionError::Fatal("repository lock failed".to_owned()))?
            .commit_transition(&prepared)
            .map_err(|error| ExecutionError::Fatal(error.to_string()))?;
        self.hub.publish_committed(snapshot.clone());
        Ok(snapshot)
    }
}

impl MatchExecutor for CommittingExecutor {
    fn snapshot(&self) -> MatchSnapshot {
        self.authority.snapshot()
    }

    fn execute(
        &mut self,
        idempotency_key: Uuid,
        seat: Player,
        action: &Action,
        timing: CommandTiming,
    ) -> Result<MatchSnapshot, ExecutionError> {
        match self
            .authority
            .execute(idempotency_key, seat, action, timing)
        {
            Ok(_snapshot) if self.authority.has_prepared_transition() => self.commit_prepared(),
            Ok(snapshot) => Ok(snapshot),
            Err(ExecutionError::Rejected(CommandRejection::ExpiredTime)) => {
                self.commit_prepared()?;
                Err(ExecutionError::Rejected(CommandRejection::ExpiredTime))
            }
            Err(error) => Err(error),
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn serve_socket(
    mut socket: WebSocket,
    rooms: SharedRooms,
    hub: Arc<SnapshotHub>,
    connections: Arc<AsyncMutex<ConnectionRegistry>>,
    repository: Arc<Mutex<MatchRepository>>,
    authorities: Arc<Mutex<BTreeMap<Uuid, AuthoritativeMatch>>>,
    actors: Arc<MatchActorRegistry>,
    peer_ip: IpAddr,
) {
    let authenticated = tokio::time::timeout(AUTHENTICATION_TIMEOUT, socket.next()).await;
    let Some(Ok(message)) = authenticated.ok().flatten() else {
        close_connection(&connections, peer_ip).await;
        return;
    };
    let Some(bytes) = message_bytes(message) else {
        close_connection(&connections, peer_ip).await;
        return;
    };
    let Ok(ClientMessage::Authenticate {
        room_code,
        reconnect_token,
        ..
    }) = decode_client_message(&bytes)
    else {
        let _ = send_error(&mut socket, "Authentication is required.").await;
        close_connection(&connections, peer_ip).await;
        return;
    };
    let authenticated = rooms
        .lock()
        .await
        .authenticate_seat(&room_code, reconnect_token.expose());
    let Ok((match_id, seat, phase)) = authenticated else {
        let _ = send_error(&mut socket, "The seat credential is invalid.").await;
        close_connection(&connections, peer_ip).await;
        return;
    };
    hub.register_room(match_id);
    let Some(mut subscription) = hub.subscribe(match_id) else {
        close_connection(&connections, peer_ip).await;
        return;
    };
    hub.set_seat_connected(match_id, seat, true);
    let initial_snapshot = { subscription.snapshots.borrow().clone() };
    if let Some(initial_snapshot) = initial_snapshot
        && send_snapshot(&mut socket, initial_snapshot).await.is_err()
    {
        close_connection(&connections, peer_ip).await;
        return;
    }
    if subscription.snapshots.borrow().is_none() {
        let state = match phase {
            RoomPhase::WaitingForOpponent => ConnectionState::WaitingForOpponent,
            RoomPhase::WaitingForReady => ConnectionState::WaitingForReady,
            RoomPhase::Playing => ConnectionState::Connected,
            RoomPhase::Finished => ConnectionState::Finished,
        };
        let _ = send_server_message(
            &mut socket,
            &ServerMessage::ConnectionState {
                protocol_version: PROTOCOL_VERSION,
                match_id,
                state,
            },
        )
        .await;
    }

    loop {
        tokio::select! {
            changed = subscription.snapshots.changed() => {
                if changed.is_err() {
                    break;
                }
                let snapshot = { subscription.snapshots.borrow_and_update().clone() };
                if let Some(snapshot) = snapshot
                    && send_snapshot(&mut socket, snapshot).await.is_err()
                { break; }
            }
            event = subscription.events.recv() => {
                match event {
                    Ok(message) if send_server_message(&mut socket, &message).await.is_err() => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let snapshot = { subscription.snapshots.borrow().clone() };
                        if let Some(snapshot) = snapshot
                            && send_snapshot(&mut socket, snapshot).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Ok(_) => {}
                }
            }
            inbound = socket.next() => {
                match inbound {
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(message)) => {
                        let Some(bytes) = message_bytes(message) else { continue; };
                        if !handle_authenticated_message(
                            &mut socket,
                            &bytes,
                            &room_code,
                            reconnect_token.expose(),
                            match_id,
                            seat,
                            &rooms,
                            &hub,
                            &repository,
                            &authorities,
                            &actors,
                        ).await {
                            break;
                        }
                    }
                }
            }
        }
    }
    hub.set_seat_connected(match_id, seat, false);
    close_connection(&connections, peer_ip).await;
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn handle_authenticated_message(
    socket: &mut WebSocket,
    bytes: &[u8],
    room_code: &str,
    token: &str,
    authenticated_match: Uuid,
    seat: Player,
    rooms: &SharedRooms,
    hub: &Arc<SnapshotHub>,
    repository: &Arc<Mutex<MatchRepository>>,
    authorities: &Arc<Mutex<BTreeMap<Uuid, AuthoritativeMatch>>>,
    actors: &Arc<MatchActorRegistry>,
) -> bool {
    let Ok(message) = decode_client_message(bytes) else {
        let _ = send_public_error(
            socket,
            ErrorCode::InvalidRequest,
            "Invalid message.",
            false,
            None,
        )
        .await;
        return true;
    };
    match message {
        ClientMessage::Ready { context, .. } => {
            if context.match_id != authenticated_match {
                let _ =
                    send_public_error(socket, ErrorCode::Unauthorized, "Wrong match.", false, None)
                        .await;
                return true;
            }
            let started = {
                let mut rooms = rooms.lock().await;
                match rooms.ready(room_code, token) {
                    Ok(RoomPhase::Playing) => rooms
                        .persisted_room(room_code)
                        .zip(rooms.installed_scenario_for_room(room_code)),
                    Ok(phase) => {
                        let state = match phase {
                            RoomPhase::WaitingForOpponent => ConnectionState::WaitingForOpponent,
                            RoomPhase::WaitingForReady => ConnectionState::WaitingForReady,
                            RoomPhase::Playing => ConnectionState::Connected,
                            RoomPhase::Finished => ConnectionState::Finished,
                        };
                        hub.publish_room_state(authenticated_match, state, None);
                        None
                    }
                    Err(_) => {
                        drop(rooms);
                        let _ = send_public_error(
                            socket,
                            ErrorCode::InvalidRequest,
                            "Ready was rejected.",
                            false,
                            None,
                        )
                        .await;
                        return true;
                    }
                }
            };
            if let Some((record, installed)) = started {
                if start_authority(record, installed, hub, repository, authorities)
                    .await
                    .is_err()
                {
                    let _ = send_public_error(
                        socket,
                        ErrorCode::Internal,
                        "Match startup failed.",
                        true,
                        None,
                    )
                    .await;
                    return true;
                }
                hub.publish_room_state(authenticated_match, ConnectionState::Connected, None);
            }
        }
        ClientMessage::Action { request, .. } => {
            if request.context.match_id != authenticated_match {
                let _ =
                    send_public_error(socket, ErrorCode::Unauthorized, "Wrong match.", false, None)
                        .await;
                return true;
            }
            if let Some(snapshot) = send_actor_result(
                socket,
                actors
                    .submit(MatchCommand {
                        context: request.context,
                        seat,
                        action: request.action,
                    })
                    .await,
            )
            .await
            {
                let _ = rooms.lock().await.sync_committed_state(
                    room_code,
                    snapshot.match_id,
                    snapshot.state,
                );
            }
        }
        ClientMessage::Draw {
            context, command, ..
        } => {
            if context.match_id != authenticated_match {
                let _ =
                    send_public_error(socket, ErrorCode::Unauthorized, "Wrong match.", false, None)
                        .await;
                return true;
            }
            let action = match command {
                DrawCommand::Offer => Action::OfferDraw { player: seat },
                DrawCommand::Accept => Action::RespondToDraw {
                    player: seat,
                    accept: true,
                },
                DrawCommand::Reject => Action::RespondToDraw {
                    player: seat,
                    accept: false,
                },
            };
            if let Some(snapshot) = send_actor_result(
                socket,
                actors
                    .submit(MatchCommand {
                        context,
                        seat,
                        action,
                    })
                    .await,
            )
            .await
            {
                let _ = rooms.lock().await.sync_committed_state(
                    room_code,
                    snapshot.match_id,
                    snapshot.state,
                );
            }
        }
        ClientMessage::Rematch {
            context, command, ..
        } => {
            if context.match_id != authenticated_match {
                let _ =
                    send_public_error(socket, ErrorCode::Unauthorized, "Wrong match.", false, None)
                        .await;
                return true;
            }
            match command {
                RematchCommand::Decline => {
                    if rooms.lock().await.decline_rematch(room_code, token).is_ok() {
                        hub.publish_room_state(
                            authenticated_match,
                            ConnectionState::Finished,
                            Some(RematchState::Declined),
                        );
                    }
                }
                RematchCommand::Request | RematchCommand::Accept => {
                    let rematch = {
                        let mut rooms = rooms.lock().await;
                        match rooms.accept_rematch(room_code, token) {
                            Ok(RoomPhase::Playing) => Some((
                                rooms.persisted_room(room_code),
                                rooms.installed_scenario_for_room(room_code),
                            )),
                            Ok(RoomPhase::Finished) => None,
                            _ => return true,
                        }
                    };
                    match rematch {
                        None => hub.publish_room_state(
                            authenticated_match,
                            ConnectionState::Finished,
                            Some(RematchState::Requested),
                        ),
                        Some((Some(record), Some(installed))) => {
                            if start_authority(record, installed, hub, repository, authorities)
                                .await
                                .is_err()
                            {
                                let _ = send_public_error(
                                    socket,
                                    ErrorCode::Internal,
                                    "Rematch startup failed.",
                                    true,
                                    None,
                                )
                                .await;
                                return true;
                            }
                            hub.publish_room_state(
                                authenticated_match,
                                ConnectionState::Finished,
                                Some(RematchState::Accepted),
                            );
                            return false;
                        }
                        Some(_) => {
                            let _ = send_public_error(
                                socket,
                                ErrorCode::Internal,
                                "Rematch state is invalid.",
                                true,
                                None,
                            )
                            .await;
                        }
                    }
                }
            }
        }
        ClientMessage::Leave { context, .. } => {
            if context.match_id != authenticated_match {
                let _ =
                    send_public_error(socket, ErrorCode::Unauthorized, "Wrong match.", false, None)
                        .await;
                return true;
            }
            let _ = rooms.lock().await.leave_lobby(room_code, token);
            return false;
        }
        ClientMessage::Authenticate { .. } => {
            let _ = send_public_error(
                socket,
                ErrorCode::InvalidRequest,
                "Already authenticated.",
                false,
                None,
            )
            .await;
        }
    }
    true
}

async fn start_authority(
    record: crate::rooms::PersistedRoomRecord,
    installed: crate::rooms::InstalledScenario,
    hub: &Arc<SnapshotHub>,
    repository: &Arc<Mutex<MatchRepository>>,
    authorities: &Arc<Mutex<BTreeMap<Uuid, AuthoritativeMatch>>>,
) -> Result<(), ()> {
    if hub.latest(record.match_id).is_some() {
        return Ok(());
    }
    let now = std::time::SystemTime::now();
    let authority =
        AuthoritativeMatch::new(record.match_id, installed.definition, record.clock, now)
            .map_err(|_| ())?;
    let image = authority.persistence_image(now).map_err(|_| ())?;
    let repository_for_task = Arc::clone(repository);
    let record_for_task = record.clone();
    let registration = tokio::task::spawn_blocking(move || {
        repository_for_task
            .lock()
            .map_err(|_| "repository lock failed".to_owned())?
            .register_match(&record_for_task, &image)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| ())?;
    registration.map_err(|_| ())?;
    let snapshot = authority.snapshot();
    authorities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(record.match_id, authority);
    hub.publish_committed(snapshot);
    Ok(())
}

async fn send_actor_result(
    socket: &mut WebSocket,
    result: Result<MatchCommandResult, ActorSubmitError>,
) -> Option<MatchSnapshot> {
    let committed = match &result {
        Ok(MatchCommandResult::Accepted(result)) => Some(result.snapshot.clone()),
        Ok(MatchCommandResult::Rejected { snapshot, .. }) => Some(snapshot.clone()),
        Ok(MatchCommandResult::Stale(_)) | Err(_) => None,
    };
    let message = match result {
        Ok(MatchCommandResult::Accepted(result)) => ServerMessage::Acknowledgement {
            protocol_version: PROTOCOL_VERSION,
            result: Box::new(result),
        },
        Ok(MatchCommandResult::Stale(snapshot)) => stale_revision_message(snapshot),
        Ok(MatchCommandResult::Rejected { reason, snapshot }) => {
            let (code, text) = match reason {
                CommandRejection::WrongSeat => (ErrorCode::Unauthorized, "Wrong seat."),
                CommandRejection::InactivePhase => {
                    (ErrorCode::InvalidAction, "Match is not active.")
                }
                CommandRejection::ExpiredTime => (ErrorCode::InvalidAction, "Clock expired."),
                CommandRejection::IllegalAction(_) => {
                    (ErrorCode::InvalidAction, "Action is illegal.")
                }
            };
            ServerMessage::Error {
                protocol_version: PROTOCOL_VERSION,
                code,
                message: text.to_owned(),
                retryable: false,
                snapshot: Some(Box::new(snapshot)),
            }
        }
        Err(error) => ServerMessage::Error {
            protocol_version: PROTOCOL_VERSION,
            code: if error == ActorSubmitError::QueueFull {
                ErrorCode::RateLimited
            } else {
                ErrorCode::Internal
            },
            message: "The match command could not be processed.".to_owned(),
            retryable: error.retryable(),
            snapshot: None,
        },
    };
    let _ = send_server_message(socket, &message).await;
    committed
}

fn message_bytes(message: Message) -> Option<Vec<u8>> {
    match message {
        Message::Text(text) => Some(text.as_bytes().to_vec()),
        Message::Binary(bytes) => Some(bytes.to_vec()),
        _ => None,
    }
}

async fn send_snapshot(socket: &mut WebSocket, snapshot: MatchSnapshot) -> Result<(), ()> {
    send_server_message(
        socket,
        &ServerMessage::Snapshot {
            protocol_version: PROTOCOL_VERSION,
            snapshot: Box::new(snapshot),
        },
    )
    .await
}

async fn send_error(socket: &mut WebSocket, message: &str) -> Result<(), ()> {
    send_public_error(socket, ErrorCode::Unauthorized, message, false, None).await
}

async fn send_public_error(
    socket: &mut WebSocket,
    code: ErrorCode,
    message: &str,
    retryable: bool,
    snapshot: Option<Box<MatchSnapshot>>,
) -> Result<(), ()> {
    send_server_message(
        socket,
        &ServerMessage::Error {
            protocol_version: PROTOCOL_VERSION,
            code,
            message: message.to_owned(),
            retryable,
            snapshot,
        },
    )
    .await
}

async fn send_server_message(socket: &mut WebSocket, message: &ServerMessage) -> Result<(), ()> {
    let json = serde_json::to_string(message).map_err(|error| {
        warn!(%error, "failed to serialize outbound server message");
    })?;
    socket
        .send(Message::Text(json.into()))
        .await
        .map_err(|error| {
            warn!(%error, "failed to send outbound server message");
        })
}

async fn close_connection(connections: &Arc<AsyncMutex<ConnectionRegistry>>, peer_ip: IpAddr) {
    connections.lock().await.close(peer_ip);
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crownline_core::{Action, ClockSettings};

    use crate::{
        actors::{CommandTiming, MatchExecutor},
        database::Database,
        recovery::MatchRepository,
        rooms::{PersistedRoomRecord, PersistedSeatRecord, RoomPhase, ScenarioCatalog},
    };

    use super::*;

    fn fixture() -> (
        AuthoritativeMatch,
        Arc<Mutex<MatchRepository>>,
        Arc<SnapshotHub>,
        SystemTime,
    ) {
        let catalog = ScenarioCatalog::installed();
        let installed = catalog.get("crownlines-standard").unwrap();
        let started = UNIX_EPOCH.checked_add(Duration::from_secs(70_000)).unwrap();
        let authority = AuthoritativeMatch::new(
            Uuid::new_v4(),
            installed.definition.clone(),
            Some(ClockSettings {
                base_minutes: 5,
                increment_seconds: 0,
            }),
            started,
        )
        .unwrap();
        let initial = authority.persistence_image(started).unwrap();
        let match_id = authority.snapshot().match_id;
        let room = PersistedRoomRecord {
            code: "UVW456".to_owned(),
            match_id,
            scenario_id: "crownlines-standard".to_owned(),
            scenario_hash: installed.hash.clone(),
            clock: Some(ClockSettings {
                base_minutes: 5,
                increment_seconds: 0,
            }),
            phase: RoomPhase::Playing,
            seats: [
                PersistedSeatRecord {
                    player: Player::North,
                    display_name: "North".to_owned(),
                    token_hash: [1; 32],
                    ready: true,
                },
                PersistedSeatRecord {
                    player: Player::South,
                    display_name: "South".to_owned(),
                    token_hash: [2; 32],
                    ready: true,
                },
            ],
        };
        let mut repository = MatchRepository::new(Database::open_in_memory().unwrap());
        repository.register_match(&room, &initial).unwrap();
        let hub = Arc::new(SnapshotHub::default());
        hub.register(authority.snapshot());
        (authority, Arc::new(Mutex::new(repository)), hub, started)
    }

    #[test]
    fn slow_subscriber_observes_latest_snapshot_without_backpressuring_publishers() {
        let (authority, _repository, hub, _started) = fixture();
        let match_id = authority.snapshot().match_id;
        let mut subscription = hub.subscribe(match_id).unwrap();
        for revision in 1..=1_000 {
            let mut snapshot = authority.snapshot();
            snapshot.revision = revision;
            snapshot.state.revision = revision;
            snapshot.state_hash = snapshot.state.canonical_hash().unwrap();
            hub.publish_committed(snapshot);
        }
        assert_eq!(
            subscription
                .snapshots
                .borrow_and_update()
                .as_ref()
                .unwrap()
                .revision,
            1_000
        );
    }

    #[test]
    fn persistence_commit_precedes_publish_and_no_subscriber_does_not_roll_back() {
        let (authority, repository, hub, started) = fixture();
        let match_id = authority.snapshot().match_id;
        let mut executor =
            CommittingExecutor::new(authority, Arc::clone(&repository), Arc::clone(&hub));
        let received = started.checked_add(Duration::from_secs(5)).unwrap();
        let idempotency_key = Uuid::new_v4();
        let snapshot = executor
            .execute(
                idempotency_key,
                Player::South,
                &Action::Hold {
                    player: Player::South,
                },
                CommandTiming {
                    received_at: received,
                    decided_at: received,
                },
            )
            .unwrap();
        let duplicate = executor
            .execute(
                idempotency_key,
                Player::South,
                &Action::Hold {
                    player: Player::South,
                },
                CommandTiming {
                    received_at: received,
                    decided_at: received,
                },
            )
            .unwrap();
        assert_eq!(hub.latest(match_id).unwrap(), snapshot);
        assert_eq!(duplicate, snapshot);
        let persisted_revision: u64 = repository
            .lock()
            .unwrap()
            .database()
            .connection()
            .query_row(
                "SELECT revision FROM matches WHERE match_id = ?1",
                [match_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_revision, snapshot.revision);
    }

    #[test]
    fn failed_persistence_never_publishes_uncommitted_snapshot() {
        let (authority, _repository, hub, started) = fixture();
        let match_id = authority.snapshot().match_id;
        let missing_repository = Arc::new(Mutex::new(MatchRepository::new(
            Database::open_in_memory().unwrap(),
        )));
        let mut executor = CommittingExecutor::new(authority, missing_repository, Arc::clone(&hub));
        let received = started.checked_add(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            executor.execute(
                Uuid::new_v4(),
                Player::South,
                &Action::Hold {
                    player: Player::South
                },
                CommandTiming {
                    received_at: received,
                    decided_at: received
                },
            ),
            Err(ExecutionError::Fatal(_))
        ));
        assert_eq!(hub.latest(match_id).unwrap().revision, 0);
    }

    #[test]
    fn terminal_commit_persists_and_broadcasts_finished_lifecycle() {
        let (authority, repository, hub, started) = fixture();
        let match_id = authority.snapshot().match_id;
        let mut executor =
            CommittingExecutor::new(authority, Arc::clone(&repository), Arc::clone(&hub));
        let offered = started.checked_add(Duration::from_secs(1)).unwrap();
        executor
            .execute(
                Uuid::new_v4(),
                Player::South,
                &Action::OfferDraw {
                    player: Player::South,
                },
                CommandTiming {
                    received_at: offered,
                    decided_at: offered,
                },
            )
            .unwrap();
        let accepted = offered.checked_add(Duration::from_secs(1)).unwrap();
        let snapshot = executor
            .execute(
                Uuid::new_v4(),
                Player::North,
                &Action::RespondToDraw {
                    player: Player::North,
                    accept: true,
                },
                CommandTiming {
                    received_at: accepted,
                    decided_at: accepted,
                },
            )
            .unwrap();

        assert!(snapshot.state.outcome.is_some());
        assert_eq!(snapshot.room_state, ConnectionState::Finished);
        hub.set_seat_connected(match_id, Player::South, false);
        assert_eq!(
            hub.latest(match_id).unwrap().room_state,
            ConnectionState::Finished
        );
        let lifecycle: String = repository
            .lock()
            .unwrap()
            .database()
            .connection()
            .query_row(
                "SELECT lifecycle FROM rooms WHERE code = 'UVW456'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lifecycle, "finished");
    }

    #[test]
    fn reconnect_subscription_contains_clocks_draw_and_rematch_state() {
        let (authority, _repository, hub, _started) = fixture();
        let match_id = authority.snapshot().match_id;
        let mut snapshot = authority.snapshot();
        snapshot.state.outstanding_draw_offer = Some(Player::South);
        snapshot.state_hash = snapshot.state.canonical_hash().unwrap();
        hub.publish_committed(snapshot);
        hub.publish_room_state(
            match_id,
            ConnectionState::OpponentDisconnected,
            Some(RematchState::Requested),
        );
        let subscription = hub.subscribe(match_id).unwrap();
        let latest = subscription.snapshots.borrow().clone().unwrap();
        assert!(latest.state.clocks.is_some());
        assert_eq!(latest.state.outstanding_draw_offer, Some(Player::South));
        assert_eq!(latest.rematch_state, Some(RematchState::Requested));
        assert_eq!(latest.room_state, ConnectionState::OpponentDisconnected);
    }
}
