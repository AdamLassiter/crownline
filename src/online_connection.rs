use std::{
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use bevy::prelude::*;
use crownline_protocol::{
    ClientMessage, ConnectionState, ErrorCode, MatchSnapshot, MutationContext, PROTOCOL_VERSION,
    ReconnectToken, ServerMessage, validate_snapshot,
};
use directories::ProjectDirs;
use futures_util::{SinkExt as _, StreamExt as _};
use rand::Rng as _;
use reqwest::Url;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

use crate::{
    config::{ClientSettings, SavedOnlineSeat},
    lifecycle::{ClientFlow, ScenarioCatalog},
    online_lobby::{LobbyScreen, OnlineLobby, OnlineSeat},
    rendering::{
        DisplayedGame, LocalTransitionEventQueue, LocalTransitionNoticeLog, OverlaySelection,
    },
};

const COMMAND_CAPACITY: usize = 8;
const EVENT_CAPACITY: usize = 32;
const RETRY_BASE: Duration = Duration::from_millis(500);
const RETRY_CAP: Duration = Duration::from_secs(30);
const CREDENTIAL_SERVICE: &str = "org.Crownlines.Crownlines";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum ConnectionPhase {
    Connecting,
    Connected,
    Retrying {
        attempt: u32,
        delay: Duration,
    },
    #[default]
    Offline,
    Rejected,
    Terminal,
}

#[derive(Resource, Default)]
pub(crate) struct OnlineConnection {
    pub(crate) phase: ConnectionPhase,
    active_match: Option<Uuid>,
    observed_lobby_match: Option<Uuid>,
    restore_requested: bool,
    ready_sent: bool,
    status: String,
}

#[derive(Component)]
struct ConnectionStatusText;

#[derive(Debug)]
enum ConnectionCommand {
    Connect {
        server_url: String,
        seat: OnlineSeat,
        credential_id: Uuid,
        persist: bool,
    },
    Restore(SavedOnlineSeat),
    Ready,
    Retry,
    Cancel,
    Forget(SavedOnlineSeat),
}

#[derive(Debug)]
enum ConnectionEvent {
    Phase(ConnectionPhase),
    Snapshot(Box<MatchSnapshot>),
    Persisted(SavedOnlineSeat, CredentialProtection),
    Restored {
        saved: SavedOnlineSeat,
        token: ReconnectToken,
    },
    Forgotten,
    Notice(String),
}

#[derive(Debug, Clone, Copy)]
enum CredentialProtection {
    OperatingSystem,
    UserOnlyFile,
}

#[derive(Resource)]
struct ConnectionTransport {
    commands: mpsc::SyncSender<ConnectionCommand>,
    events: Arc<Mutex<mpsc::Receiver<ConnectionEvent>>>,
}

impl Default for ConnectionTransport {
    fn default() -> Self {
        let (command_sender, command_receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (event_sender, event_receiver) = mpsc::sync_channel(EVENT_CAPACITY);
        std::thread::Builder::new()
            .name("crownline-online-connection".to_owned())
            .spawn(move || connection_thread(command_receiver, event_sender))
            .expect("online connection worker must start");
        Self {
            commands: command_sender,
            events: Arc::new(Mutex::new(event_receiver)),
        }
    }
}

pub struct OnlineConnectionPlugin;

impl Plugin for OnlineConnectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnlineConnection>()
            .init_resource::<ConnectionTransport>()
            .add_systems(Startup, spawn_connection_status)
            .add_systems(
                Update,
                (
                    start_or_restore_connection,
                    handle_connection_controls,
                    poll_connection_events,
                    sync_connection_status,
                )
                    .chain(),
            );
    }
}

