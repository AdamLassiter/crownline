use std::fmt::Write as _;

use bevy::prelude::*;
use crownline_core::{
    governance_report, is_in_check,
    rules::GovernanceBlocker,
    scenario::ScenarioDefinition,
    state::{ClockState, MatchState, PieceId, TurnPhase},
};

use crate::{
    help::{HelpLink, HelpSection},
    lifecycle::ClientFlow,
    local_persistence::LocalPersistenceStatus,
    playtest::PlaytestStatus,
    rendering::{
        DisplayedGame, FogPresentation, LocalTransitionNoticeLog, OverlaySelection, PointerCapture,
    },
    ui_layout::{BOTTOM_REGION_PERCENT, SIDE_REGION_PERCENT},
};

const HISTORY_LIMIT: usize = 12;
const LOW_CLOCK_MILLIS: u64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub(crate) enum PanelKind {
    Match,
    Settlements,
}

#[derive(Resource, Default)]
struct PanelState {
    match_collapsed: bool,
    settlements_collapsed: bool,
}

#[derive(Resource, Default)]
struct PanelContentCache {
    revision: Option<u64>,
    selected: Option<PieceId>,
    history_len: usize,
    clocks: Option<ClockState>,
    online: bool,
    persistence_message: String,
    playtest_message: String,
    projection: Option<String>,
}

impl PanelState {
    const fn collapsed(&self, kind: PanelKind) -> bool {
        match kind {
            PanelKind::Match => self.match_collapsed,
            PanelKind::Settlements => self.settlements_collapsed,
        }
    }

    fn toggle(&mut self, kind: PanelKind) {
        match kind {
            PanelKind::Match => self.match_collapsed = !self.match_collapsed,
            PanelKind::Settlements => self.settlements_collapsed = !self.settlements_collapsed,
        }
    }
}

#[derive(Component)]
struct InformationPanel;

#[derive(Component)]
pub(crate) struct PanelSurface;

#[derive(Component)]
struct PanelToggle(PanelKind);

#[derive(Component)]
pub(crate) struct PanelBody(pub(crate) PanelKind);

#[derive(Component)]
struct MatchPanelText;

#[derive(Component)]
struct SettlementPanelText;

#[derive(Component)]
struct HistoryPanelText;

#[derive(Component)]
struct ToggleLabel(PanelKind);

pub struct InformationPanelsPlugin;

impl Plugin for InformationPanelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelState>()
            .init_resource::<PanelContentCache>()
            .add_systems(Startup, spawn_information_panels)
            .add_systems(
                Update,
                (
                    handle_panel_toggles,
                    apply_panel_visibility,
                    update_panel_text,
                    update_pointer_capture,
                )
                    .chain(),
            );
    }
}

fn spawn_information_panels(mut commands: Commands) {
    spawn_panel(
        &mut commands,
        PanelKind::Match,
        UiRect {
            left: px(0),
            right: Val::Auto,
            top: px(0),
            bottom: percent(BOTTOM_REGION_PERCENT),
        },
    );
    spawn_panel(
        &mut commands,
        PanelKind::Settlements,
        UiRect {
            left: Val::Auto,
            right: px(0),
            top: px(0),
            bottom: percent(BOTTOM_REGION_PERCENT),
        },
    );
}

fn spawn_panel(commands: &mut Commands, kind: PanelKind, inset: UiRect) {
    let title = match kind {
        PanelKind::Match => "MATCH - [I] collapse all",
        PanelKind::Settlements => "SETTLEMENTS",
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: inset.left,
                right: inset.right,
                top: inset.top,
                bottom: inset.bottom,
                width: percent(SIDE_REGION_PERCENT),
                min_width: px(0),
                max_width: percent(SIDE_REGION_PERCENT),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                padding: UiRect::all(px(8)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.035, 0.045, 0.07, 0.92)),
            BorderColor::all(Color::srgb(0.34, 0.4, 0.52)),
            Interaction::default(),
            InformationPanel,
            kind,
            PanelSurface,
        ))
        .with_children(|panel| {
            panel.spawn((
                Button,
                Node {
                    width: percent(100),
                    min_height: px(30),
                    padding: UiRect::axes(px(7), px(4)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.12, 0.16, 0.24)),
                PanelToggle(kind),
                PanelSurface,
                children![(
                    Text::new(title),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.91, 0.72)),
                    ToggleLabel(kind),
                )],
            ));
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Interaction::default(),
                    PanelBody(kind),
                    PanelSurface,
                ))
                .with_children(|body| match kind {
                    PanelKind::Match => {
                        body.spawn(panel_text("Loading match…", MatchPanelText));
                        body.spawn(panel_text(
                            "RECENT ACTIONS\nNo actions yet.",
                            HistoryPanelText,
                        ));
                        body.spawn(help_link(
                            "Help: commands, clocks, outcomes",
                            HelpSection::Match,
                        ));
                    }
                    PanelKind::Settlements => {
                        body.spawn(panel_text("Loading settlements…", SettlementPanelText));
                        body.spawn(help_link(
                            "Help: settlements, promotion, board legend",
                            HelpSection::Realm,
                        ));
                    }
                });
        });
}

