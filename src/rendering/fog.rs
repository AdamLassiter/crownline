use bevy::prelude::*;
use crownline_core::{PlayerView, project_player_view, scenario::Player};

use crate::lifecycle::ClientFlow;

use super::DisplayedGame;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FogPresentationPhase {
    Disabled,
    AwaitingHandoff {
        player: Player,
    },
    Presenting {
        player: Player,
        scenario_id: String,
        revision: u64,
    },
}

#[derive(Resource)]
pub(crate) struct FogPresentation {
    phase: FogPresentationPhase,
    view: Option<PlayerView>,
    confirmed_this_frame: bool,
}

impl Default for FogPresentation {
    fn default() -> Self {
        Self {
            phase: FogPresentationPhase::Disabled,
            view: None,
            confirmed_this_frame: false,
        }
    }
}

impl FogPresentation {
    pub(crate) fn phase(&self) -> &FogPresentationPhase {
        &self.phase
    }

    pub(crate) fn view(&self) -> Option<&PlayerView> {
        self.view.as_ref()
    }

    pub(crate) const fn confirmed_this_frame(&self) -> bool {
        self.confirmed_this_frame
    }

    pub(crate) fn blocks_local_input(&self, game: &DisplayedGame) -> bool {
        game.scenario.rules.fog.is_some()
            && !matches!(
                self.phase,
                FogPresentationPhase::Presenting { player, revision, .. }
                    if player == game.state.active_player && revision == game.state.revision
            )
    }

    pub(crate) fn require_handoff(&mut self, game: &DisplayedGame) {
        if game.scenario.rules.fog.is_some() {
            self.phase = FogPresentationPhase::AwaitingHandoff {
                player: game.state.active_player,
            };
            self.view = None;
        } else {
            self.phase = FogPresentationPhase::Disabled;
            self.view = None;
        }
    }

    pub(crate) fn confirm(&mut self, game: &DisplayedGame) -> bool {
        let FogPresentationPhase::AwaitingHandoff { player } = self.phase else {
            return false;
        };
        if player != game.state.active_player {
            self.require_handoff(game);
            return false;
        }
        let Ok(view) = project_player_view(&game.scenario, &game.state, player) else {
            self.require_handoff(game);
            return false;
        };
        self.view = Some(view);
        self.confirmed_this_frame = true;
        self.phase = FogPresentationPhase::Presenting {
            player,
            scenario_id: game.scenario.id.clone(),
            revision: game.state.revision,
        };
        true
    }

    fn reconcile(&mut self, game: &DisplayedGame) {
        if game.scenario.rules.fog.is_none() {
            self.phase = FogPresentationPhase::Disabled;
            self.view = None;
            return;
        }
        if game.state.outcome.is_some() {
            let player = game.state.active_player;
            if let Ok(view) = project_player_view(&game.scenario, &game.state, player) {
                self.view = Some(view);
                self.phase = FogPresentationPhase::Presenting {
                    player,
                    scenario_id: game.scenario.id.clone(),
                    revision: game.state.revision,
                };
            } else {
                self.require_handoff(game);
            }
            return;
        }
        let FogPresentationPhase::Presenting {
            player,
            scenario_id,
            revision,
        } = &self.phase
        else {
            if matches!(self.phase, FogPresentationPhase::Disabled) {
                self.require_handoff(game);
            }
            return;
        };
        let continuous_same_seat = *player == game.state.active_player
            && *scenario_id == game.scenario.id
            && game.state.revision >= *revision
            && game.state.revision <= revision.saturating_add(1);
        if !continuous_same_seat {
            self.require_handoff(game);
            return;
        }
        if game.state.revision != *revision {
            let player = *player;
            match project_player_view(&game.scenario, &game.state, player) {
                Ok(view) => {
                    self.view = Some(view);
                    self.phase = FogPresentationPhase::Presenting {
                        player,
                        scenario_id: game.scenario.id.clone(),
                        revision: game.state.revision,
                    };
                }
                Err(_) => self.require_handoff(game),
            }
        }
    }
}