fn spawn_connection_status(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgb(0.86, 0.9, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(10),
            ..default()
        },
        GlobalZIndex(90),
        Visibility::Hidden,
        ConnectionStatusText,
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn start_or_restore_connection(
    settings: Res<ClientSettings>,
    lobby: Res<OnlineLobby>,
    transport: Res<ConnectionTransport>,
    mut connection: ResMut<OnlineConnection>,
) {
    if !connection.restore_requested {
        connection.restore_requested = true;
        if lobby.seat.is_none()
            && let Some(saved) = settings.saved_online_seat.clone()
        {
            let _ = transport
                .commands
                .try_send(ConnectionCommand::Restore(saved));
        }
    }
    let Some(seat) = lobby.seat.as_ref() else {
        return;
    };
    if connection.observed_lobby_match == Some(seat.match_id) {
        return;
    }
    connection.observed_lobby_match = Some(seat.match_id);
    connection.ready_sent = false;
    let credential_id = settings
        .saved_online_seat
        .as_ref()
        .filter(|saved| saved.match_id == seat.match_id)
        .map_or_else(Uuid::new_v4, |saved| saved.credential_id);
    let _ = transport.commands.try_send(ConnectionCommand::Connect {
        server_url: lobby.server_url.clone(),
        seat: seat.clone(),
        credential_id,
        persist: true,
    });
}

#[allow(clippy::needless_pass_by_value)]
fn handle_connection_controls(
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<ClientFlow>,
    mut lobby: ResMut<OnlineLobby>,
    mut settings: ResMut<ClientSettings>,
    transport: Res<ConnectionTransport>,
    mut connection: ResMut<OnlineConnection>,
) {
    if lobby.ready_requested
        && !connection.ready_sent
        && transport
            .commands
            .try_send(ConnectionCommand::Ready)
            .is_ok()
    {
        connection.ready_sent = true;
    }
    if !matches!(*flow, ClientFlow::OnlineLobby | ClientFlow::OnlinePlaying) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        let command = if matches!(
            connection.phase,
            ConnectionPhase::Offline | ConnectionPhase::Rejected
        ) {
            settings
                .saved_online_seat
                .clone()
                .map_or(ConnectionCommand::Retry, ConnectionCommand::Restore)
        } else {
            ConnectionCommand::Retry
        };
        let _ = transport.commands.try_send(command);
    }
    if keys.just_pressed(KeyCode::KeyX)
        && matches!(
            connection.phase,
            ConnectionPhase::Connecting | ConnectionPhase::Retrying { .. }
        )
    {
        let _ = transport.commands.try_send(ConnectionCommand::Cancel);
    }
    if keys.just_pressed(KeyCode::KeyF)
        && let Some(saved) = settings.saved_online_seat.take()
    {
        let _ = transport
            .commands
            .try_send(ConnectionCommand::Forget(saved));
        if let Err(error) = settings.save() {
            tracing::warn!(%error, "could not clear saved online seat settings");
        }
        lobby.seat = None;
        lobby.ready_requested = false;
        lobby.screen = LobbyScreen::Menu;
        connection.observed_lobby_match = None;
    }
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn poll_connection_events(
    transport: Res<ConnectionTransport>,
    mut connection: ResMut<OnlineConnection>,
    mut settings: ResMut<ClientSettings>,
    mut lobby: ResMut<OnlineLobby>,
    catalog: Res<ScenarioCatalog>,
    mut flow: ResMut<ClientFlow>,
    mut game: ResMut<DisplayedGame>,
    mut selection: ResMut<OverlaySelection>,
    mut transitions: ResMut<LocalTransitionEventQueue>,
    mut notices: ResMut<LocalTransitionNoticeLog>,
) {
    let Ok(events) = transport.events.lock() else {
        return;
    };
    while let Ok(event) = events.try_recv() {
        match event {
            ConnectionEvent::Phase(phase) => connection.phase = phase,
            ConnectionEvent::Notice(message) => connection.status = message,
            ConnectionEvent::Forgotten => {
                connection.phase = ConnectionPhase::Offline;
                "Saved seat credential deleted.".clone_into(&mut connection.status);
            }
            ConnectionEvent::Persisted(saved, protection) => {
                connection.active_match = Some(saved.match_id);
                settings.saved_online_seat = Some(saved);
                if let Err(error) = settings.save() {
                    tracing::warn!(%error, "could not save online seat locator");
                    "Connected, but the seat could not be saved."
                        .clone_into(&mut connection.status);
                } else {
                    connection.status = match protection {
                        CredentialProtection::OperatingSystem => {
                            "Seat credential protected by the operating system.".to_owned()
                        }
                        CredentialProtection::UserOnlyFile => {
                            "Seat credential stored in a user-only (0600) fallback file.".to_owned()
                        }
                    };
                }
            }
            ConnectionEvent::Restored { saved, token } => {
                lobby.server_url.clone_from(&saved.server_url);
                lobby.seat = Some(OnlineSeat {
                    match_id: saved.match_id,
                    room_code: saved.room_code,
                    seat: saved.seat,
                    reconnect_token: token,
                });
                lobby.screen = LobbyScreen::Waiting;
                *flow = ClientFlow::OnlineLobby;
                connection.observed_lobby_match = Some(saved.match_id);
                connection.active_match = Some(saved.match_id);
            }
            ConnectionEvent::Snapshot(snapshot) => match adopt_snapshot(
                &snapshot,
                connection.active_match,
                &catalog,
                &mut game,
                &mut selection,
                &mut transitions,
                &mut notices,
            ) {
                Ok(()) => {
                    connection.active_match = Some(snapshot.match_id);
                    connection.phase = if snapshot.room_state == ConnectionState::Finished
                        || snapshot.state.outcome.is_some()
                    {
                        ConnectionPhase::Terminal
                    } else {
                        ConnectionPhase::Connected
                    };
                    *flow = ClientFlow::OnlinePlaying;
                }
                Err(message) => {
                    connection.phase = ConnectionPhase::Rejected;
                    connection.status = message;
                    let _ = transport.commands.try_send(ConnectionCommand::Cancel);
                }
            },
        }
    }
}

fn adopt_snapshot(
    snapshot: &MatchSnapshot,
    active_match: Option<Uuid>,
    catalog: &ScenarioCatalog,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    transitions: &mut LocalTransitionEventQueue,
    notices: &mut LocalTransitionNoticeLog,
) -> Result<(), String> {
    validate_snapshot(snapshot)
        .map_err(|_| "The server sent an invalid match snapshot.".to_owned())?;
    if active_match.is_some_and(|match_id| match_id != snapshot.match_id) {
        return Err("The server snapshot belongs to a different match.".to_owned());
    }
    let scenario = catalog
        .0
        .iter()
        .find(|scenario| scenario.id == snapshot.scenario_id)
        .ok_or_else(|| "The match scenario is not installed.".to_owned())?;
    let scenario_hash = scenario
        .canonical_hash()
        .map_err(|_| "The installed match scenario is invalid.".to_owned())?;
    if scenario_hash != snapshot.scenario_hash {
        return Err("The installed scenario differs from the server scenario.".to_owned());
    }
    game.scenario = scenario.clone();
    game.state = snapshot.state.clone();
    *selection = OverlaySelection::default();
    transitions.clear();
    notices.entries.clear();
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn sync_connection_status(
    connection: Res<OnlineConnection>,
    settings: Res<ClientSettings>,
    mut text: Query<(&mut Text, &mut Visibility), With<ConnectionStatusText>>,
) {
    let phase = match &connection.phase {
        ConnectionPhase::Connecting => "CONNECTING".to_owned(),
        ConnectionPhase::Connected => "CONNECTED".to_owned(),
        ConnectionPhase::Retrying { attempt, delay } => {
            format!("RETRYING #{attempt} IN {:.1}s", delay.as_secs_f32())
        }
        ConnectionPhase::Offline => "OFFLINE".to_owned(),
        ConnectionPhase::Rejected => "REJECTED".to_owned(),
        ConnectionPhase::Terminal => "TERMINAL".to_owned(),
    };
    for (mut text, mut visibility) in &mut text {
        *visibility = if settings.saved_online_seat.is_some()
            || !matches!(connection.phase, ConnectionPhase::Offline)
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        text.0 = format!(
            "ONLINE {phase} · T retry · X cancel retry · F forget seat\n{}",
            connection.status
        );
    }
}

fn connection_thread(
    commands: mpsc::Receiver<ConnectionCommand>,
    events: mpsc::SyncSender<ConnectionEvent>,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        let _ = events.send(ConnectionEvent::Notice(
            "The online runtime could not start.".to_owned(),
        ));
        return;
    };
    runtime.block_on(connection_worker(commands, events));
}

#[derive(Clone)]
struct ConnectionTarget {
    server_url: String,
    seat: OnlineSeat,
}

async fn connection_worker(
    commands: mpsc::Receiver<ConnectionCommand>,
    events: mpsc::SyncSender<ConnectionEvent>,
) {
    let vault = CredentialVault::system();
    let mut target: Option<ConnectionTarget> = None;
    let mut retry_attempt: u32 = 0;
    loop {
        if target.is_none() {
            let Ok(command) = commands.recv() else { return };
            if !apply_idle_command(command, &vault, &events, &mut target) {
                continue;
            }
        }
        let Some(current) = target.clone() else {
            continue;
        };
        let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Connecting));
        match connect_once(&current, &commands, &events, &vault).await {
            SocketOutcome::Cancelled => {
                target = None;
                retry_attempt = 0;
                let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Offline));
            }
            SocketOutcome::Rejected => {
                target = None;
                retry_attempt = 0;
                let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Rejected));
            }
            SocketOutcome::RetryNow => retry_attempt = 0,
            SocketOutcome::Switch(command) => {
                target = None;
                retry_attempt = 0;
                let _ = apply_idle_command(command, &vault, &events, &mut target);
            }
            SocketOutcome::Transient => {
                retry_attempt = retry_attempt.saturating_add(1);
                let delay = retry_delay(retry_attempt, rand::rng().random_range(-0.2..=0.2));
                let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Retrying {
                    attempt: retry_attempt,
                    delay,
                }));
                if wait_for_retry(delay, &commands, &events, &vault, &mut target).await {
                    retry_attempt = 0;
                }
            }
        }
    }
}

