use std::{
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use bevy::{ecs::system::SystemParam, prelude::*};
use crownline_core::Action;
use crownline_protocol::{
    ActionRequest, ClientMessage, ConnectionState, ErrorCode, MatchSnapshot, MutationContext,
    MutationResult, PROTOCOL_VERSION, ReconnectToken, RematchState, ServerMessage,
    validate_snapshot,
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
    local_interaction::BoardInteraction,
    online_lobby::{LobbyScreen, OnlineLobby, OnlineSeat},
    online_status::{AuthoritativePresentationSnapshot, OnlineRoomStateChanged},
    rendering::{
        DisplayedGame, HoveredBoardSquare, LocalTransitionEventQueue, LocalTransitionNoticeLog,
        OverlaySelection,
    },
};

const COMMAND_CAPACITY: usize = 8;
const EVENT_CAPACITY: usize = 32;
const RETRY_BASE: Duration = Duration::from_millis(500);
const RETRY_CAP: Duration = Duration::from_secs(30);
const ACTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);
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
    pending_action: Option<PendingAction>,
    pending_control: Option<PendingControl>,
    last_snapshot: Option<SnapshotIdentity>,
    force_resync_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotIdentity {
    match_id: Uuid,
    scenario_id: String,
    revision: u64,
    state_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotDisposition {
    Replace,
    Equal,
    Older,
    Diverged,
}

#[derive(Debug, Clone)]
struct PendingAction {
    request: ActionRequest,
    last_sent: Instant,
    attempts: u32,
}

#[derive(Debug, Clone)]
struct PendingControl {
    message: ClientMessage,
    idempotency_key: Uuid,
    kind: OnlineControlKind,
    last_sent: Instant,
    attempts: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct OnlineActionIntent {
    pub(crate) action: Action,
    pub(crate) expected_revision: u64,
}

#[derive(Resource, Default)]
pub(crate) struct OnlineIntentOutbox {
    pub(crate) intent: Option<OnlineActionIntent>,
    pub(crate) locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnlineControlKind {
    Draw,
    Resign,
    Rematch,
    Leave,
}

#[derive(Resource, Default)]
pub(crate) struct OnlineControlOutbox {
    pub(crate) message: Option<(ClientMessage, OnlineControlKind)>,
    pub(crate) locked: bool,
}

#[derive(Debug, Clone, Copy, Message)]
pub(crate) struct OnlineControlResolved {
    pub(crate) kind: OnlineControlKind,
    pub(crate) accepted: bool,
}

#[derive(Debug, Clone, Copy, Message)]
pub(crate) struct OnlineRematchStateChanged(pub(crate) Option<RematchState>);

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
    Action(ActionRequest),
    Control(ClientMessage),
    LeaveAndForget {
        message: ClientMessage,
        saved: Option<SavedOnlineSeat>,
    },
    Retry,
    Cancel,
    Forget(SavedOnlineSeat),
}

#[derive(Debug)]
enum ConnectionEvent {
    Phase(ConnectionPhase),
    Snapshot(Box<MatchSnapshot>),
    Acknowledgement(Box<MutationResult>),
    CommandRejected {
        failure: CommandFailure,
        retryable: bool,
    },
    RoomState(ConnectionState),
    RematchState(RematchState),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandFailure {
    WrongTurn,
    ClockExpired,
    Stale,
    Rejected,
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
            .init_resource::<OnlineIntentOutbox>()
            .init_resource::<OnlineControlOutbox>()
            .init_resource::<ConnectionTransport>()
            .add_message::<OnlineControlResolved>()
            .add_message::<OnlineRematchStateChanged>()
            .add_systems(Startup, spawn_connection_status)
            .add_systems(
                Update,
                (
                    start_or_restore_connection,
                    handle_connection_controls,
                    queue_online_action,
                    queue_online_control,
                    request_forced_resync,
                    retry_timed_out_commands,
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
    connection.last_snapshot = None;
    connection.force_resync_requested = false;
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

#[allow(clippy::needless_pass_by_value)]
fn queue_online_action(
    mut outbox: ResMut<OnlineIntentOutbox>,
    mut connection: ResMut<OnlineConnection>,
    transport: Res<ConnectionTransport>,
    mut interaction: ResMut<BoardInteraction>,
) {
    let Some(intent) = outbox.intent.take() else {
        return;
    };
    if connection.pending_action.is_some() {
        outbox.locked = true;
        return;
    }
    let Some(match_id) = connection.active_match else {
        outbox.locked = false;
        interaction.resolve_online("Command not sent: no authenticated match is active.");
        return;
    };
    let request = ActionRequest {
        context: MutationContext {
            match_id,
            expected_revision: intent.expected_revision,
            idempotency_key: Uuid::new_v4(),
        },
        action: intent.action,
    };
    let sent = transport
        .commands
        .try_send(ConnectionCommand::Action(request.clone()))
        .is_ok();
    if !sent {
        "Command queued; the bounded network channel is busy.".clone_into(&mut connection.status);
    }
    connection.pending_action = Some(PendingAction {
        request,
        last_sent: if sent {
            Instant::now()
        } else {
            Instant::now()
                .checked_sub(ACTION_RESPONSE_TIMEOUT)
                .unwrap_or_else(Instant::now)
        },
        attempts: u32::from(sent),
    });
    outbox.locked = true;
    if sent {
        "Command pending authoritative acknowledgement.".clone_into(&mut connection.status);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn queue_online_control(
    mut outbox: ResMut<OnlineControlOutbox>,
    mut connection: ResMut<OnlineConnection>,
    settings: Res<ClientSettings>,
    transport: Res<ConnectionTransport>,
) {
    let Some((message, kind)) = outbox.message.take() else {
        return;
    };
    if connection.pending_action.is_some() || connection.pending_control.is_some() {
        outbox.message = Some((message, kind));
        outbox.locked = true;
        return;
    }
    let Some(context) = message_context(&message) else {
        outbox.locked = false;
        "The lifecycle command was malformed.".clone_into(&mut connection.status);
        return;
    };
    let command = if kind == OnlineControlKind::Leave {
        ConnectionCommand::LeaveAndForget {
            message: message.clone(),
            saved: settings.saved_online_seat.clone(),
        }
    } else {
        ConnectionCommand::Control(message.clone())
    };
    let sent = transport.commands.try_send(command).is_ok();
    connection.pending_control = Some(PendingControl {
        message,
        idempotency_key: context.idempotency_key,
        kind,
        last_sent: if sent {
            Instant::now()
        } else {
            Instant::now()
                .checked_sub(ACTION_RESPONSE_TIMEOUT)
                .unwrap_or_else(Instant::now)
        },
        attempts: u32::from(sent),
    });
    outbox.locked = true;
    if sent {
        "Match control pending authoritative response.".clone_into(&mut connection.status);
    } else {
        "Match control queued; the bounded network channel is busy."
            .clone_into(&mut connection.status);
    }
}

fn message_context(message: &ClientMessage) -> Option<MutationContext> {
    match message {
        ClientMessage::Ready { context, .. }
        | ClientMessage::Draw { context, .. }
        | ClientMessage::Rematch { context, .. }
        | ClientMessage::Leave { context, .. } => Some(*context),
        ClientMessage::Action { request, .. } => Some(request.context),
        ClientMessage::Authenticate { .. } => None,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn retry_timed_out_commands(
    mut connection: ResMut<OnlineConnection>,
    transport: Res<ConnectionTransport>,
) {
    if !matches!(
        connection.phase,
        ConnectionPhase::Connected | ConnectionPhase::Terminal
    ) {
        return;
    }
    if let Some(pending) = connection.pending_action.as_mut()
        && pending.last_sent.elapsed() >= ACTION_RESPONSE_TIMEOUT
        && transport
            .commands
            .try_send(ConnectionCommand::Action(pending.request.clone()))
            .is_ok()
    {
        pending.last_sent = Instant::now();
        pending.attempts = pending.attempts.saturating_add(1);
        "Command response timed out; retrying the same idempotent intent."
            .clone_into(&mut connection.status);
    }
    if let Some(pending) = connection.pending_control.as_mut()
        && pending.last_sent.elapsed() >= ACTION_RESPONSE_TIMEOUT
        && pending.kind != OnlineControlKind::Leave
        && transport
            .commands
            .try_send(ConnectionCommand::Control(pending.message.clone()))
            .is_ok()
    {
        pending.last_sent = Instant::now();
        pending.attempts = pending.attempts.saturating_add(1);
        "Match control timed out; retrying the same idempotent request."
            .clone_into(&mut connection.status);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn request_forced_resync(
    mut connection: ResMut<OnlineConnection>,
    transport: Res<ConnectionTransport>,
) {
    if connection.force_resync_requested
        && transport
            .commands
            .try_send(ConnectionCommand::Retry)
            .is_ok()
    {
        connection.force_resync_requested = false;
    }
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
#[derive(SystemParam)]
struct ConnectionEventParams<'w> {
    settings: ResMut<'w, ClientSettings>,
    lobby: ResMut<'w, OnlineLobby>,
    catalog: Res<'w, ScenarioCatalog>,
    flow: ResMut<'w, ClientFlow>,
    game: ResMut<'w, DisplayedGame>,
    selection: ResMut<'w, OverlaySelection>,
    hovered: ResMut<'w, HoveredBoardSquare>,
    transitions: ResMut<'w, LocalTransitionEventQueue>,
    notices: ResMut<'w, LocalTransitionNoticeLog>,
    interaction: ResMut<'w, BoardInteraction>,
    outbox: ResMut<'w, OnlineIntentOutbox>,
    control_outbox: ResMut<'w, OnlineControlOutbox>,
    presentation_snapshots: MessageWriter<'w, AuthoritativePresentationSnapshot>,
    room_states: MessageWriter<'w, OnlineRoomStateChanged>,
    control_resolutions: MessageWriter<'w, OnlineControlResolved>,
    rematch_states: MessageWriter<'w, OnlineRematchStateChanged>,
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn poll_connection_events(
    transport: Res<ConnectionTransport>,
    mut connection: ResMut<OnlineConnection>,
    params: ConnectionEventParams,
) {
    let ConnectionEventParams {
        mut settings,
        mut lobby,
        catalog,
        mut flow,
        mut game,
        mut selection,
        mut hovered,
        mut transitions,
        mut notices,
        mut interaction,
        mut outbox,
        mut control_outbox,
        mut presentation_snapshots,
        mut room_states,
        mut control_resolutions,
        mut rematch_states,
    } = params;
    let Ok(events) = transport.events.lock() else {
        return;
    };
    while let Ok(event) = events.try_recv() {
        match event {
            ConnectionEvent::Phase(phase) => {
                if phase == ConnectionPhase::Rejected {
                    connection.pending_action = None;
                    connection.pending_control = None;
                    outbox.locked = false;
                    outbox.intent = None;
                    control_outbox.locked = false;
                    control_outbox.message = None;
                    interaction.resolve_online("Online authentication was rejected.");
                }
                connection.phase = phase;
            }
            ConnectionEvent::Notice(message) => connection.status = message,
            ConnectionEvent::Forgotten => {
                connection.phase = ConnectionPhase::Offline;
                connection.pending_action = None;
                connection.pending_control = None;
                outbox.locked = false;
                outbox.intent = None;
                control_outbox.locked = false;
                control_outbox.message = None;
                interaction.resolve_online("Saved seat credential deleted.");
                "Saved seat credential deleted.".clone_into(&mut connection.status);
                settings.saved_online_seat = None;
                if let Err(error) = settings.save() {
                    tracing::warn!(%error, "could not clear forgotten online seat settings");
                }
                lobby.seat = None;
                lobby.ready_requested = false;
                lobby.screen = LobbyScreen::Closed;
                *flow = ClientFlow::Setup;
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
                connection.last_snapshot = None;
                connection.force_resync_requested = false;
            }
            ConnectionEvent::Acknowledgement(result) => {
                let matches_pending = connection.pending_action.as_ref().is_some_and(|pending| {
                    pending.request.context.match_id == result.match_id
                        && pending.request.context.idempotency_key == result.idempotency_key
                });
                if matches_pending {
                    connection.pending_action = None;
                    outbox.locked = false;
                    interaction.resolve_online("Command accepted by the server.");
                } else if let Some(pending) = connection.pending_control.as_ref()
                    && pending.idempotency_key == result.idempotency_key
                    && result.match_id == connection.active_match.unwrap_or(result.match_id)
                {
                    let kind = pending.kind;
                    connection.pending_control = None;
                    control_outbox.locked = false;
                    control_resolutions.write(OnlineControlResolved {
                        kind,
                        accepted: true,
                    });
                } else {
                    "Received an acknowledgement for a non-pending command."
                        .clone_into(&mut connection.status);
                }
            }
            ConnectionEvent::CommandRejected { failure, retryable } => {
                if matches!(
                    failure,
                    CommandFailure::WrongTurn | CommandFailure::ClockExpired
                ) {
                    *selection = OverlaySelection::default();
                }
                if retryable && failure != CommandFailure::Stale {
                    if let Some(pending) = connection.pending_action.as_mut() {
                        pending.last_sent = Instant::now();
                    }
                    if let Some(pending) = connection.pending_control.as_mut() {
                        pending.last_sent = Instant::now();
                    }
                    "The server asked the client to retry the same command."
                        .clone_into(&mut connection.status);
                } else {
                    connection.pending_action = None;
                    outbox.locked = false;
                    if let Some(pending) = connection.pending_control.take() {
                        control_resolutions.write(OnlineControlResolved {
                            kind: pending.kind,
                            accepted: false,
                        });
                        control_outbox.locked = false;
                    }
                    let message = match failure {
                        CommandFailure::WrongTurn => {
                            "Command rejected: it is not this seat's turn."
                        }
                        CommandFailure::ClockExpired => {
                            "Command rejected: the authoritative clock expired."
                        }
                        CommandFailure::Stale => {
                            "Command cancelled because the match revision changed."
                        }
                        CommandFailure::Rejected => "Command rejected by the server.",
                    };
                    interaction.resolve_online(message);
                }
            }
            ConnectionEvent::RoomState(state) => {
                room_states.write(OnlineRoomStateChanged(state));
            }
            ConnectionEvent::RematchState(state) => {
                if connection
                    .pending_control
                    .as_ref()
                    .is_some_and(|pending| pending.kind == OnlineControlKind::Rematch)
                {
                    connection.pending_control = None;
                    control_outbox.locked = false;
                    control_resolutions.write(OnlineControlResolved {
                        kind: OnlineControlKind::Rematch,
                        accepted: true,
                    });
                }
                rematch_states.write(OnlineRematchStateChanged(Some(state)));
                if state == RematchState::Accepted {
                    connection.active_match = None;
                    connection.last_snapshot = None;
                    connection.force_resync_requested = true;
                    connection.pending_action = None;
                    outbox.locked = false;
                }
            }
            ConnectionEvent::Snapshot(snapshot) => match reconcile_snapshot(
                &snapshot,
                connection.active_match,
                connection.last_snapshot.as_ref(),
                &catalog,
                &mut game,
                &mut selection,
                &mut hovered,
                &mut transitions,
                &mut notices,
            ) {
                Ok(SnapshotDisposition::Replace | SnapshotDisposition::Equal) => {
                    if connection.active_match != Some(snapshot.match_id) {
                        if let Some(seat) = lobby.seat.as_mut() {
                            seat.match_id = snapshot.match_id;
                        }
                        if let Some(saved) = settings.saved_online_seat.as_mut() {
                            saved.match_id = snapshot.match_id;
                            if let Err(error) = settings.save() {
                                tracing::warn!(%error, "could not save rematch identity");
                            }
                        }
                    }
                    connection.active_match = Some(snapshot.match_id);
                    connection.last_snapshot = Some(snapshot_identity(&snapshot));
                    connection.phase = if snapshot.room_state == ConnectionState::Finished
                        || snapshot.state.outcome.is_some()
                    {
                        ConnectionPhase::Terminal
                    } else {
                        ConnectionPhase::Connected
                    };
                    *flow = ClientFlow::OnlinePlaying;
                    presentation_snapshots.write(AuthoritativePresentationSnapshot {
                        clocks: snapshot.state.clocks,
                        active_player: snapshot.state.active_player,
                        phase: snapshot.state.phase.clone(),
                        terminal: snapshot.state.outcome.is_some()
                            || snapshot.room_state == ConnectionState::Finished,
                        room_state: snapshot.room_state,
                    });
                    rematch_states.write(OnlineRematchStateChanged(snapshot.rematch_state));
                }
                Ok(SnapshotDisposition::Older) => {
                    tracing::debug!(
                        match_id = %snapshot.match_id,
                        revision = snapshot.revision,
                        "ignored older authoritative snapshot"
                    );
                }
                Ok(SnapshotDisposition::Diverged) => {
                    tracing::warn!(
                        match_id = %snapshot.match_id,
                        revision = snapshot.revision,
                        "equal-revision online snapshot hash mismatch; forcing resync"
                    );
                    connection.last_snapshot = None;
                    connection.force_resync_requested = true;
                    connection.pending_action = None;
                    connection.pending_control = None;
                    connection.phase = ConnectionPhase::Connecting;
                    outbox.locked = false;
                    outbox.intent = None;
                    control_outbox.locked = false;
                    control_outbox.message = None;
                    *selection = OverlaySelection::default();
                    interaction.resolve_online(
                        "Canonical state disagreed at the same revision; forcing a clean resync.",
                    );
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

#[allow(clippy::too_many_arguments)]
fn reconcile_snapshot(
    snapshot: &MatchSnapshot,
    active_match: Option<Uuid>,
    last_snapshot: Option<&SnapshotIdentity>,
    catalog: &ScenarioCatalog,
    game: &mut DisplayedGame,
    selection: &mut OverlaySelection,
    hovered: &mut HoveredBoardSquare,
    transitions: &mut LocalTransitionEventQueue,
    notices: &mut LocalTransitionNoticeLog,
) -> Result<SnapshotDisposition, String> {
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

    let displayed_hash = last_snapshot
        .filter(|last| last.match_id == snapshot.match_id && last.revision == snapshot.revision)
        .map(|_| game.state.canonical_hash())
        .transpose()
        .map_err(|_| "The displayed canonical state could not be verified.".to_owned())?;
    let disposition = classify_snapshot(last_snapshot, snapshot, displayed_hash.as_deref())?;
    if disposition != SnapshotDisposition::Replace {
        return Ok(disposition);
    }

    let selected = selection.piece;
    game.scenario = scenario.clone();
    game.state = snapshot.state.clone();
    selection.piece = selected.filter(|piece_id| selection_is_valid(*piece_id, &game.state));
    if hovered
        .0
        .is_some_and(|coord| !coord.is_within(game.scenario.board))
    {
        hovered.0 = None;
    }
    transitions.clear();
    notices.entries.clear();
    Ok(SnapshotDisposition::Replace)
}

fn snapshot_identity(snapshot: &MatchSnapshot) -> SnapshotIdentity {
    SnapshotIdentity {
        match_id: snapshot.match_id,
        scenario_id: snapshot.scenario_id.clone(),
        revision: snapshot.revision,
        state_hash: snapshot.state_hash.clone(),
    }
}

fn classify_snapshot(
    last: Option<&SnapshotIdentity>,
    incoming: &MatchSnapshot,
    displayed_hash: Option<&str>,
) -> Result<SnapshotDisposition, String> {
    let Some(last) = last else {
        return Ok(SnapshotDisposition::Replace);
    };
    if last.match_id != incoming.match_id {
        return Ok(SnapshotDisposition::Replace);
    }
    if last.scenario_id != incoming.scenario_id {
        return Err("The server changed scenarios within an active match.".to_owned());
    }
    if incoming.revision < last.revision {
        return Ok(SnapshotDisposition::Older);
    }
    if incoming.revision > last.revision {
        return Ok(SnapshotDisposition::Replace);
    }
    if incoming.state_hash == last.state_hash
        && displayed_hash.is_some_and(|displayed_hash| incoming.state_hash == displayed_hash)
    {
        Ok(SnapshotDisposition::Equal)
    } else {
        Ok(SnapshotDisposition::Diverged)
    }
}

fn selection_is_valid(
    piece_id: crownline_core::state::PieceId,
    state: &crownline_core::MatchState,
) -> bool {
    state.outcome.is_none()
        && matches!(state.phase, crownline_core::state::TurnPhase::Command)
        && state
            .pieces
            .get(&piece_id)
            .is_some_and(|piece| piece.owner == state.active_player)
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
    let pending = connection.pending_action.as_ref().map_or_else(
        || {
            connection.pending_control.as_ref().map_or_else(
                || "no command pending".to_owned(),
                |pending| {
                    format!(
                        "{:?} control pending · attempt {}",
                        pending.kind, pending.attempts
                    )
                },
            )
        },
        |pending| format!("command pending · attempt {}", pending.attempts),
    );
    for (mut text, mut visibility) in &mut text {
        *visibility = if settings.saved_online_seat.is_some()
            || !matches!(connection.phase, ConnectionPhase::Offline)
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        text.0 = format!(
            "ONLINE {phase} · {pending} · T retry · X cancel retry · F forget seat\n{}",
            connection.status,
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
        ConnectionCommand::Ready
        | ConnectionCommand::Action(_)
        | ConnectionCommand::Control(_)
        | ConnectionCommand::LeaveAndForget { .. }
        | ConnectionCommand::Retry
        | ConnectionCommand::Cancel => false,
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
                        ConnectionCommand::Action(request) => {
                            let action = ClientMessage::Action { protocol_version: PROTOCOL_VERSION, request };
                            if send_client_message(&mut socket, &action).await.is_err() { return SocketOutcome::Transient; }
                        }
                        ConnectionCommand::Control(message) => {
                            if send_client_message(&mut socket, &message).await.is_err() { return SocketOutcome::Transient; }
                        }
                        ConnectionCommand::LeaveAndForget { message, saved } => {
                            let _ = send_client_message(&mut socket, &message).await;
                            if let Some(saved) = saved {
                                let _ = vault.delete(saved.credential_id);
                            }
                            let _ = events.send(ConnectionEvent::Forgotten);
                            return SocketOutcome::Cancelled;
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
    if server_message_version(&message) != PROTOCOL_VERSION {
        let _ = events.send(ConnectionEvent::Notice(
            "The client and server protocol versions are incompatible.".to_owned(),
        ));
        return false;
    }
    let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Connected));
    match message {
        ServerMessage::Snapshot { snapshot, .. } => {
            let _ = events.send(ConnectionEvent::Snapshot(snapshot));
        }
        ServerMessage::Acknowledgement { result, .. } => {
            let _ = events.send(ConnectionEvent::Acknowledgement(result.clone()));
            let _ = events.send(ConnectionEvent::Snapshot(Box::new(result.snapshot.clone())));
        }
        ServerMessage::Error {
            code,
            message,
            retryable,
            snapshot,
            ..
        } => {
            if code == ErrorCode::Unauthorized && snapshot.is_none() {
                let _ = events.send(ConnectionEvent::Notice(
                    "The saved seat credential was rejected. Forget it or join again.".to_owned(),
                ));
                return false;
            }
            if let Some(snapshot) = snapshot {
                let _ = events.send(ConnectionEvent::Snapshot(snapshot));
            }
            let failure = classify_command_failure(code, &message);
            let _ = events.send(ConnectionEvent::CommandRejected { failure, retryable });
        }
        ServerMessage::ConnectionState {
            state: ConnectionState::Finished,
            ..
        } => {
            let _ = events.send(ConnectionEvent::RoomState(ConnectionState::Finished));
            let _ = events.send(ConnectionEvent::Phase(ConnectionPhase::Terminal));
        }
        ServerMessage::ConnectionState { state, .. } => {
            let _ = events.send(ConnectionEvent::RoomState(state));
        }
        ServerMessage::RematchState { state, .. } => {
            let _ = events.send(ConnectionEvent::RematchState(state));
        }
    }
    true
}

const fn server_message_version(message: &ServerMessage) -> u16 {
    match message {
        ServerMessage::Snapshot {
            protocol_version, ..
        }
        | ServerMessage::Acknowledgement {
            protocol_version, ..
        }
        | ServerMessage::Error {
            protocol_version, ..
        }
        | ServerMessage::ConnectionState {
            protocol_version, ..
        }
        | ServerMessage::RematchState {
            protocol_version, ..
        } => *protocol_version,
    }
}

fn classify_command_failure(code: ErrorCode, message: &str) -> CommandFailure {
    if code == ErrorCode::Unauthorized {
        CommandFailure::WrongTurn
    } else if code == ErrorCode::StaleRevision {
        CommandFailure::Stale
    } else if code == ErrorCode::InvalidAction && message == "Clock expired." {
        CommandFailure::ClockExpired
    } else {
        CommandFailure::Rejected
    }
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
                ConnectionCommand::Action(_) | ConnectionCommand::Control(_) => continue,
                ConnectionCommand::LeaveAndForget { saved, .. } => {
                    if let Some(saved) = saved {
                        let _ = vault.delete(saved.credential_id);
                    }
                    *target = None;
                    let _ = events.send(ConnectionEvent::Forgotten);
                    return false;
                }
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
    use crownline_core::{
        MatchState,
        scenario::{Coord, Player, ScenarioDefinition},
    };

    fn fixture_snapshot() -> (ScenarioDefinition, MatchSnapshot) {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let snapshot = MatchSnapshot {
            match_id: Uuid::new_v4(),
            revision: state.revision,
            scenario_id: scenario.id.clone(),
            scenario_hash: scenario.canonical_hash().unwrap(),
            state_hash: state.canonical_hash().unwrap(),
            state,
            room_state: ConnectionState::Connected,
            rematch_state: None,
        };
        (scenario, snapshot)
    }

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

    #[test]
    fn retry_clones_the_original_revision_and_idempotency_key() {
        let request = ActionRequest {
            context: MutationContext {
                match_id: Uuid::new_v4(),
                expected_revision: 17,
                idempotency_key: Uuid::new_v4(),
            },
            action: Action::Hold {
                player: Player::North,
            },
        };
        let pending = PendingAction {
            request: request.clone(),
            last_sent: Instant::now(),
            attempts: 1,
        };

        let ConnectionCommand::Action(first_retry) =
            ConnectionCommand::Action(pending.request.clone())
        else {
            unreachable!();
        };
        let ConnectionCommand::Action(second_retry) =
            ConnectionCommand::Action(pending.request.clone())
        else {
            unreachable!();
        };
        assert_eq!(first_retry, request);
        assert_eq!(
            second_retry.context.idempotency_key,
            request.context.idempotency_key
        );
        assert_eq!(second_retry.context.expected_revision, 17);
    }

    #[test]
    fn obsolete_turn_and_clock_failures_are_distinguished() {
        assert_eq!(
            classify_command_failure(ErrorCode::Unauthorized, "Wrong seat."),
            CommandFailure::WrongTurn
        );
        assert_eq!(
            classify_command_failure(ErrorCode::InvalidAction, "Clock expired."),
            CommandFailure::ClockExpired
        );
        assert_eq!(
            classify_command_failure(ErrorCode::StaleRevision, "ignored"),
            CommandFailure::Stale
        );
    }

    #[test]
    fn snapshot_ordering_ignores_old_checks_equal_and_detects_divergence() {
        let (_, mut incoming) = fixture_snapshot();
        let mut last = snapshot_identity(&incoming);
        last.revision = 4;
        last.state_hash = "revision-four".to_owned();

        incoming.revision = 3;
        assert_eq!(
            classify_snapshot(Some(&last), &incoming, Some("revision-four")).unwrap(),
            SnapshotDisposition::Older
        );
        incoming.revision = 4;
        incoming.state_hash = "revision-four".to_owned();
        assert_eq!(
            classify_snapshot(Some(&last), &incoming, Some("revision-four")).unwrap(),
            SnapshotDisposition::Equal
        );
        assert_eq!(
            classify_snapshot(Some(&last), &incoming, Some("different-local-hash")).unwrap(),
            SnapshotDisposition::Diverged
        );
        incoming.revision = 5;
        assert_eq!(
            classify_snapshot(Some(&last), &incoming, Some("revision-four")).unwrap(),
            SnapshotDisposition::Replace
        );
    }

    #[test]
    fn canonical_replacement_preserves_only_valid_board_context() {
        let (scenario, snapshot) = fixture_snapshot();
        let mut game = DisplayedGame {
            scenario: scenario.clone(),
            state: snapshot.state.clone(),
        };
        let active_piece = game
            .state
            .pieces
            .values()
            .find(|piece| piece.owner == game.state.active_player)
            .unwrap()
            .id;
        let mut selection = OverlaySelection {
            piece: Some(active_piece),
        };
        let mut hovered = HoveredBoardSquare(Some(Coord::new(999, 999)));
        let mut transitions = LocalTransitionEventQueue::default();
        let mut notices = LocalTransitionNoticeLog::default();

        assert_eq!(
            reconcile_snapshot(
                &snapshot,
                Some(snapshot.match_id),
                None,
                &ScenarioCatalog(vec![scenario]),
                &mut game,
                &mut selection,
                &mut hovered,
                &mut transitions,
                &mut notices,
            )
            .unwrap(),
            SnapshotDisposition::Replace
        );
        assert_eq!(selection.piece, Some(active_piece));
        assert_eq!(hovered.0, None);

        let inactive_piece = game
            .state
            .pieces
            .values()
            .find(|piece| piece.owner != game.state.active_player)
            .unwrap()
            .id;
        selection.piece = Some(inactive_piece);
        let scenario = game.scenario.clone();
        assert_eq!(
            reconcile_snapshot(
                &snapshot,
                Some(snapshot.match_id),
                None,
                &ScenarioCatalog(vec![scenario]),
                &mut game,
                &mut selection,
                &mut hovered,
                &mut transitions,
                &mut notices,
            )
            .unwrap(),
            SnapshotDisposition::Replace
        );
        assert_eq!(selection.piece, None);
    }

    #[test]
    fn server_protocol_version_is_checked_before_snapshot_delivery() {
        let (_, snapshot) = fixture_snapshot();
        let message = ServerMessage::Snapshot {
            protocol_version: PROTOCOL_VERSION + 1,
            snapshot: Box::new(snapshot),
        };
        assert_eq!(server_message_version(&message), PROTOCOL_VERSION + 1);
    }
}
