use bevy::prelude::*;
use crownline_core::{
    scenario::Player,
    state::{ClockState, TurnPhase},
};
use crownline_protocol::ConnectionState;
use std::time::Duration;

use crate::{
    lifecycle::ClientFlow,
    online_connection::{ConnectionPhase, OnlineConnection, OnlineIntentOutbox},
    online_lobby::OnlineLobby,
    ui_layout::SIDE_REGION_PERCENT,
};

const DRIFT_CORRECTION_SECONDS: f64 = 0.35;

#[derive(Debug, Clone, Message)]
pub(crate) struct AuthoritativePresentationSnapshot {
    pub(crate) clocks: Option<ClockState>,
    pub(crate) active_player: Player,
    pub(crate) phase: TurnPhase,
    pub(crate) terminal: bool,
    pub(crate) room_state: ConnectionState,
}

#[derive(Debug, Clone, Copy, Message)]
pub(crate) struct OnlineRoomStateChanged(pub(crate) ConnectionState);

#[derive(Debug, Clone, Copy, PartialEq)]
struct ClockPair {
    north_millis: f64,
    south_millis: f64,
}

#[allow(clippy::cast_precision_loss)]
impl From<ClockState> for ClockPair {
    fn from(clocks: ClockState) -> Self {
        Self {
            north_millis: clocks.north_millis as f64,
            south_millis: clocks.south_millis as f64,
        }
    }
}

#[derive(Debug, Resource)]
struct OnlineMatchPresentation {
    displayed: Option<ClockPair>,
    target: Option<ClockPair>,
    active_player: Player,
    phase: TurnPhase,
    room_state: ConnectionState,
    terminal: bool,
}

impl Default for OnlineMatchPresentation {
    fn default() -> Self {
        Self {
            displayed: None,
            target: None,
            active_player: Player::North,
            phase: TurnPhase::Command,
            room_state: ConnectionState::WaitingForOpponent,
            terminal: false,
        }
    }
}

impl OnlineMatchPresentation {
    fn observe(&mut self, snapshot: AuthoritativePresentationSnapshot) {
        self.active_player = snapshot.active_player;
        self.phase = snapshot.phase;
        self.room_state = snapshot.room_state;
        self.terminal = snapshot.terminal;
        self.target = snapshot.clocks.map(ClockPair::from);
        if self.displayed.is_none() || self.terminal {
            self.displayed = self.target;
        }
    }

    fn tick(&mut self, seconds: f64) {
        let seconds = seconds.max(0.0);
        let running = !self.terminal
            && matches!(
                self.room_state,
                ConnectionState::Connected | ConnectionState::OpponentDisconnected
            );
        if running {
            let elapsed_millis = seconds * 1_000.0;
            if let Some(target) = self.target.as_mut() {
                subtract_active(target, self.active_player, elapsed_millis);
            }
            if let Some(displayed) = self.displayed.as_mut() {
                subtract_active(displayed, self.active_player, elapsed_millis);
            }
        }
        let correction = (seconds / DRIFT_CORRECTION_SECONDS).clamp(0.0, 1.0);
        if let (Some(displayed), Some(target)) = (self.displayed.as_mut(), self.target) {
            displayed.north_millis += (target.north_millis - displayed.north_millis) * correction;
            displayed.south_millis += (target.south_millis - displayed.south_millis) * correction;
        }
    }
}

fn subtract_active(clocks: &mut ClockPair, active: Player, elapsed_millis: f64) {
    let remaining = match active {
        Player::North => &mut clocks.north_millis,
        Player::South => &mut clocks.south_millis,
    };
    *remaining = (*remaining - elapsed_millis).max(0.0);
}

#[derive(Component)]
struct OnlineMatchStatusText;

pub struct OnlineStatusPlugin;

impl Plugin for OnlineStatusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnlineMatchPresentation>()
            .add_message::<AuthoritativePresentationSnapshot>()
            .add_message::<OnlineRoomStateChanged>()
            .add_systems(Startup, spawn_online_match_status)
            .add_systems(
                Update,
                (
                    observe_authoritative_presentation,
                    tick_online_clock_estimate,
                    sync_online_match_status,
                )
                    .chain(),
            );
    }
}

fn spawn_online_match_status(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgb(0.94, 0.95, 1.0)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            bottom: px(44),
            width: percent(SIDE_REGION_PERCENT),
            padding: UiRect::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.025, 0.035, 0.06, 0.88)),
        GlobalZIndex(70),
        Visibility::Hidden,
        OnlineMatchStatusText,
    ));
}

fn observe_authoritative_presentation(
    mut snapshots: MessageReader<AuthoritativePresentationSnapshot>,
    mut room_states: MessageReader<OnlineRoomStateChanged>,
    mut presentation: ResMut<OnlineMatchPresentation>,
) {
    for snapshot in snapshots.read() {
        presentation.observe(snapshot.clone());
    }
    for room_state in room_states.read() {
        presentation.room_state = room_state.0;
    }
}