fn apply_idle_command(
    command: ConnectionCommand,
    vault: &CredentialVault,
    events: &mpsc::SyncSender<ConnectionEvent>,
    target: &mut Option<ConnectionTarget>,
) -> bool {
    match command {
        ConnectionCommand::Connect {
            server_url,
            seat,
            credential_id,
            persist,
        } => {
            if persist {
                let saved = saved_seat(&server_url, &seat, credential_id);
                match vault.store(credential_id, seat.reconnect_token.expose()) {
                    Ok(protection) => {
                        let _ = events.send(ConnectionEvent::Persisted(saved, protection));
                    }
                    Err(()) => {
                        let _ = events.send(ConnectionEvent::Notice(
                            "The seat credential could not be stored securely.".to_owned(),
                        ));
                    }
                }
            }
            *target = Some(ConnectionTarget { server_url, seat });
            true
        }
        ConnectionCommand::Restore(saved) => {
            if let Ok(token) = vault.load(saved.credential_id) {
                let seat = OnlineSeat {
                    match_id: saved.match_id,
                    room_code: saved.room_code.clone(),
                    seat: saved.seat,
                    reconnect_token: ReconnectToken::issued(token),
                };
                let _ = events.send(ConnectionEvent::Restored {
                    saved: saved.clone(),
                    token: seat.reconnect_token.clone(),
                });
                *target = Some(ConnectionTarget {
                    server_url: saved.server_url,
                    seat,
                });
                true
            } else {
                let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Rejected));
                let _ = events.send(ConnectionEvent::Notice(
                    "The saved seat credential is unavailable. Forget it or join again.".to_owned(),
                ));
                false
            }
        }
        ConnectionCommand::Forget(saved) => {
            let _ = vault.delete(saved.credential_id);
            *target = None;
            let _ = events.send(ConnectionEvent::Forgotten);
            false
        }
        ConnectionCommand::Ready | ConnectionCommand::Retry | ConnectionCommand::Cancel => false,
    }
}