#[derive(Component)]
pub(crate) struct FogHandoffCurtain;

#[derive(Component)]
pub(super) struct FogHandoffText;

pub(super) fn spawn_handoff_curtain(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.018, 0.022, 0.032)),
            GlobalZIndex(120),
            Visibility::Hidden,
            FogHandoffCurtain,
            Name::new("fog hot-seat handoff curtain"),
        ))
        .with_children(|curtain| {
            curtain.spawn((
                Text::new("PRIVATE HANDOFF"),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(Color::srgb(0.94, 0.95, 1.0)),
                TextLayout::justify(Justify::Center),
                FogHandoffText,
            ));
        });
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn prepare_fog_presentation(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    flow: Option<Res<ClientFlow>>,
    game: Res<DisplayedGame>,
    mut fog: ResMut<FogPresentation>,
) {
    fog.confirmed_this_frame = false;
    fog.reconcile(&game);
    let local_playing = flow
        .as_deref()
        .is_none_or(|flow| *flow == ClientFlow::Playing);
    if local_playing
        && keys
            .as_deref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Enter))
    {
        fog.confirm(&game);
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn secure_fog_after_update(game: Res<DisplayedGame>, mut fog: ResMut<FogPresentation>) {
    fog.reconcile(&game);
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn sync_handoff_curtain(
    fog: Res<FogPresentation>,
    mut curtain: Query<&mut Visibility, With<FogHandoffCurtain>>,
    mut text: Query<&mut Text, With<FogHandoffText>>,
) {
    let awaiting = match fog.phase() {
        FogPresentationPhase::AwaitingHandoff { player } => Some(*player),
        FogPresentationPhase::Disabled | FogPresentationPhase::Presenting { .. } => None,
    };
    for mut visibility in &mut curtain {
        *visibility = if awaiting.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if let Some(player) = awaiting {
        for mut text in &mut text {
            text.0 = format!(
                "PRIVATE HANDOFF\nPass control to {player:?}.\nPress Enter when ready.\nBoard and clocks remain hidden and paused."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crownline_core::{
        FOG_RULES_SCHEMA_VERSION, FogRules, MatchState, scenario::ScenarioDefinition,
    };

    fn fog_game() -> DisplayedGame {
        let mut scenario: ScenarioDefinition =
            ron::from_str(include_str!("../../assets/scenarios/introductory.ron")).unwrap();
        scenario.rules.fog = Some(FogRules {
            schema_version: FOG_RULES_SCHEMA_VERSION,
            vision_radius: 3,
        });
        let state = MatchState::from_scenario(&scenario).unwrap();
        DisplayedGame { scenario, state }
    }

    #[test]
    fn handoff_never_retains_the_outgoing_or_constructs_the_incoming_view_early() {
        let mut game = fog_game();
        let mut fog = FogPresentation::default();
        fog.reconcile(&game);
        assert!(matches!(
            fog.phase(),
            FogPresentationPhase::AwaitingHandoff {
                player: Player::South
            }
        ));
        assert!(fog.view().is_none());
        assert!(fog.blocks_local_input(&game));

        assert!(fog.confirm(&game));
        assert_eq!(fog.view().unwrap().seat, Player::South);
        assert!(!fog.blocks_local_input(&game));

        game.state.active_player = Player::North;
        game.state.revision += 1;
        fog.reconcile(&game);
        assert!(matches!(
            fog.phase(),
            FogPresentationPhase::AwaitingHandoff {
                player: Player::North
            }
        ));
        assert!(fog.view().is_none());
        assert!(fog.blocks_local_input(&game));
    }

    #[test]
    fn same_seat_choice_revision_reprojects_without_a_handoff() {
        let mut game = fog_game();
        let mut fog = FogPresentation::default();
        fog.reconcile(&game);
        fog.confirm(&game);
        game.state.revision += 1;
        fog.reconcile(&game);
        assert!(matches!(
            fog.phase(),
            FogPresentationPhase::Presenting {
                player: Player::South,
                revision: 1,
                ..
            }
        ));
        assert_eq!(fog.view().unwrap().revision, 1);
    }
}
