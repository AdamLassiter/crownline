use bevy::prelude::*;
use crownline_core::{
    Action,
    scenario::Player,
    state::{MatchOutcome, MatchState},
};
use crownline_protocol::{
    ActionRequest, ClientMessage, DrawCommand, MutationContext, PROTOCOL_VERSION, RematchCommand,
    RematchState,
};
use uuid::Uuid;

use crate::{
    config::unmodified_just_pressed,
    lifecycle::ClientFlow,
    online_connection::{
        OnlineControlKind, OnlineControlOutbox, OnlineControlResolved, OnlineIntentOutbox,
        OnlineRematchStateChanged,
    },
    online_lobby::OnlineLobby,
    rendering::DisplayedGame,
};

#[derive(Debug, Resource, Default)]
struct OnlineLifecycleState {
    confirm_resign: bool,
    rematch_state: Option<RematchState>,
    requested_rematch_by_self: bool,
    status: String,
}

#[derive(Component)]
struct OnlineLifecycleText;

pub struct OnlineLifecyclePlugin;

impl Plugin for OnlineLifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnlineLifecycleState>()
            .add_systems(Startup, spawn_online_lifecycle_controls)
            .add_systems(
                Update,
                (
                    observe_lifecycle_results,
                    handle_online_lifecycle_input,
                    sync_online_lifecycle_ui,
                )
                    .chain(),
            );
    }
}

fn spawn_online_lifecycle_controls(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.94, 0.95, 1.0)),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            bottom: px(10),
            width: percent(38),
            padding: UiRect::all(px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.025, 0.035, 0.06, 0.9)),
        GlobalZIndex(72),
        Visibility::Hidden,
        OnlineLifecycleText,
    ));
}