#[derive(Debug)]
enum SocketOutcome {
    Cancelled,
    Rejected,
    RetryNow,
    Transient,
    Switch(ConnectionCommand),
}

async fn connect_once(
    target: &ConnectionTarget,
    commands: &mpsc::Receiver<ConnectionCommand>,
    events: &mpsc::SyncSender<ConnectionEvent>,
    vault: &CredentialVault,
) -> SocketOutcome {
    let Ok(url) = websocket_endpoint(&target.server_url) else {
        let _ = events.send(ConnectionEvent::Notice(
            "The saved server address is invalid or insecure.".to_owned(),
        ));
        return SocketOutcome::Rejected;
    };
    let Ok((mut socket, _)) = connect_async(url.as_str()).await else {
        return SocketOutcome::Transient;
    };
    let auth = ClientMessage::Authenticate {
        protocol_version: PROTOCOL_VERSION,
        room_code: target.seat.room_code.clone(),
        reconnect_token: target.seat.reconnect_token.clone(),
    };
    if send_client_message(&mut socket, &auth).await.is_err() {
        return SocketOutcome::Transient;
    }
    loop {
        tokio::select! {
            message = socket.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if !handle_server_message(text.as_bytes(), events) { return SocketOutcome::Rejected; }
                }
                Some(Ok(Message::Binary(bytes))) => {
                    if !handle_server_message(&bytes, events) { return SocketOutcome::Rejected; }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => return SocketOutcome::Transient,
                Some(Ok(_)) => {}
            },
            () = tokio::time::sleep(Duration::from_millis(50)) => {
                while let Ok(command) = commands.try_recv() {
                    match command {
                        ConnectionCommand::Ready => {
                            let ready = ClientMessage::Ready {
                                protocol_version: PROTOCOL_VERSION,
                                context: MutationContext { match_id: target.seat.match_id, expected_revision: 0, idempotency_key: Uuid::new_v4() },
                            };
                            if send_client_message(&mut socket, &ready).await.is_err() { return SocketOutcome::Transient; }
                        }
                        ConnectionCommand::Retry => return SocketOutcome::RetryNow,
                        ConnectionCommand::Cancel => return SocketOutcome::Cancelled,
                        ConnectionCommand::Forget(saved) => {
                            let _ = vault.delete(saved.credential_id);
                            let _ = events.send(ConnectionEvent::Forgotten);
                            return SocketOutcome::Cancelled;
                        }
                        command @ (ConnectionCommand::Connect { .. } | ConnectionCommand::Restore(_)) => return SocketOutcome::Switch(command),
                    }
                }
            }
        }
    }
}

