use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use bevy::{
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    text::{EditableText, TextCursorStyle},
};
use crownline_core::{
    ClockSettings, MAX_BASE_MINUTES, MAX_INCREMENT_SECONDS, MIN_BASE_MINUTES, scenario::Player,
};
use crownline_protocol::{
    CreateRoomRequest, CreateRoomResponse, ErrorCode, JoinRoomRequest, JoinRoomResponse,
    MAX_HTTP_REQUEST_BYTES, PROTOCOL_VERSION, ReconnectToken, ServerMessage, validate_create_room,
    validate_join_room,
};
use reqwest::{StatusCode, Url, blocking::Client};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    config::ClientSettings,
    lifecycle::{ClientFlow, ScenarioCatalog},
};

const NETWORK_QUEUE_CAPACITY: usize = 1;
const MAX_HTTP_RESPONSE_BYTES: usize = MAX_HTTP_REQUEST_BYTES * 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LobbyScreen {
    #[default]
    Closed,
    Menu,
    Host,
    Join,
    Waiting,
}

#[derive(Debug, Clone)]
pub(crate) struct OnlineSeat {
    pub(crate) match_id: uuid::Uuid,
    pub(crate) room_code: String,
    pub(crate) seat: Player,
    pub(crate) reconnect_token: ReconnectToken,
}

impl OnlineSeat {
    fn has_credential(&self) -> bool {
        !self.reconnect_token.expose().is_empty()
    }
}

#[derive(Resource)]
pub(crate) struct OnlineLobby {
    pub(crate) screen: LobbyScreen,
    pub(crate) server_url: String,
    player_name: String,
    room_code: String,
    selected_scenario: usize,
    clock: Option<ClockSettings>,
    share_server_address: bool,
    request_pending: bool,
    pub(crate) ready_requested: bool,
    pub(crate) status: String,
    pub(crate) seat: Option<OnlineSeat>,
}

impl FromWorld for OnlineLobby {
    fn from_world(world: &mut World) -> Self {
        let server_url = world.resource::<ClientSettings>().server_url.clone();
        let selected_scenario = world
            .resource::<ScenarioCatalog>()
            .0
            .iter()
            .position(|scenario| scenario.metadata.is_default)
            .unwrap_or(0);
        Self {
            screen: LobbyScreen::Closed,
            server_url,
            player_name: "Player".to_owned(),
            room_code: String::new(),
            selected_scenario,
            clock: None,
            share_server_address: false,
            request_pending: false,
            ready_requested: false,
            status: String::new(),
            seat: None,
        }
    }
}

impl OnlineLobby {
    pub(crate) fn open_menu(&mut self) {
        self.screen = LobbyScreen::Menu;
        self.status.clear();
    }
}

#[derive(Component)]
struct OnlineLobbyRoot;

#[derive(Component)]
struct OnlineLobbyText;