fn help_link(label: &str, section: HelpSection) -> impl Bundle {
    (
        Button,
        Node {
            width: percent(100),
            min_height: px(28),
            padding: UiRect::axes(px(6), px(4)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.1, 0.2, 0.25)),
        HelpLink(section),
        PanelSurface,
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::srgb(0.55, 0.9, 0.94)),
        )],
    )
}

fn panel_text(marker_text: &str, marker: impl Component) -> impl Bundle {
    (
        Text::new(marker_text),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.92, 0.96)),
        TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
        marker,
    )
}

#[allow(clippy::needless_pass_by_value)]
fn handle_panel_toggles(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<PanelState>,
    toggles: Query<(&Interaction, &PanelToggle), Changed<Interaction>>,
) {
    if keys.just_pressed(KeyCode::KeyI) {
        let collapse = !(state.match_collapsed && state.settlements_collapsed);
        state.match_collapsed = collapse;
        state.settlements_collapsed = collapse;
    }
    for (interaction, toggle) in &toggles {
        if *interaction == Interaction::Pressed {
            state.toggle(toggle.0);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn apply_panel_visibility(
    state: Res<PanelState>,
    mut bodies: Query<(&PanelBody, &mut Node)>,
    mut labels: Query<(&ToggleLabel, &mut Text)>,
) {
    for (body, mut node) in &mut bodies {
        node.display = if state.collapsed(body.0) {
            Display::None
        } else {
            Display::Flex
        };
    }
    for (label, mut text) in &mut labels {
        let name = match label.0 {
            PanelKind::Match => "MATCH",
            PanelKind::Settlements => "SETTLEMENTS",
        };
        text.0 = format!(
            "{name} - {}",
            if state.collapsed(label.0) {
                "expand"
            } else {
                "collapse"
            }
        );
    }
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn update_panel_text(
    game: Res<DisplayedGame>,
    selection: Res<OverlaySelection>,
    history: Res<LocalTransitionNoticeLog>,
    persistence: Option<Res<LocalPersistenceStatus>>,
    playtest: Option<Res<PlaytestStatus>>,
    flow: Option<Res<ClientFlow>>,
    mut cache: ResMut<PanelContentCache>,
    mut texts: Query<(
        &mut Text,
        Option<&MatchPanelText>,
        Option<&SettlementPanelText>,
        Option<&HistoryPanelText>,
    )>,
    fog: Res<FogPresentation>,
) {
    let persistence_message = persistence
        .as_deref()
        .map_or("", |status| status.message.as_str());
    let playtest_message = playtest
        .as_deref()
        .map_or("", |status| status.message.as_str());
    let online = flow
        .as_deref()
        .is_some_and(|flow| *flow == ClientFlow::OnlinePlaying);
    let projection = fog.view().map(|view| view.projection_hash.clone());
    if cache.revision == Some(game.state.revision)
        && cache.selected == selection.piece
        && cache.history_len == history.entries.len()
        && cache.clocks == game.state.clocks
        && cache.online == online
        && cache.persistence_message == persistence_message
        && cache.playtest_message == playtest_message
        && cache.projection == projection
    {
        return;
    }
    let mut match_text = fog.view().map_or_else(
        || {
            if game.scenario.rules.fog.is_some() {
                "PRIVATE HANDOFF\nBoard and clocks are hidden and paused.".to_owned()
            } else {
                match_panel_text_with_clock_context(
                    &game.scenario,
                    &game.state,
                    selection.piece,
                    online,
                )
            }
        },
        fog_match_panel_text,
    );
    if let Some(status) = persistence.as_deref() {
        let _ = write!(
            match_text,
            "\nSave slot {}: {}",
            status.slot, status.message
        );
    }
    if let Some(status) = playtest.as_deref() {
        let _ = write!(match_text, "\nPlaytest: {}", status.message);
    }
    let settlement_text = fog.view().map_or_else(
        || {
            if game.scenario.rules.fog.is_some() {
                "SETTLEMENTS\nHidden during handoff.".to_owned()
            } else {
                settlement_panel_text(&game.scenario, &game.state)
            }
        },
        fog_settlement_panel_text,
    );
    let history_text = if game.scenario.rules.fog.is_some() {
        "EVENTS\nPrivate details are omitted in fog-of-war matches.".to_owned()
    } else {
        bounded_history(&history.entries)
    };
    for (mut text, match_marker, settlement_marker, history_marker) in &mut texts {
        if match_marker.is_some() {
            text.0.clone_from(&match_text);
        } else if settlement_marker.is_some() {
            text.0.clone_from(&settlement_text);
        } else if history_marker.is_some() {
            text.0.clone_from(&history_text);
        }
    }
    cache.revision = Some(game.state.revision);
    cache.selected = selection.piece;
    cache.history_len = history.entries.len();
    cache.clocks = game.state.clocks;
    cache.online = online;
    persistence_message.clone_into(&mut cache.persistence_message);
    playtest_message.clone_into(&mut cache.playtest_message);
    cache.projection = projection;
}

fn fog_match_panel_text(view: &crownline_core::PlayerView) -> String {
    let mut lines = vec![format!(
        "TURN {} - {:?} to act\nViewing: {:?}",
        view.turn_number, view.active_player, view.seat
    )];
    if view.checked_players.contains(&view.seat) {
        lines.push("!!! CHECK - your King is threatened".to_owned());
    }
    lines.push(match &view.phase {
        crownline_core::ViewTurnPhase::Command => "Phase: Command - Move or Hold".to_owned(),
        crownline_core::ViewTurnPhase::OwnChoices { queue } => {
            format!("!!! MANDATORY CHOICE - {} remaining", queue.len())
        }
        crownline_core::ViewTurnPhase::PrivateChoice { .. } => {
            "Another player has a private mandatory choice.".to_owned()
        }
    });
    if let Some(clocks) = view.clocks {
        lines.push(format!(
            "Clocks: North {} - South {}",
            format_clock(clocks.north_millis),
            format_clock(clocks.south_millis)
        ));
    }
    lines.join("\n")
}

fn fog_settlement_panel_text(view: &crownline_core::PlayerView) -> String {
    let mut lines = vec!["SETTLEMENTS - known information".to_owned()];
    for settlement in view.settlements.values() {
        if let Some(dynamic) = &settlement.dynamic {
            lines.push(format!(
                "{} at {:?}: owner {:?}, production {}",
                settlement.id, settlement.at, dynamic.owner, dynamic.production_progress
            ));
        } else {
            lines.push(format!(
                "{} at {:?}: last-known site",
                settlement.id, settlement.at
            ));
        }
    }
    lines.join("\n")
}

#[allow(clippy::needless_pass_by_value)]
fn update_pointer_capture(
    surfaces: Query<&Interaction, With<PanelSurface>>,
    mut capture: ResMut<PointerCapture>,
) {
    capture.ui_has_pointer = surfaces
        .iter()
        .any(|interaction| *interaction != Interaction::None);
}

#[cfg(test)]
fn match_panel_text(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    selected: Option<PieceId>,
) -> String {
    match_panel_text_with_clock_context(scenario, state, selected, false)
}

fn match_panel_text_with_clock_context(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    selected: Option<PieceId>,
    online: bool,
) -> String {
    let mut lines = vec![format!(
        "TURN {} - {:?} to act",
        state.turn_number, state.active_player
    )];
    if is_in_check(scenario, state, state.active_player).unwrap_or(false) {
        lines.push(format!(
            "!!! CHECK - {:?} King is threatened",
            state.active_player
        ));
    }
    match &state.phase {
        TurnPhase::Command => lines.push("Phase: Command - Move or Hold".to_owned()),
        TurnPhase::ResolvingChoices { queue } => {
            lines.push(format!("!!! MANDATORY CHOICE - {} remaining", queue.len()));
        }
    }
    if let Some(clocks) = state.clocks {
        let heading = if online {
            "Clocks at last server snapshot"
        } else {
            "Clocks"
        };
        lines.push(format!(
            "{heading}: North {} - South {}",
            format_clock(clocks.north_millis),
            format_clock(clocks.south_millis)
        ));
        for (player, millis) in [
            ("North", clocks.north_millis),
            ("South", clocks.south_millis),
        ] {
            if millis <= LOW_CLOCK_MILLIS {
                lines.push(format!("!! LOW CLOCK - {player} {}", format_clock(millis)));
            }
        }
    } else {
        lines.push("Clocks: untimed".to_owned());
    }
    if let Some(piece) = selected.and_then(|id| state.pieces.get(&id)) {
        lines.push(format!(
            "Selected: {:?} {:?} at ({}, {})",
            piece.owner, piece.kind, piece.at.x, piece.at.y
        ));
    } else {
        lines.push("Selected: none".to_owned());
    }
    if let Some(outcome) = state.outcome {
        lines.push(format!(
            "!!! MATCH ENDED - {:?} - winner {:?}",
            outcome.reason, outcome.winner
        ));
    }
    lines.push("Controls: H Hold - I collapse/expand panels".to_owned());
    lines.join("\n")
}

fn settlement_panel_text(scenario: &ScenarioDefinition, state: &MatchState) -> String {
    let mut sections = Vec::with_capacity(state.settlements.len());
    for settlement in &state.settlements {
        let site = &scenario.settlements[usize::from(settlement.site_index)];
        let founder = settlement.founder.map_or_else(
            || "none".to_owned(),
            |founder| {
                state.pieces.get(&founder).map_or_else(
                    || "founder absent".to_owned(),
                    |piece| format!("{} present", piece_description(piece)),
                )
            },
        );
        let (governors, blockers) = governance_report(scenario, state, settlement.site_index)
            .map_or_else(
                |_| ("unavailable".to_owned(), "unavailable".to_owned()),
                |report| {
                    let governors = if report.governors.is_empty() {
                        "none".to_owned()
                    } else {
                        report
                            .governors
                            .iter()
                            .map(|line| {
                                state.pieces.get(&line.attacker).map_or_else(
                                    || "piece no longer on board".to_owned(),
                                    piece_description,
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    let blockers = if report.blocked.is_empty() {
                        "none".to_owned()
                    } else {
                        report
                            .blocked
                            .iter()
                            .map(|blocked| blocker_text(state, blocked.blocker))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    (governors, blockers)
                },
            );
        let placement_ready = matches!(
            &state.phase,
            TurnPhase::ResolvingChoices { queue }
                if queue.iter().any(|choice| matches!(
                    choice,
                    crownline_core::state::MandatoryChoice::PlacePawn { settlement_index, .. }
                        if *settlement_index == settlement.site_index
                ))
        );
        let supported_pawn = settlement.produced_pawn.map_or_else(
            || "none".to_owned(),
            |pawn| {
                state
                    .pieces
                    .get(&pawn)
                    .map_or_else(|| "produced Pawn unavailable".to_owned(), piece_description)
            },
        );
        sections.push(format!(
            "{} at {:?}\nOwner: {:?} - founder: {founder}\nGovernors: {governors}\nBlockers: {blockers}\nEstablishment: {}/{}{}\nProduction: {}/{} - readiness: {}\nSupported Pawn: {supported_pawn}",
            site.id,
            site.at,
            settlement.owner,
            settlement.establishment_progress,
            scenario.rules.establishment_cycles,
            if settlement.established { " - ESTABLISHED" } else { "" },
            settlement.production_progress,
            scenario.rules.production_cycles,
            if placement_ready { "PAWN PLACEMENT READY" } else { "not ready" },
        ));
    }
    if sections.is_empty() {
        "No settlements in this scenario.".to_owned()
    } else {
        sections.join("\n\n")
    }
}

fn blocker_text(state: &MatchState, blocker: GovernanceBlocker) -> String {
    match blocker {
        GovernanceBlocker::Piece { piece, at } => state.pieces.get(&piece).map_or_else(
            || format!("piece no longer on board at ({}, {})", at.x, at.y),
            piece_description,
        ),
        GovernanceBlocker::Terrain { terrain, at } => format!("{terrain:?} at {at:?}"),
        GovernanceBlocker::Edge { kind, edge } => format!("{kind:?} at {edge:?}"),
    }
}

fn piece_description(piece: &crownline_core::state::Piece) -> String {
    format!(
        "{:?} {:?} at ({}, {})",
        piece.owner, piece.kind, piece.at.x, piece.at.y
    )
}

fn bounded_history(entries: &[String]) -> String {
    if entries.is_empty() {
        return "RECENT ACTIONS - 0\nNo actions yet.".to_owned();
    }
    let start = entries.len().saturating_sub(HISTORY_LIMIT);
    let mut lines = vec![format!(
        "RECENT ACTIONS - showing {} of {}",
        entries.len() - start,
        entries.len()
    )];
    lines.extend(
        entries[start..]
            .iter()
            .enumerate()
            .map(|(offset, entry)| format!("{}. {entry}", start + offset + 1)),
    );
    lines.join("\n")
}

fn format_clock(millis: u64) -> String {
    let seconds = millis.div_ceil(1_000);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use crownline_core::{scenario::PieceKind, state::ClockState};

    use super::*;

    fn game() -> (ScenarioDefinition, MatchState) {
        let scenario = ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        (scenario, state)
    }

    #[test]
    fn match_panel_names_low_clock_and_selected_piece_without_color_dependency() {
        let (scenario, mut state) = game();
        state.clocks = Some(ClockState {
            north_millis: 59_500,
            south_millis: 90_000,
            increment_millis: 0,
        });
        let king = state
            .pieces
            .values()
            .find(|piece| piece.owner == state.active_player && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        let opposing_king = state
            .pieces
            .values()
            .find(|piece| piece.owner != state.active_player && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        let opposing_rook = state
            .pieces
            .values()
            .find(|piece| piece.owner != state.active_player && piece.kind == PieceKind::Rook)
            .unwrap()
            .id;
        state
            .pieces
            .retain(|id, _| [king, opposing_king, opposing_rook].contains(id));
        state.pieces.get_mut(&king).unwrap().at = crownline_core::scenario::Coord::new(0, 5);
        state.pieces.get_mut(&opposing_rook).unwrap().at =
            crownline_core::scenario::Coord::new(0, 6);
        state.pieces.get_mut(&opposing_king).unwrap().at =
            crownline_core::scenario::Coord::new(7, 7);
        let text = match_panel_text(&scenario, &state, Some(king));
        assert!(text.contains("LOW CLOCK - North"));
        assert!(text.contains("!!! CHECK"));
        assert!(text.contains("Selected:"));
        assert!(text.contains("Phase: Command"));
    }

    #[test]
    fn settlement_panel_covers_required_status_fields() {
        let (scenario, state) = game();
        let text = settlement_panel_text(&scenario, &state);
        for label in [
            "Owner:",
            "founder:",
            "Governors:",
            "Blockers:",
            "Establishment:",
            "Production:",
            "readiness:",
            "Supported Pawn:",
        ] {
            assert!(text.contains(label), "missing {label}");
        }
    }

    #[test]
    fn history_presentation_is_one_bounded_recent_slice() {
        let entries: Vec<_> = (1..=100).map(|index| format!("Move {index}")).collect();
        let text = bounded_history(&entries);
        assert!(text.contains("showing 12 of 100"));
        assert!(!text.contains("Move 88\n"));
        assert!(text.contains("89. Move 89"));
        assert!(text.contains("100. Move 100"));
        assert_eq!(text.lines().count(), HISTORY_LIMIT + 1);
    }

    #[test]
    fn each_panel_collapses_independently() {
        let mut state = PanelState::default();
        state.toggle(PanelKind::Match);
        assert!(state.collapsed(PanelKind::Match));
        assert!(!state.collapsed(PanelKind::Settlements));
        state.toggle(PanelKind::Settlements);
        assert!(state.collapsed(PanelKind::Settlements));
    }

    #[test]
    fn panel_plugin_spawns_two_collapsible_bounded_text_surfaces() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(crate::rendering::BoardRenderingPlugin)
            .add_plugins(InformationPanelsPlugin);
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<InformationPanel>>()
                .iter(world)
                .count(),
            2
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<HistoryPanelText>>()
                .iter(world)
                .count(),
            1
        );
        let mut panels = world.query_filtered::<(&PanelKind, &Node), With<InformationPanel>>();
        for (kind, node) in panels.iter(world) {
            assert_eq!(node.min_width, px(0));
            assert_eq!(node.overflow, Overflow::scroll_y());
            assert_eq!(node.width, percent(SIDE_REGION_PERCENT));
            assert_eq!(node.bottom, percent(BOTTOM_REGION_PERCENT));
            match kind {
                PanelKind::Match => {
                    assert_eq!(node.left, px(0));
                    assert_eq!(node.right, Val::Auto);
                }
                PanelKind::Settlements => {
                    assert_eq!(node.left, Val::Auto);
                    assert_eq!(node.right, px(0));
                }
            }
        }
    }
}