fn handle_server_message(bytes: &[u8], events: &mpsc::SyncSender<ConnectionEvent>) -> bool {
    let Ok(message) = serde_json::from_slice::<ServerMessage>(bytes) else {
        let _ = events.send(ConnectionEvent::Notice(
            "The server sent an incompatible message.".to_owned(),
        ));
        return false;
    };
    let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Connected));
    match message {
        ServerMessage::Snapshot { snapshot, .. }
        | ServerMessage::Error {
            snapshot: Some(snapshot),
            ..
        } => {
            let _ = events.send(ConnectionEvent::Snapshot(snapshot));
        }
        ServerMessage::Acknowledgement { result, .. } => {
            let _ = events.send(ConnectionEvent::Snapshot(Box::new(result.snapshot)));
        }
        ServerMessage::Error {
            code: ErrorCode::Unauthorized,
            ..
        } => {
            let _ = events.send(ConnectionEvent::Notice(
                "The saved seat credential was rejected. Forget it or join again.".to_owned(),
            ));
            return false;
        }
        ServerMessage::Error {
            retryable: false, ..
        } => {
            let _ = events.send(ConnectionEvent::Notice(
                "The server rejected the online request.".to_owned(),
            ));
        }
        ServerMessage::ConnectionState {
            state: ConnectionState::Finished,
            ..
        } => {
            let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Terminal));
        }
        ServerMessage::ConnectionState { .. }
        | ServerMessage::RematchState { .. }
        | ServerMessage::Error { .. } => {}
    }
    true
}

async fn send_client_message<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    message: &ClientMessage,
) -> Result<(), ()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let json = serde_json::to_string(message).map_err(|_| ())?;
    socket
        .send(Message::Text(json.into()))
        .await
        .map_err(|_| ())
}