#[derive(Debug, Clone, Copy, Component)]
enum OnlineField {
    ServerUrl,
    PlayerName,
    RoomCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
enum LobbyControl {
    Host,
    Join,
    Back,
    PreviousScenario,
    NextScenario,
    ToggleClock,
    DecreaseBase,
    IncreaseBase,
    DecreaseIncrement,
    IncreaseIncrement,
    Create,
    SubmitJoin,
    ToggleShareAddress,
    CopyInvitation,
    Ready,
    Leave,
}

#[derive(Debug)]
enum LobbyRequest {
    Create {
        server_url: String,
        request: CreateRoomRequest,
    },
    Join {
        server_url: String,
        request: JoinRoomRequest,
    },
}

#[derive(Debug)]
enum LobbyResponse {
    Created(CreateRoomResponse),
    Joined(JoinRoomResponse),
    Failed(String),
}

#[derive(Resource)]
struct LobbyTransport {
    requests: mpsc::SyncSender<LobbyRequest>,
    responses: Arc<Mutex<mpsc::Receiver<LobbyResponse>>>,
}

impl Default for LobbyTransport {
    fn default() -> Self {
        let (request_sender, request_receiver) = mpsc::sync_channel(NETWORK_QUEUE_CAPACITY);
        let (response_sender, response_receiver) = mpsc::sync_channel(NETWORK_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("crownline-lobby-http".to_owned())
            .spawn(move || lobby_worker(&request_receiver, &response_sender))
            .expect("lobby HTTP worker must start");
        Self {
            requests: request_sender,
            responses: Arc::new(Mutex::new(response_receiver)),
        }
    }
}

pub struct OnlineLobbyPlugin;

impl Plugin for OnlineLobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnlineLobby>()
            .init_resource::<LobbyTransport>()
            .add_systems(Startup, spawn_online_lobby)
            .add_systems(
                Update,
                (
                    handle_online_lobby_input,
                    poll_lobby_transport,
                    sync_online_lobby_ui,
                )
                    .chain(),
            );
    }
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_online_lobby(mut commands: Commands, settings: Res<ClientSettings>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(16),
                top: percent(9),
                width: percent(68),
                min_height: percent(70),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(10),
                padding: UiRect::all(px(18)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.035, 0.06, 0.985)),
            GlobalZIndex(85),
            Visibility::Hidden,
            OnlineLobbyRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("CROWNLINES - ONLINE"),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.94, 1.0)),
                TextLayout::justify(Justify::Center),
                OnlineLobbyText,
            ));
            root.spawn(online_input(
                &settings.server_url,
                OnlineField::ServerUrl,
                10,
            ));
            root.spawn(online_input("Player", OnlineField::PlayerName, 11));
            root.spawn(online_input("", OnlineField::RoomCode, 12));
            root.spawn(Node {
                display: Display::Flex,
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::Center,
                column_gap: px(7),
                row_gap: px(7),
                ..default()
            })
            .with_children(|controls| {
                for (label, control) in [
                    ("Host private room", LobbyControl::Host),
                    ("Join with code", LobbyControl::Join),
                    ("Back", LobbyControl::Back),
                    ("Previous scenario", LobbyControl::PreviousScenario),
                    ("Next scenario", LobbyControl::NextScenario),
                    ("Toggle clock", LobbyControl::ToggleClock),
                    ("Base -", LobbyControl::DecreaseBase),
                    ("Base +", LobbyControl::IncreaseBase),
                    ("Increment -", LobbyControl::DecreaseIncrement),
                    ("Increment +", LobbyControl::IncreaseIncrement),
                    ("Create room", LobbyControl::Create),
                    ("Join room", LobbyControl::SubmitJoin),
                    ("Include server address", LobbyControl::ToggleShareAddress),
                    ("Copy invitation", LobbyControl::CopyInvitation),
                    ("Ready", LobbyControl::Ready),
                    ("Leave", LobbyControl::Leave),
                ] {
                    controls.spawn(lobby_button(label, control));
                }
            });
        });
}

fn lobby_button(label: &'static str, control: LobbyControl) -> impl Bundle {
    (
        Button,
        Node {
            min_height: px(36),
            padding: UiRect::axes(px(10), px(6)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.11, 0.2, 0.29)),
        control,
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.94, 1.0)),
        )],
    )
}

