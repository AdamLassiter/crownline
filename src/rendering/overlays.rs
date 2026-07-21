use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use crownline_core::{
    apply_action, attack_lines_on, governance_report, is_in_check, legal_moves,
    rules::{GovernanceBlocker, LegalMove, MoveKind},
    scenario::{Coord, PieceKind, ScenarioDefinition},
    state::{Action, MatchState, PieceId, TurnPhase},
};

use super::{DisplayedGame, HoveredBoardSquare, tile_position};

const OVERLAY_Z: f32 = 4.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverlayKind {
    Attack,
    LegalMove,
    Capture,
    GainedAttack,
    LostAttack,
    Governor,
    GovernanceBlocker,
    GainedGovernance,
    LostGovernance,
    Selected,
    IllegalWarning,
    Check,
}

impl OverlayKind {
    const fn precedence(self) -> u8 {
        self as u8
    }
}

#[derive(Resource, Default)]
pub struct OverlaySelection {
    pub piece: Option<PieceId>,
}

#[derive(Resource, Default)]
pub struct OverlayText {
    pub lines: Vec<String>,
}

#[derive(Resource)]
pub struct OverlayLegend {
    pub entries: Vec<(OverlayKind, &'static str)>,
}

impl Default for OverlayLegend {
    fn default() -> Self {
        Self {
            entries: vec![
                (OverlayKind::Selected, "selected square"),
                (OverlayKind::LegalMove, "legal move"),
                (OverlayKind::Capture, "capture"),
                (OverlayKind::Attack, "current attack"),
                (OverlayKind::Check, "King in check"),
                (OverlayKind::Governor, "governance line"),
                (OverlayKind::GovernanceBlocker, "blocked governance"),
                (OverlayKind::GainedAttack, "attack gained by preview"),
                (OverlayKind::LostAttack, "attack lost by preview"),
                (
                    OverlayKind::GainedGovernance,
                    "governance gained by preview",
                ),
                (OverlayKind::LostGovernance, "governance lost by preview"),
                (OverlayKind::IllegalWarning, "illegal destination"),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OverlayCacheKey {
    revision: u64,
    selected: Option<PieceId>,
    hovered: Option<Coord>,
}

#[derive(Resource, Default)]
pub struct OverlayCache {
    key: Option<OverlayCacheKey>,
    overlays: BTreeMap<Coord, BTreeSet<OverlayKind>>,
    recomputations: u64,
}

#[derive(Component)]
pub(super) struct OverlayVisual;

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn sync_overlays(
    mut commands: Commands,
    game: Res<DisplayedGame>,
    selection: Res<OverlaySelection>,
    hovered: Res<HoveredBoardSquare>,
    mut cache: ResMut<OverlayCache>,
    mut text: ResMut<OverlayText>,
    legend: Res<OverlayLegend>,
    existing: Query<Entity, With<OverlayVisual>>,
) {
    let key = OverlayCacheKey {
        revision: game.state.revision,
        selected: selection.piece,
        hovered: hovered.0,
    };
    if cache.key == Some(key) {
        return;
    }
    let (overlays, mut lines) =
        build_overlay_model(&game.scenario, &game.state, selection.piece, hovered.0);
    lines.push("Overlay legend:".to_owned());
    lines.extend(
        legend
            .entries
            .iter()
            .map(|(_, description)| format!("- {description}")),
    );
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    for (at, kinds) in &overlays {
        let [x, y] = tile_position(*at, &game.scenario);
        for kind in kinds {
            let (symbol, color, size) = overlay_style(*kind);
            commands.spawn((
                Text2d::new(symbol),
                TextFont {
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(color),
                TextLayout::justify(Justify::Center),
                Transform::from_xyz(x, y, OVERLAY_Z + f32::from(kind.precedence()) * 0.01),
                OverlayVisual,
            ));
        }
    }
    cache.key = Some(key);
    cache.overlays = overlays;
    cache.recomputations = cache.recomputations.saturating_add(1);
    text.lines = lines;
}

fn build_overlay_model(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    selected: Option<PieceId>,
    hovered: Option<Coord>,
) -> (BTreeMap<Coord, BTreeSet<OverlayKind>>, Vec<String>) {
    let mut overlays = BTreeMap::<Coord, BTreeSet<OverlayKind>>::new();
    let mut lines = Vec::new();
    let in_check = is_in_check(scenario, state, state.active_player).unwrap_or(false);
    let hold_available =
        state.outcome.is_none() && matches!(state.phase, TurnPhase::Command) && !in_check;
    lines.push(if hold_available {
        "Hold is available.".to_owned()
    } else {
        "Hold is unavailable while checked or resolving choices.".to_owned()
    });
    if in_check
        && let Some(king) = state
            .pieces
            .values()
            .find(|piece| piece.owner == state.active_player && piece.kind == PieceKind::King)
    {
        insert(&mut overlays, king.at, OverlayKind::Check);
        lines.push(format!("{:?} King is in check.", state.active_player));
    }

    add_governance_overlays(scenario, state, &mut overlays);
    let Some(piece_id) = selected else {
        return (overlays, lines);
    };
    let Some(piece) = state.pieces.get(&piece_id) else {
        lines.push("The selected piece no longer exists.".to_owned());
        return (overlays, lines);
    };
    insert(&mut overlays, piece.at, OverlayKind::Selected);
    let moves: Vec<_> = legal_moves(scenario, state)
        .unwrap_or_default()
        .into_iter()
        .filter(|candidate| candidate.piece == piece_id)
        .collect();
    for candidate in &moves {
        insert(
            &mut overlays,
            candidate.to,
            if matches!(
                candidate.kind,
                MoveKind::Capture { .. } | MoveKind::EnPassant { .. }
            ) {
                OverlayKind::Capture
            } else {
                OverlayKind::LegalMove
            },
        );
    }
    for at in attacked_by_piece(scenario, state, piece_id) {
        insert(&mut overlays, at, OverlayKind::Attack);
    }
    lines.push(format!(
        "Selected {:?} at {:?}: {} legal destinations.",
        piece.kind,
        piece.at,
        moves.len()
    ));

    if let Some(hovered) = hovered {
        add_hover_preview(
            scenario,
            state,
            piece_id,
            piece.at,
            hovered,
            &moves,
            &mut overlays,
            &mut lines,
        );
    }
    (overlays, lines)
}

#[allow(clippy::too_many_arguments)]
fn add_hover_preview(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    piece_id: PieceId,
    piece_at: Coord,
    hovered: Coord,
    moves: &[LegalMove],
    overlays: &mut BTreeMap<Coord, BTreeSet<OverlayKind>>,
    lines: &mut Vec<String>,
) {
    let Some(candidate) = moves.iter().find(|candidate| candidate.to == hovered) else {
        if hovered != piece_at {
            insert(overlays, hovered, OverlayKind::IllegalWarning);
            lines.push(format!("{hovered:?} is not a legal destination."));
        }
        return;
    };
    let action = Action::Move {
        player: state.active_player,
        piece: piece_id,
        to: candidate.to,
    };
    let Ok(preview) = apply_action(scenario, state, &action) else {
        insert(overlays, hovered, OverlayKind::IllegalWarning);
        return;
    };

    let before_attacks = attacked_by_piece(scenario, state, piece_id);
    let after_attacks = attacked_by_piece(scenario, &preview.state, piece_id);
    for at in after_attacks.difference(&before_attacks) {
        insert(overlays, *at, OverlayKind::GainedAttack);
    }
    for at in before_attacks.difference(&after_attacks) {
        insert(overlays, *at, OverlayKind::LostAttack);
    }
    let before_governance = governed_sites(scenario, state, piece_id);
    let after_governance = governed_sites(scenario, &preview.state, piece_id);
    for index in after_governance.difference(&before_governance) {
        insert(
            overlays,
            scenario.settlements[usize::from(*index)].at,
            OverlayKind::GainedGovernance,
        );
    }
    for index in before_governance.difference(&after_governance) {
        insert(
            overlays,
            scenario.settlements[usize::from(*index)].at,
            OverlayKind::LostGovernance,
        );
    }
    lines.push(format!(
        "Preview: {} attacks gained, {} lost; {} governance links gained, {} lost.",
        after_attacks.difference(&before_attacks).count(),
        before_attacks.difference(&after_attacks).count(),
        after_governance.difference(&before_governance).count(),
        before_governance.difference(&after_governance).count(),
    ));
}

fn add_governance_overlays(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    overlays: &mut BTreeMap<Coord, BTreeSet<OverlayKind>>,
) {
    for settlement in &state.settlements {
        let Ok(report) = governance_report(scenario, state, settlement.site_index) else {
            continue;
        };
        for governor in report.governors {
            for at in governor.path {
                insert(overlays, at, OverlayKind::Governor);
            }
        }
        for blocked in report.blocked {
            let at = match blocked.blocker {
                GovernanceBlocker::Piece { at, .. } | GovernanceBlocker::Terrain { at, .. } => at,
                GovernanceBlocker::Edge { edge, .. } => edge.second,
            };
            insert(overlays, at, OverlayKind::GovernanceBlocker);
        }
    }
}

fn attacked_by_piece(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    piece: PieceId,
) -> BTreeSet<Coord> {
    let Some(owner) = state.pieces.get(&piece).map(|piece| piece.owner) else {
        return BTreeSet::new();
    };
    let mut attacked = BTreeSet::new();
    for y in 0..scenario.board.height {
        for x in 0..scenario.board.width {
            let at = Coord::new(x, y);
            if attack_lines_on(scenario, state, at, owner)
                .unwrap_or_default()
                .iter()
                .any(|line| line.attacker == piece)
            {
                attacked.insert(at);
            }
        }
    }
    attacked
}

fn governed_sites(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    piece: PieceId,
) -> BTreeSet<u16> {
    state
        .settlements
        .iter()
        .filter_map(|settlement| {
            governance_report(scenario, state, settlement.site_index)
                .ok()
                .filter(|report| {
                    report
                        .governors
                        .iter()
                        .any(|governor| governor.attacker == piece)
                })
                .map(|report| report.settlement_index)
        })
        .collect()
}

fn insert(overlays: &mut BTreeMap<Coord, BTreeSet<OverlayKind>>, at: Coord, kind: OverlayKind) {
    overlays.entry(at).or_default().insert(kind);
}

fn overlay_style(kind: OverlayKind) -> (&'static str, Color, f32) {
    match kind {
        OverlayKind::Attack => ("·", Color::srgba(0.7, 0.82, 1.0, 0.9), 18.0),
        OverlayKind::LegalMove => ("•", Color::srgb(0.2, 0.92, 0.72), 17.0),
        OverlayKind::Capture => ("×", Color::srgb(1.0, 0.42, 0.24), 24.0),
        OverlayKind::GainedAttack => ("+", Color::srgb(0.2, 1.0, 0.48), 16.0),
        OverlayKind::LostAttack => ("−", Color::srgb(0.95, 0.38, 0.52), 16.0),
        OverlayKind::Governor => ("G", Color::srgb(0.3, 0.88, 1.0), 10.0),
        OverlayKind::GovernanceBlocker => ("#", Color::srgb(0.92, 0.48, 0.2), 12.0),
        OverlayKind::GainedGovernance => ("G+", Color::srgb(0.1, 1.0, 0.72), 11.0),
        OverlayKind::LostGovernance => ("G−", Color::srgb(1.0, 0.34, 0.62), 11.0),
        OverlayKind::Selected => ("□", Color::srgb(1.0, 0.94, 0.28), 28.0),
        OverlayKind::IllegalWarning => ("/", Color::srgb(1.0, 0.16, 0.12), 28.0),
        OverlayKind::Check => ("!", Color::srgb(1.0, 0.05, 0.05), 30.0),
    }
}

#[cfg(test)]
mod tests {
    use crownline_core::scenario::Player;

    use super::*;
    use crate::rendering::BoardRenderingPlugin;

    #[test]
    fn precedence_places_check_and_illegal_warnings_above_ordinary_highlights() {
        assert!(OverlayKind::Check.precedence() > OverlayKind::Selected.precedence());
        assert!(OverlayKind::IllegalWarning.precedence() > OverlayKind::LegalMove.precedence());
        assert!(OverlayKind::Capture.precedence() > OverlayKind::LegalMove.precedence());
        assert_ne!(
            overlay_style(OverlayKind::GainedAttack).0,
            overlay_style(OverlayKind::LostAttack).0
        );
        assert_ne!(
            overlay_style(OverlayKind::GainedGovernance).0,
            overlay_style(OverlayKind::LostGovernance).0
        );
    }

    #[test]
    fn overlay_model_recomputes_only_for_revision_selection_or_hover_changes() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let initial = app.world().resource::<OverlayCache>().recomputations;
        app.update();
        assert_eq!(
            app.world().resource::<OverlayCache>().recomputations,
            initial
        );

        let selected = app
            .world()
            .resource::<DisplayedGame>()
            .state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::Pawn)
            .unwrap()
            .id;
        app.world_mut().resource_mut::<OverlaySelection>().piece = Some(selected);
        app.update();
        assert_eq!(
            app.world().resource::<OverlayCache>().recomputations,
            initial + 1
        );
        app.update();
        assert_eq!(
            app.world().resource::<OverlayCache>().recomputations,
            initial + 1
        );
    }

    #[test]
    fn selected_piece_model_contains_moves_attacks_and_text_equivalent() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let selected = app
            .world()
            .resource::<DisplayedGame>()
            .state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::Knight)
            .unwrap()
            .id;
        app.world_mut().resource_mut::<OverlaySelection>().piece = Some(selected);
        app.update();

        let cache = app.world().resource::<OverlayCache>();
        assert!(
            cache
                .overlays
                .values()
                .any(|kinds| kinds.contains(&OverlayKind::Selected))
        );
        assert!(cache.overlays.values().any(|kinds| {
            kinds.contains(&OverlayKind::LegalMove) || kinds.contains(&OverlayKind::Capture)
        }));
        assert!(
            app.world()
                .resource::<OverlayText>()
                .lines
                .iter()
                .any(|line| line.contains("legal destinations"))
        );
        assert_eq!(app.world().resource::<OverlayLegend>().entries.len(), 12);
    }
}