fn observe_lifecycle_results(
    mut resolutions: MessageReader<OnlineControlResolved>,
    mut rematches: MessageReader<OnlineRematchStateChanged>,
    mut lifecycle: ResMut<OnlineLifecycleState>,
) {
    for resolution in resolutions.read() {
        lifecycle.confirm_resign = false;
        lifecycle.status = if resolution.accepted {
            format!("{:?} control accepted by the server.", resolution.kind)
        } else {
            format!("{:?} control rejected by the server.", resolution.kind)
        };
    }
    for rematch in rematches.read() {
        lifecycle.rematch_state = rematch.0;
        match rematch.0 {
            Some(RematchState::Requested) => {
                "A rematch has been requested.".clone_into(&mut lifecycle.status);
            }
            Some(RematchState::Accepted) => {
                "Both seats accepted. Starting a fresh rematch…".clone_into(&mut lifecycle.status);
            }
            Some(RematchState::Declined) => {
                lifecycle.requested_rematch_by_self = false;
                "The rematch was declined.".clone_into(&mut lifecycle.status);
            }
            None => {
                lifecycle.requested_rematch_by_self = false;
            }
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_value
)]
fn handle_online_lifecycle_input(
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<ClientFlow>,
    game: Res<DisplayedGame>,
    lobby: Res<OnlineLobby>,
    board_outbox: Res<OnlineIntentOutbox>,
    mut control_outbox: ResMut<OnlineControlOutbox>,
    mut lifecycle: ResMut<OnlineLifecycleState>,
) {
    if *flow != ClientFlow::OnlinePlaying {
        lifecycle.confirm_resign = false;
        return;
    }
    let Some(seat) = lobby.seat.as_ref() else {
        return;
    };
    let controls_locked = board_outbox.locked || control_outbox.locked;
    let terminal = game.state.outcome.is_some();

    if terminal {
        lifecycle.confirm_resign = false;
        if controls_locked {
            return;
        }
        if keys.just_pressed(KeyCode::KeyL) {
            submit_control(
                &mut control_outbox,
                leave_message(seat.match_id, game.state.revision),
                OnlineControlKind::Leave,
            );
            "Leaving this finished room; its durable result remains on the server."
                .clone_into(&mut lifecycle.status);
        } else if keys.just_pressed(KeyCode::KeyN)
            && lifecycle.rematch_state == Some(RematchState::Requested)
        {
            submit_control(
                &mut control_outbox,
                rematch_message(seat.match_id, game.state.revision, RematchCommand::Decline),
                OnlineControlKind::Rematch,
            );
            lifecycle.requested_rematch_by_self = false;
        } else if keys.just_pressed(KeyCode::KeyR)
            && lifecycle.rematch_state != Some(RematchState::Accepted)
        {
            let accepting_other = lifecycle.rematch_state == Some(RematchState::Requested)
                && !lifecycle.requested_rematch_by_self;
            submit_control(
                &mut control_outbox,
                rematch_message(
                    seat.match_id,
                    game.state.revision,
                    if accepting_other {
                        RematchCommand::Accept
                    } else {
                        RematchCommand::Request
                    },
                ),
                OnlineControlKind::Rematch,
            );
            lifecycle.requested_rematch_by_self = true;
        }
        return;
    }

    if lifecycle.confirm_resign {
        if keys.just_pressed(KeyCode::Escape) {
            lifecycle.confirm_resign = false;
            "Resignation cancelled.".clone_into(&mut lifecycle.status);
        } else if keys.just_pressed(KeyCode::Enter) && !controls_locked {
            if resignation_available(&game.state, seat.seat) {
                submit_control(
                    &mut control_outbox,
                    resign_message(seat.match_id, game.state.revision, seat.seat),
                    OnlineControlKind::Resign,
                );
                "Resignation submitted; awaiting the authoritative outcome."
                    .clone_into(&mut lifecycle.status);
            } else {
                lifecycle.confirm_resign = false;
                "Resignation cancelled because the authoritative turn changed."
                    .clone_into(&mut lifecycle.status);
            }
        }
        return;
    }
    if controls_locked {
        return;
    }
    if unmodified_just_pressed(&keys, KeyCode::KeyQ)
        && resignation_available(&game.state, seat.seat)
    {
        lifecycle.confirm_resign = true;
        "Confirm resignation with Enter, or cancel with Escape.".clone_into(&mut lifecycle.status);
        return;
    }
    match game.state.outstanding_draw_offer {
        None if unmodified_just_pressed(&keys, KeyCode::KeyD)
            && game.state.active_player == seat.seat =>
        {
            submit_control(
                &mut control_outbox,
                draw_message(seat.match_id, game.state.revision, DrawCommand::Offer),
                OnlineControlKind::Draw,
            );
        }
        Some(offering)
            if offering != seat.seat
                && (keys.just_pressed(KeyCode::KeyY) || keys.just_pressed(KeyCode::KeyN)) =>
        {
            submit_control(
                &mut control_outbox,
                draw_message(
                    seat.match_id,
                    game.state.revision,
                    if keys.just_pressed(KeyCode::KeyY) {
                        DrawCommand::Accept
                    } else {
                        DrawCommand::Reject
                    },
                ),
                OnlineControlKind::Draw,
            );
        }
        _ => {}
    }
}

fn submit_control(
    outbox: &mut OnlineControlOutbox,
    message: ClientMessage,
    kind: OnlineControlKind,
) {
    if outbox.locked {
        return;
    }
    outbox.locked = true;
    outbox.message = Some((message, kind));
}

fn mutation_context(match_id: Uuid, revision: u64) -> MutationContext {
    MutationContext {
        match_id,
        expected_revision: revision,
        idempotency_key: Uuid::new_v4(),
    }
}

fn draw_message(match_id: Uuid, revision: u64, command: DrawCommand) -> ClientMessage {
    ClientMessage::Draw {
        protocol_version: PROTOCOL_VERSION,
        context: mutation_context(match_id, revision),
        command,
    }
}

fn resign_message(match_id: Uuid, revision: u64, seat: Player) -> ClientMessage {
    ClientMessage::Action {
        protocol_version: PROTOCOL_VERSION,
        request: ActionRequest {
            context: mutation_context(match_id, revision),
            action: Action::Resign { player: seat },
        },
    }
}

fn rematch_message(match_id: Uuid, revision: u64, command: RematchCommand) -> ClientMessage {
    ClientMessage::Rematch {
        protocol_version: PROTOCOL_VERSION,
        context: mutation_context(match_id, revision),
        command,
    }
}

fn leave_message(match_id: Uuid, revision: u64) -> ClientMessage {
    ClientMessage::Leave {
        protocol_version: PROTOCOL_VERSION,
        context: mutation_context(match_id, revision),
    }
}

fn resignation_available(state: &MatchState, seat: Player) -> bool {
    state.outcome.is_none() && state.active_player == seat
}

#[allow(clippy::needless_pass_by_value)]
fn sync_online_lifecycle_ui(
    flow: Res<ClientFlow>,
    game: Res<DisplayedGame>,
    lobby: Res<OnlineLobby>,
    board_outbox: Res<OnlineIntentOutbox>,
    control_outbox: Res<OnlineControlOutbox>,
    lifecycle: Res<OnlineLifecycleState>,
    mut text: Query<(&mut Text, &mut Visibility), With<OnlineLifecycleText>>,
) {
    let body = lobby.seat.as_ref().map_or_else(String::new, |seat| {
        if let Some(outcome) = game.state.outcome {
            terminal_controls(
                outcome,
                lifecycle.rematch_state,
                lifecycle.requested_rematch_by_self,
                board_outbox.locked || control_outbox.locked,
                &lifecycle.status,
            )
        } else {
            active_controls(
                &game.state,
                seat.seat,
                lifecycle.confirm_resign,
                board_outbox.locked || control_outbox.locked,
                &lifecycle.status,
            )
        }
    });
    for (mut text, mut visibility) in &mut text {
        *visibility = if *flow == ClientFlow::OnlinePlaying {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        text.0.clone_from(&body);
    }
}

fn active_controls(
    state: &MatchState,
    seat: Player,
    confirm_resign: bool,
    locked: bool,
    status: &str,
) -> String {
    let draw = match state.outstanding_draw_offer {
        None if state.active_player == seat => "D offer draw",
        None => "Draw offer unavailable until your turn",
        Some(offering) if offering == seat => "Draw offered · awaiting opponent",
        Some(_) => "Opponent offered draw · Y accept · N decline",
    };
    let resign = if state.outcome.is_some() {
        "Resign disabled: match finished"
    } else if confirm_resign {
        "CONFIRM RESIGNATION · Enter confirm · Esc cancel"
    } else if state.active_player == seat {
        "Q resign (confirmation required)"
    } else {
        "Resign unavailable until your turn"
    };
    format!(
        "MATCH CONTROLS{}\n{draw}\n{resign}\n{status}",
        if locked { " · PENDING" } else { "" }
    )
}

fn terminal_controls(
    outcome: MatchOutcome,
    rematch: Option<RematchState>,
    requested_by_self: bool,
    locked: bool,
    status: &str,
) -> String {
    let winner = outcome
        .winner
        .map_or_else(|| "Draw".to_owned(), |winner| format!("{winner:?} wins"));
    let rematch = match rematch {
        None | Some(RematchState::Declined) => "R request rematch",
        Some(RematchState::Requested) if requested_by_self => {
            "Rematch requested · waiting for opponent · N decline"
        }
        Some(RematchState::Requested) => "Opponent requests rematch · R accept · N decline",
        Some(RematchState::Accepted) => "Rematch accepted · reconnecting to fresh match",
    };
    format!(
        "MATCH ENDED · {:?} · {winner}{}\n{rematch}\nL leave finished room\n{status}",
        outcome.reason,
        if locked { " · CONTROL PENDING" } else { "" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crownline_core::state::OutcomeReason;

    fn game_state() -> MatchState {
        let scenario = ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        MatchState::from_scenario(&scenario).unwrap()
    }

    #[test]
    fn resignation_is_confirmable_only_for_active_non_terminal_seat() {
        let mut state = game_state();
        assert!(resignation_available(&state, state.active_player));
        assert!(!resignation_available(
            &state,
            state.active_player.opponent()
        ));
        state.outcome = Some(MatchOutcome {
            winner: Some(Player::North),
            reason: OutcomeReason::Resignation,
        });
        assert!(!resignation_available(&state, state.active_player));
    }

    #[test]
    fn draw_ui_shows_exactly_one_offer_and_opponent_choices() {
        let mut state = game_state();
        let seat = state.active_player.opponent();
        state.outstanding_draw_offer = Some(state.active_player);
        let text = active_controls(&state, seat, false, false, "");
        assert!(text.contains("Opponent offered draw · Y accept · N decline"));
        assert_eq!(text.matches("offered draw").count(), 1);
    }

    #[test]
    fn terminal_summary_names_exact_reason_winner_rematch_and_leave() {
        let text = terminal_controls(
            MatchOutcome {
                winner: Some(Player::South),
                reason: OutcomeReason::Timeout,
            },
            Some(RematchState::Requested),
            false,
            false,
            "",
        );
        assert!(text.contains("Timeout · South wins"));
        assert!(text.contains("R accept · N decline"));
        assert!(text.contains("L leave finished room"));
    }

    #[test]
    fn control_messages_use_separate_protocol_envelopes_and_unique_keys() {
        let match_id = Uuid::new_v4();
        let ClientMessage::Draw { context: draw, .. } =
            draw_message(match_id, 4, DrawCommand::Offer)
        else {
            panic!("draw must use draw envelope");
        };
        let ClientMessage::Action {
            request: resign, ..
        } = resign_message(match_id, 4, Player::North)
        else {
            panic!("resign must use canonical action envelope");
        };
        let ClientMessage::Rematch {
            context: rematch, ..
        } = rematch_message(match_id, 4, RematchCommand::Request)
        else {
            panic!("rematch must use rematch envelope");
        };
        assert_eq!(draw.expected_revision, 4);
        assert_eq!(resign.context.expected_revision, 4);
        assert_ne!(draw.idempotency_key, resign.context.idempotency_key);
        assert_ne!(draw.idempotency_key, rematch.idempotency_key);
    }
}