fn online_input(value: &str, field: OnlineField, tab: i32) -> impl Bundle {
    (
        Node {
            width: percent(78),
            min_height: px(36),
            border: UiRect::all(px(2)),
            padding: UiRect::all(px(6)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.42, 0.55, 0.72)),
        BackgroundColor(Color::srgb(0.08, 0.1, 0.16)),
        EditableText::new(value),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextCursorStyle::default(),
        TabIndex(tab),
        field,
    )
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn handle_online_lobby_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut flow: ResMut<ClientFlow>,
    mut lobby: ResMut<OnlineLobby>,
    transport: Res<LobbyTransport>,
    catalog: Res<ScenarioCatalog>,
    fields: Query<(&EditableText, &OnlineField)>,
    buttons: Query<(&Interaction, &LobbyControl), Changed<Interaction>>,
) {
    let pressed = buttons
        .iter()
        .filter_map(|(interaction, control)| {
            (*interaction == Interaction::Pressed).then_some(*control)
        })
        .collect::<Vec<_>>();
    if *flow == ClientFlow::Setup && keys.just_pressed(KeyCode::F3) {
        lobby.open_menu();
        *flow = ClientFlow::OnlineLobby;
        return;
    }
    if *flow != ClientFlow::OnlineLobby {
        return;
    }
    sync_field_values(&mut lobby, &fields);
    match lobby.screen {
        LobbyScreen::Closed => {}
        LobbyScreen::Menu => {
            if keys.just_pressed(KeyCode::KeyH) || pressed.contains(&LobbyControl::Host) {
                lobby.screen = LobbyScreen::Host;
            } else if keys.just_pressed(KeyCode::KeyJ) || pressed.contains(&LobbyControl::Join) {
                lobby.screen = LobbyScreen::Join;
            } else if keys.just_pressed(KeyCode::Escape) || pressed.contains(&LobbyControl::Back) {
                lobby.screen = LobbyScreen::Closed;
                *flow = ClientFlow::Setup;
            }
        }
        LobbyScreen::Host => {
            update_host_settings(&keys, &pressed, &mut lobby, catalog.0.len());
            if (keys.just_pressed(KeyCode::Enter) || pressed.contains(&LobbyControl::Create))
                && !lobby.request_pending
            {
                submit_create(&mut lobby, &transport, &catalog);
            } else if (keys.just_pressed(KeyCode::Escape) || pressed.contains(&LobbyControl::Back))
                && !lobby.request_pending
            {
                lobby.screen = LobbyScreen::Menu;
            }
        }
        LobbyScreen::Join => {
            if (keys.just_pressed(KeyCode::Enter) || pressed.contains(&LobbyControl::SubmitJoin))
                && !lobby.request_pending
            {
                submit_join(&mut lobby, &transport);
            } else if (keys.just_pressed(KeyCode::Escape) || pressed.contains(&LobbyControl::Back))
                && !lobby.request_pending
            {
                lobby.screen = LobbyScreen::Menu;
            }
        }
        LobbyScreen::Waiting => {
            if keys.just_pressed(KeyCode::KeyA)
                || pressed.contains(&LobbyControl::ToggleShareAddress)
            {
                lobby.share_server_address = !lobby.share_server_address;
            }
            if keys.just_pressed(KeyCode::KeyC) || pressed.contains(&LobbyControl::CopyInvitation) {
                copy_room_share(&mut lobby);
            }
            if keys.just_pressed(KeyCode::KeyR) || pressed.contains(&LobbyControl::Ready) {
                lobby.ready_requested = true;
                "Ready selected. Waiting for the authoritative match connection."
                    .clone_into(&mut lobby.status);
            }
            if keys.just_pressed(KeyCode::Escape) || pressed.contains(&LobbyControl::Leave) {
                lobby.seat = None;
                lobby.ready_requested = false;
                lobby.screen = LobbyScreen::Menu;
            }
        }
    }
}

fn sync_field_values(lobby: &mut OnlineLobby, fields: &Query<(&EditableText, &OnlineField)>) {
    for (value, field) in fields {
        match field {
            OnlineField::ServerUrl => lobby.server_url = value.value().to_string(),
            OnlineField::PlayerName => lobby.player_name = value.value().to_string(),
            OnlineField::RoomCode => lobby.room_code = value.value().to_string(),
        }
    }
}