async fn wait_for_retry(
    delay: Duration,
    commands: &mpsc::Receiver<ConnectionCommand>,
    events: &mpsc::SyncSender<ConnectionEvent>,
    vault: &CredentialVault,
    target: &mut Option<ConnectionTarget>,
) -> bool {
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        if let Ok(command) = commands.try_recv() {
            match command {
                ConnectionCommand::Retry => return true,
                ConnectionCommand::Cancel => {
                    *target = None;
                    let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Offline));
                    return false;
                }
                other => {
                    return apply_idle_command(other, vault, events, target);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn retry_delay(attempt: u32, jitter: f64) -> Duration {
    let exponent = attempt.saturating_sub(1).min(16);
    let base = RETRY_BASE
        .mul_f64(f64::from(2_u32.pow(exponent)))
        .min(RETRY_CAP);
    base.mul_f64((1.0 + jitter.clamp(-0.2, 0.2)).clamp(0.8, 1.2))
        .min(RETRY_CAP)
}

fn websocket_endpoint(server_url: &str) -> Result<Url, ()> {
    let mut url = Url::parse(server_url.trim()).map_err(|_| ())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(());
    }
    match url.scheme() {
        "wss" => {}
        "ws" if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")) => {}
        _ => return Err(()),
    }
    url.set_path("/ws");
    Ok(url)
}

fn saved_seat(server_url: &str, seat: &OnlineSeat, credential_id: Uuid) -> SavedOnlineSeat {
    SavedOnlineSeat {
        server_url: server_url.to_owned(),
        room_code: seat.room_code.clone(),
        match_id: seat.match_id,
        seat: seat.seat,
        credential_id,
    }
}

struct CredentialVault {
    fallback_dir: PathBuf,
    use_keyring: bool,
}

impl CredentialVault {
    fn system() -> Self {
        let fallback_dir = ProjectDirs::from("org", "Crownlines", "Crownlines").map_or_else(
            || std::env::temp_dir().join("crownline-credentials"),
            |dirs| dirs.data_local_dir().join("credentials"),
        );
        Self {
            fallback_dir,
            use_keyring: true,
        }
    }

    fn store(&self, id: Uuid, token: &str) -> Result<CredentialProtection, ()> {
        if self.use_keyring
            && let Ok(entry) = keyring::Entry::new(CREDENTIAL_SERVICE, &id.to_string())
            && entry.set_password(token).is_ok()
        {
            let _ = fs::remove_file(self.path(id));
            return Ok(CredentialProtection::OperatingSystem);
        }
        write_secret(&self.path(id), token.as_bytes())?;
        Ok(CredentialProtection::UserOnlyFile)
    }

    fn load(&self, id: Uuid) -> Result<String, ()> {
        if self.use_keyring
            && let Ok(entry) = keyring::Entry::new(CREDENTIAL_SERVICE, &id.to_string())
            && let Ok(token) = entry.get_password()
        {
            return Ok(token);
        }
        let mut token = String::new();
        OpenOptions::new()
            .read(true)
            .open(self.path(id))
            .map_err(|_| ())?
            .read_to_string(&mut token)
            .map_err(|_| ())?;
        (!token.is_empty()).then_some(token).ok_or(())
    }

    fn delete(&self, id: Uuid) -> Result<(), ()> {
        if self.use_keyring
            && let Ok(entry) = keyring::Entry::new(CREDENTIAL_SERVICE, &id.to_string())
        {
            let _ = entry.delete_credential();
        }
        match fs::remove_file(self.path(id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(()),
        }
    }

    fn path(&self, id: Uuid) -> PathBuf {
        self.fallback_dir.join(format!("{id}.secret"))
    }
}

fn write_secret(path: &Path, bytes: &[u8]) -> Result<(), ()> {
    let parent = path.parent().ok_or(())?;
    fs::create_dir_all(parent).map_err(|_| ())?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| ())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| ())?;
    }
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crownline_core::scenario::Player;

    #[test]
    fn retry_backoff_is_jittered_and_capped() {
        assert_eq!(retry_delay(1, 0.0), Duration::from_millis(500));
        assert_eq!(retry_delay(2, -0.2), Duration::from_millis(800));
        assert_eq!(retry_delay(2, 0.2), Duration::from_millis(1200));
        assert_eq!(retry_delay(99, 0.2), RETRY_CAP);
    }

    #[test]
    fn websocket_endpoint_requires_tls_except_on_loopback() {
        assert_eq!(
            websocket_endpoint("ws://127.0.0.1:5000").unwrap().as_str(),
            "ws://127.0.0.1:5000/ws"
        );
        assert!(websocket_endpoint("ws://example.com").is_err());
        assert!(websocket_endpoint("wss://user@example.com?token=no").is_err());
    }

    #[test]
    fn fallback_credentials_round_trip_and_delete() {
        let root = std::env::temp_dir().join(format!("crownline-vault-test-{}", Uuid::new_v4()));
        let vault = CredentialVault {
            fallback_dir: root.clone(),
            use_keyring: false,
        };
        let id = Uuid::new_v4();
        assert!(matches!(
            vault.store(id, "secret"),
            Ok(CredentialProtection::UserOnlyFile)
        ));
        assert_eq!(vault.load(id).unwrap(), "secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(vault.path(id)).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        vault.delete(id).unwrap();
        assert!(vault.load(id).is_err());
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn saved_metadata_does_not_contain_token() {
        let seat = OnlineSeat {
            match_id: Uuid::new_v4(),
            room_code: "ABC234".to_owned(),
            seat: Player::North,
            reconnect_token: ReconnectToken::issued("never-serialize-me".to_owned()),
        };
        let saved = saved_seat("wss://play.example.com", &seat, Uuid::new_v4());
        let encoded = ron::to_string(&saved).unwrap();
        assert!(!encoded.contains(seat.reconnect_token.expose()));
    }
}