#[allow(clippy::needless_pass_by_value)]
fn tick_online_clock_estimate(
    time: Res<Time<Real>>,
    flow: Res<ClientFlow>,
    mut presentation: ResMut<OnlineMatchPresentation>,
) {
    if *flow == ClientFlow::OnlinePlaying {
        presentation.tick(time.delta_secs_f64());
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_online_match_status(
    flow: Res<ClientFlow>,
    presentation: Res<OnlineMatchPresentation>,
    connection: Res<OnlineConnection>,
    outbox: Res<OnlineIntentOutbox>,
    lobby: Res<OnlineLobby>,
    mut text: Query<(&mut Text, &mut Visibility), With<OnlineMatchStatusText>>,
) {
    let own_seat = lobby
        .seat
        .as_ref()
        .map_or("Unknown".to_owned(), |seat| format!("{:?}", seat.seat));
    let phase = match &presentation.phase {
        TurnPhase::Command => "command".to_owned(),
        TurnPhase::ResolvingChoices { queue } => {
            format!("mandatory choice - {} remaining", queue.len())
        }
    };
    let room = room_state_label(presentation.room_state);
    let transport = connection_phase_label(&connection.phase);
    let clocks = presentation.displayed.map_or_else(
        || "Clocks: untimed".to_owned(),
        |clocks| {
            format!(
                "Clock estimate - North {} - South {}{}",
                format_estimated_clock(clocks.north_millis),
                format_estimated_clock(clocks.south_millis),
                if !presentation.terminal
                    && (clocks.north_millis <= 0.0 || clocks.south_millis <= 0.0)
                {
                    " - awaiting server outcome"
                } else {
                    ""
                }
            )
        },
    );
    let pending = if outbox.locked {
        "command pending"
    } else {
        "no command pending"
    };
    for (mut text, mut visibility) in &mut text {
        *visibility = if *flow == ClientFlow::OnlinePlaying {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        text.0 = format!(
            "You: {own_seat} - Active: {:?} - Phase: {phase}\n{clocks}\nRoom: {room} - Your connection: {transport} - {pending}",
            presentation.active_player,
        );
    }
}

const fn room_state_label(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::WaitingForOpponent => "waiting for opponent",
        ConnectionState::WaitingForReady => "waiting for ready",
        ConnectionState::Connected => "both seats connected",
        ConnectionState::OpponentDisconnected => "opponent disconnected; clock continues",
        ConnectionState::Finished => "finished",
    }
}

fn connection_phase_label(phase: &ConnectionPhase) -> &'static str {
    match phase {
        ConnectionPhase::Connecting => "connecting",
        ConnectionPhase::Connected => "connected",
        ConnectionPhase::Retrying { .. } => "retrying",
        ConnectionPhase::Offline => "offline",
        ConnectionPhase::Rejected => "rejected",
        ConnectionPhase::Terminal => "terminal",
    }
}

fn format_estimated_clock(millis: f64) -> String {
    let duration = Duration::from_secs_f64(millis.max(0.0) / 1_000.0);
    let seconds = u64::try_from(duration.as_millis().div_ceil(1_000)).unwrap_or(u64::MAX);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(clocks: ClockState, terminal: bool) -> AuthoritativePresentationSnapshot {
        AuthoritativePresentationSnapshot {
            clocks: Some(clocks),
            active_player: Player::North,
            phase: TurnPhase::Command,
            terminal,
            room_state: if terminal {
                ConnectionState::Finished
            } else {
                ConnectionState::Connected
            },
        }
    }

    #[test]
    fn estimate_reaches_zero_without_deciding_terminal_state() {
        let mut presentation = OnlineMatchPresentation::default();
        presentation.observe(snapshot(
            ClockState {
                north_millis: 500,
                south_millis: 5_000,
                increment_millis: 0,
            },
            false,
        ));
        presentation.tick(1.0);
        assert!(presentation.displayed.unwrap().north_millis.abs() < f64::EPSILON);
        assert!(!presentation.terminal);
    }

    #[test]
    fn opponent_disconnect_does_not_pause_active_estimate() {
        let mut presentation = OnlineMatchPresentation::default();
        let mut update = snapshot(
            ClockState {
                north_millis: 10_000,
                south_millis: 10_000,
                increment_millis: 0,
            },
            false,
        );
        update.room_state = ConnectionState::OpponentDisconnected;
        presentation.observe(update);
        presentation.tick(1.0);
        assert!((presentation.displayed.unwrap().north_millis - 9_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ordinary_drift_eases_but_terminal_truth_replaces_immediately() {
        let mut presentation = OnlineMatchPresentation::default();
        presentation.observe(snapshot(
            ClockState {
                north_millis: 10_000,
                south_millis: 10_000,
                increment_millis: 0,
            },
            false,
        ));
        presentation.observe(snapshot(
            ClockState {
                north_millis: 5_000,
                south_millis: 10_000,
                increment_millis: 0,
            },
            false,
        ));
        presentation.tick(0.1);
        let eased = presentation.displayed.unwrap().north_millis;
        assert!(eased > 5_000.0 && eased < 10_000.0);

        presentation.observe(snapshot(
            ClockState {
                north_millis: 0,
                south_millis: 10_000,
                increment_millis: 0,
            },
            true,
        ));
        assert!(presentation.displayed.unwrap().north_millis.abs() < f64::EPSILON);
    }

    #[test]
    fn phases_and_connection_effects_have_distinct_labels() {
        assert_eq!(
            room_state_label(ConnectionState::OpponentDisconnected),
            "opponent disconnected; clock continues"
        );
        assert_eq!(
            connection_phase_label(&ConnectionPhase::Retrying {
                attempt: 2,
                delay: std::time::Duration::from_secs(1)
            }),
            "retrying"
        );
    }
}