fn update_host_settings(
    keys: &ButtonInput<KeyCode>,
    pressed: &[LobbyControl],
    lobby: &mut OnlineLobby,
    scenario_count: usize,
) {
    if keys.just_pressed(KeyCode::PageUp) || pressed.contains(&LobbyControl::PreviousScenario) {
        lobby.selected_scenario = (lobby.selected_scenario + scenario_count - 1) % scenario_count;
    }
    if keys.just_pressed(KeyCode::PageDown) || pressed.contains(&LobbyControl::NextScenario) {
        lobby.selected_scenario = (lobby.selected_scenario + 1) % scenario_count;
    }
    if keys.just_pressed(KeyCode::KeyC) || pressed.contains(&LobbyControl::ToggleClock) {
        lobby.clock = lobby.clock.is_none().then_some(ClockSettings {
            base_minutes: 10,
            increment_seconds: 0,
        });
    }
    let Some(clock) = lobby.clock.as_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::Minus) || pressed.contains(&LobbyControl::DecreaseBase) {
        clock.base_minutes = clock.base_minutes.saturating_sub(1).max(MIN_BASE_MINUTES);
    }
    if keys.just_pressed(KeyCode::Equal) || pressed.contains(&LobbyControl::IncreaseBase) {
        clock.base_minutes = clock.base_minutes.saturating_add(1).min(MAX_BASE_MINUTES);
    }
    if keys.just_pressed(KeyCode::Comma) || pressed.contains(&LobbyControl::DecreaseIncrement) {
        clock.increment_seconds = clock.increment_seconds.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::Period) || pressed.contains(&LobbyControl::IncreaseIncrement) {
        clock.increment_seconds = clock
            .increment_seconds
            .saturating_add(1)
            .min(MAX_INCREMENT_SECONDS);
    }
}

fn submit_create(lobby: &mut OnlineLobby, transport: &LobbyTransport, catalog: &ScenarioCatalog) {
    let request = CreateRoomRequest {
        protocol_version: PROTOCOL_VERSION,
        player_name: lobby.player_name.trim().to_owned(),
        scenario_id: catalog.0[lobby.selected_scenario].id.clone(),
        clock: lobby.clock,
    };
    if let Err(error) = validate_create_room(&request) {
        lobby.status = safe_protocol_error(&error);
        return;
    }
    if let Err(error) = http_endpoint(&lobby.server_url, "/rooms") {
        lobby.status = error;
        return;
    }
    let command = LobbyRequest::Create {
        server_url: lobby.server_url.clone(),
        request,
    };
    match transport.requests.try_send(command) {
        Ok(()) => {
            lobby.request_pending = true;
            "Creating private room…".clone_into(&mut lobby.status);
        }
        Err(_) => "A room request is already in progress.".clone_into(&mut lobby.status),
    }
}

fn submit_join(lobby: &mut OnlineLobby, transport: &LobbyTransport) {
    let request = JoinRoomRequest {
        protocol_version: PROTOCOL_VERSION,
        player_name: lobby.player_name.trim().to_owned(),
        room_code: normalize_room_code(&lobby.room_code),
    };
    if let Err(error) = validate_join_room(&request) {
        lobby.status = safe_protocol_error(&error);
        return;
    }
    if let Err(error) = http_endpoint(&lobby.server_url, "/rooms/join") {
        lobby.status = error;
        return;
    }
    let command = LobbyRequest::Join {
        server_url: lobby.server_url.clone(),
        request,
    };
    match transport.requests.try_send(command) {
        Ok(()) => {
            lobby.request_pending = true;
            "Joining private room…".clone_into(&mut lobby.status);
        }
        Err(_) => "A room request is already in progress.".clone_into(&mut lobby.status),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn poll_lobby_transport(mut lobby: ResMut<OnlineLobby>, transport: Res<LobbyTransport>) {
    let response = transport
        .responses
        .lock()
        .ok()
        .and_then(|responses| responses.try_recv().ok());
    let Some(response) = response else {
        return;
    };
    lobby.request_pending = false;
    match response {
        LobbyResponse::Created(response) => {
            let room_code = response.room_code.clone();
            lobby.seat = Some(OnlineSeat {
                match_id: response.match_id,
                room_code: response.room_code,
                seat: response.seat,
                reconnect_token: response.reconnect_token,
            });
            lobby.screen = LobbyScreen::Waiting;
            lobby.status = format!("Room {room_code} created. Waiting for an opponent.");
        }
        LobbyResponse::Joined(response) => {
            let room_code = normalize_room_code(&lobby.room_code);
            lobby.seat = Some(OnlineSeat {
                match_id: response.match_id,
                room_code: room_code.clone(),
                seat: response.seat,
                reconnect_token: response.reconnect_token,
            });
            lobby.screen = LobbyScreen::Waiting;
            lobby.status = format!("Joined room {room_code}. Select ready when prepared.");
        }
        LobbyResponse::Failed(message) => lobby.status = message,
    }
}

fn copy_room_share(lobby: &mut OnlineLobby) {
    let Some(room_code) = lobby.seat.as_ref().map(|seat| seat.room_code.clone()) else {
        return;
    };
    let server_address = if lobby.share_server_address {
        match shareable_server_address(&lobby.server_url) {
            Ok(address) => Some(address),
            Err(message) => {
                lobby.status = message;
                return;
            }
        }
    } else {
        None
    };
    let text = room_share_text(&room_code, server_address.as_deref());
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
        Ok(()) => {
            "Room invitation copied without the seat credential.".clone_into(&mut lobby.status);
        }
        Err(_) => {
            "Clipboard unavailable; copy the displayed room code manually."
                .clone_into(&mut lobby.status);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_online_lobby_ui(
    flow: Res<ClientFlow>,
    lobby: Res<OnlineLobby>,
    catalog: Res<ScenarioCatalog>,
    mut roots: Query<&mut Visibility, With<OnlineLobbyRoot>>,
    mut texts: Query<&mut Text, With<OnlineLobbyText>>,
    mut controls: Query<(&LobbyControl, &mut Visibility)>,
    mut fields: Query<(&OnlineField, &mut Node)>,
) {
    for mut visibility in &mut roots {
        *visibility = if *flow == ClientFlow::OnlineLobby {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    let body = match lobby.screen {
        LobbyScreen::Closed => String::new(),
        LobbyScreen::Menu => "CROWNLINES - ONLINE\nH host a private room - J join with a code\nTab edits server/name fields - Esc local setup".to_owned(),
        LobbyScreen::Host => {
            let scenario = &catalog.0[lobby.selected_scenario];
            let scenario_label = format!(
                "{} - {}x{}",
                scenario.metadata.name, scenario.board.width, scenario.board.height
            );
            let clock = lobby.clock.map_or_else(
                || "untimed".to_owned(),
                |clock| format!("{} min + {} sec", clock.base_minutes, clock.increment_seconds),
            );
            format!(
                "HOST PRIVATE ROOM\nServer: {}\nName: {}\nScenario: {} - Clock: {clock}\nPageUp/PageDown scenario - C clock - -/+ base - ,/. increment\nEnter create - Esc back\n{}",
                lobby.server_url,
                lobby.player_name,
                scenario_label,
                lobby.status,
            )
        }
        LobbyScreen::Join => format!(
            "JOIN PRIVATE ROOM\nServer: {}\nName: {}\nRoom code: {}\nTab edits fields - Enter join - Esc back\n{}",
            lobby.server_url,
            lobby.player_name,
            normalize_room_code(&lobby.room_code),
            lobby.status,
        ),
        LobbyScreen::Waiting => {
            let seat = lobby.seat.as_ref();
            format!(
                "PRIVATE ROOM {}\nSeat: {} - Match: {} - Credential stored: {}\nR ready - C copy invitation - A include server address: {} - Esc leave screen\n{}",
                seat.map_or("------", |seat| seat.room_code.as_str()),
                seat.map_or_else(|| "unknown".to_owned(), |seat| format!("{:?}", seat.seat)),
                seat.map_or_else(|| "unknown".to_owned(), |seat| seat.match_id.to_string()),
                if seat.is_some_and(OnlineSeat::has_credential) { "yes" } else { "no" },
                if lobby.share_server_address { "yes" } else { "no" },
                lobby.status,
            )
        }
    };
    for mut text in &mut texts {
        text.0.clone_from(&body);
    }
    for (control, mut visibility) in &mut controls {
        let visible = match lobby.screen {
            LobbyScreen::Closed => false,
            LobbyScreen::Menu => matches!(
                control,
                LobbyControl::Host | LobbyControl::Join | LobbyControl::Back
            ),
            LobbyScreen::Host => match control {
                LobbyControl::Back
                | LobbyControl::PreviousScenario
                | LobbyControl::NextScenario
                | LobbyControl::ToggleClock
                | LobbyControl::Create => true,
                LobbyControl::DecreaseBase
                | LobbyControl::IncreaseBase
                | LobbyControl::DecreaseIncrement
                | LobbyControl::IncreaseIncrement => lobby.clock.is_some(),
                _ => false,
            },
            LobbyScreen::Join => {
                matches!(control, LobbyControl::Back | LobbyControl::SubmitJoin)
            }
            LobbyScreen::Waiting => matches!(
                control,
                LobbyControl::ToggleShareAddress
                    | LobbyControl::CopyInvitation
                    | LobbyControl::Ready
                    | LobbyControl::Leave
            ),
        };
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (field, mut node) in &mut fields {
        node.display = match lobby.screen {
            LobbyScreen::Closed | LobbyScreen::Waiting => Display::None,
            LobbyScreen::Menu | LobbyScreen::Host => match field {
                OnlineField::ServerUrl | OnlineField::PlayerName => Display::Flex,
                OnlineField::RoomCode => Display::None,
            },
            LobbyScreen::Join => Display::Flex,
        };
    }
}

fn lobby_worker(
    requests: &mpsc::Receiver<LobbyRequest>,
    responses: &mpsc::SyncSender<LobbyResponse>,
) {
    let Ok(client) = Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        let _ = responses.send(LobbyResponse::Failed(
            "The network client could not be initialized.".to_owned(),
        ));
        return;
    };
    while let Ok(request) = requests.recv() {
        let result = match request {
            LobbyRequest::Create {
                server_url,
                request,
            } => post_json::<_, CreateRoomResponse>(&client, &server_url, "/rooms", &request)
                .and_then(validate_response_version)
                .map(LobbyResponse::Created),
            LobbyRequest::Join {
                server_url,
                request,
            } => post_json::<_, JoinRoomResponse>(&client, &server_url, "/rooms/join", &request)
                .and_then(validate_response_version)
                .map(LobbyResponse::Joined),
        }
        .unwrap_or_else(LobbyResponse::Failed);
        if responses.send(result).is_err() {
            return;
        }
    }
}

fn validate_response_version<T: RoomResponseVersion>(response: T) -> Result<T, String> {
    if response.protocol_version() == PROTOCOL_VERSION {
        Ok(response)
    } else {
        Err("The client and server versions are incompatible.".to_owned())
    }
}

trait RoomResponseVersion {
    fn protocol_version(&self) -> u16;
}

impl RoomResponseVersion for CreateRoomResponse {
    fn protocol_version(&self) -> u16 {
        self.protocol_version
    }
}

impl RoomResponseVersion for JoinRoomResponse {
    fn protocol_version(&self) -> u16 {
        self.protocol_version
    }
}

fn post_json<T: Serialize, R: DeserializeOwned>(
    client: &Client,
    server_url: &str,
    path: &str,
    request: &T,
) -> Result<R, String> {
    let endpoint = http_endpoint(server_url, path)?;
    let response = client.post(endpoint).json(request).send().map_err(|_| {
        "The server could not be reached. Check the address and try again.".to_owned()
    })?;
    let status = response.status();
    let bytes = response
        .bytes()
        .map_err(|_| "The server response could not be read.".to_owned())?;
    if bytes.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err("The server response was unexpectedly large.".to_owned());
    }
    if status.is_success() {
        return serde_json::from_slice(&bytes)
            .map_err(|_| "The server returned an incompatible response.".to_owned());
    }
    Err(decode_public_server_error(status, &bytes))
}

fn decode_public_server_error(status: StatusCode, bytes: &[u8]) -> String {
    let code = serde_json::from_slice::<ServerMessage>(bytes)
        .ok()
        .and_then(|message| match message {
            ServerMessage::Error { code, .. } => Some(code),
            _ => None,
        });
    match code {
        Some(ErrorCode::RoomNotFound) => "That room was not found.".to_owned(),
        Some(ErrorCode::RoomFull) => "That room already has two players.".to_owned(),
        Some(ErrorCode::RateLimited) => "Too many requests; wait briefly and retry.".to_owned(),
        Some(ErrorCode::IncompatibleProtocol) => {
            "The client and server versions are incompatible.".to_owned()
        }
        Some(ErrorCode::InvalidRequest) => "The room details were rejected.".to_owned(),
        _ if status.is_server_error() => "The server is temporarily unavailable.".to_owned(),
        _ => "The server rejected the room request.".to_owned(),
    }
}

fn http_endpoint(server_url: &str, path: &str) -> Result<Url, String> {
    let mut url = Url::parse(server_url.trim())
        .map_err(|_| "Enter a valid ws:// or wss:// server address.".to_owned())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Server addresses cannot contain credentials, query data, or fragments.".to_owned(),
        );
    }
    match url.scheme() {
        "wss" => url
            .set_scheme("https")
            .map_err(|()| "The secure server address is invalid.".to_owned())?,
        "ws" if is_local_host(url.host_str()) => url
            .set_scheme("http")
            .map_err(|()| "The local server address is invalid.".to_owned())?,
        "ws" => {
            return Err(
                "Remote servers must use wss:// so seat setup is protected by TLS.".to_owned(),
            );
        }
        _ => return Err("Server addresses must begin with ws:// or wss://.".to_owned()),
    }
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn shareable_server_address(server_url: &str) -> Result<String, String> {
    let _ = http_endpoint(server_url, "/rooms")?;
    let mut url = Url::parse(server_url.trim())
        .map_err(|_| "Enter a valid ws:// or wss:// server address.".to_owned())?;
    url.set_path("");
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn is_local_host(host: Option<&str>) -> bool {
    matches!(host, Some("localhost" | "127.0.0.1" | "::1"))
}

fn normalize_room_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

fn room_share_text(room_code: &str, server_url: Option<&str>) -> String {
    server_url.map_or_else(
        || room_code.to_owned(),
        |server_url| format!("{room_code}\n{}", server_url.trim()),
    )
}

fn safe_protocol_error(error: &crownline_protocol::ProtocolError) -> String {
    use crownline_protocol::ProtocolError;
    match error {
        ProtocolError::InvalidPlayerName => "Enter a player name of 1-24 characters.".to_owned(),
        ProtocolError::InvalidRoomCode => "Enter the six-character room code.".to_owned(),
        ProtocolError::InvalidScenarioId => "Select an installed scenario.".to_owned(),
        ProtocolError::InvalidClock => "Choose a supported clock configuration.".to_owned(),
        _ => "The room details are invalid.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_codes_are_normalized_before_protocol_validation() {
        assert_eq!(normalize_room_code(" ab-c 234 "), "ABC234");
        let request = JoinRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Ada".to_owned(),
            room_code: normalize_room_code(" ab-c 234 "),
        };
        assert_eq!(validate_join_room(&request), Ok(()));
    }

    #[test]
    fn remote_plaintext_servers_are_rejected_but_loopback_is_allowed() {
        assert_eq!(
            http_endpoint("ws://127.0.0.1:5000", "/rooms")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:5000/rooms"
        );
        assert!(http_endpoint("ws://example.com", "/rooms").is_err());
        assert_eq!(
            http_endpoint("wss://play.example.com/socket", "/rooms")
                .unwrap()
                .as_str(),
            "https://play.example.com/rooms"
        );
        assert!(http_endpoint("wss://user@play.example.com?secret=no", "/rooms").is_err());
    }

    #[test]
    fn invitation_text_never_contains_the_reconnect_credential() {
        let seat = OnlineSeat {
            match_id: uuid::Uuid::nil(),
            room_code: "ABC234".to_owned(),
            seat: Player::North,
            reconnect_token: ReconnectToken::issued("top-secret".to_owned()),
        };
        let invitation = room_share_text(&seat.room_code, Some("wss://play.example.com"));
        assert_eq!(invitation, "ABC234\nwss://play.example.com");
        assert!(!invitation.contains(seat.reconnect_token.expose()));
        assert_eq!(
            shareable_server_address("wss://play.example.com/socket").unwrap(),
            "wss://play.example.com"
        );
    }

    #[test]
    fn server_error_bodies_are_mapped_to_fixed_public_copy() {
        let body = serde_json::to_vec(&ServerMessage::Error {
            protocol_version: PROTOCOL_VERSION,
            code: ErrorCode::RoomNotFound,
            message: "sensitive internal locator".to_owned(),
            retryable: false,
            snapshot: None,
        })
        .unwrap();
        let message = decode_public_server_error(StatusCode::NOT_FOUND, &body);
        assert_eq!(message, "That room was not found.");
        assert!(!message.contains("sensitive"));
    }

    #[test]
    fn authored_default_scenario_and_response_version_drive_lobby_state() {
        let mut world = World::new();
        world.insert_resource(ClientSettings::default());
        world.init_resource::<ScenarioCatalog>();
        let lobby = OnlineLobby::from_world(&mut world);
        assert!(
            world.resource::<ScenarioCatalog>().0[lobby.selected_scenario]
                .metadata
                .is_default
        );

        let response = JoinRoomResponse {
            protocol_version: PROTOCOL_VERSION + 1,
            match_id: uuid::Uuid::nil(),
            seat: Player::South,
            reconnect_token: ReconnectToken::issued("secret".to_owned()),
        };
        assert_eq!(
            validate_response_version(response).unwrap_err(),
            "The client and server versions are incompatible."
        );
    }

    #[test]
    fn ready_selection_does_not_enter_gameplay_without_a_snapshot() {
        let mut app = App::new();
        app.insert_resource(ClientSettings::default())
            .init_resource::<ScenarioCatalog>()
            .init_resource::<OnlineLobby>()
            .init_resource::<LobbyTransport>()
            .insert_resource(ClientFlow::OnlineLobby)
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(Update, handle_online_lobby_input);
        {
            let mut lobby = app.world_mut().resource_mut::<OnlineLobby>();
            lobby.screen = LobbyScreen::Waiting;
            lobby.seat = Some(OnlineSeat {
                match_id: uuid::Uuid::nil(),
                room_code: "ABC234".to_owned(),
                seat: Player::North,
                reconnect_token: ReconnectToken::issued("secret".to_owned()),
            });
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.update();
        assert_eq!(
            *app.world().resource::<ClientFlow>(),
            ClientFlow::OnlineLobby
        );
        assert!(app.world().resource::<OnlineLobby>().ready_requested);
    }

    #[test]
    fn online_lobby_scrolls_instead_of_clipping_scaled_content() {
        let mut app = App::new();
        app.insert_resource(ClientSettings::default())
            .add_systems(Startup, spawn_online_lobby);
        app.update();
        let world = app.world_mut();
        let mut roots = world.query_filtered::<&Node, With<OnlineLobbyRoot>>();
        assert_eq!(roots.single(world).unwrap().overflow, Overflow::scroll_y());

        let mut controls = world.query::<&LobbyControl>();
        let exposed: std::collections::BTreeSet<_> = controls
            .iter(world)
            .map(|control| format!("{control:?}"))
            .collect();
        assert_eq!(exposed.len(), 16);
        for required in ["Host", "Join", "Create", "SubmitJoin", "Ready", "Leave"] {
            assert!(exposed.contains(required), "missing {required} control");
        }
    }
}
