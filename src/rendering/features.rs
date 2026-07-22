use std::collections::BTreeSet;

use bevy::prelude::*;
use crownline_core::{
    scenario::{Coord, Edge, EdgeKind, Player, ScenarioDefinition},
    state::{MatchState, SettlementState},
};

use super::{DisplayedGame, ScenarioVisual, TILE_SIZE, tile_position};

pub(super) const KEEP_Z: f32 = 0.1;
pub(super) const SITE_Z: f32 = 0.6;
pub(super) const EDGE_Z: f32 = 1.0;
const SITE_SPAN: f32 = TILE_SIZE * 0.9;
const RING_THICKNESS: f32 = 2.5;
const EDGE_THICKNESS: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct SettlementVisual {
    pub index: u16,
    pub owner: Option<Player>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct PromotionSiteVisual {
    pub index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct KeepTileVisual {
    pub owner: Player,
    pub at: Coord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct FortificationVisual {
    pub index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct EdgeVisual {
    pub edge: Edge,
    pub kind: EdgeKind,
}

#[derive(Component)]
struct SiteLabel;

#[derive(Component)]
struct KeepOwnerMark;

pub(super) fn spawn_scenario_features(
    commands: &mut Commands,
    scenario: &ScenarioDefinition,
    state: &MatchState,
) {
    for keep in &scenario.keeps {
        for at in &keep.tiles {
            let [x, y] = tile_position(*at, scenario);
            commands
                .spawn((
                    Sprite::from_color(keep_tint(keep.owner), Vec2::splat(TILE_SIZE - 3.0)),
                    Transform::from_xyz(x, y, KEEP_Z),
                    KeepTileVisual {
                        owner: keep.owner,
                        at: *at,
                    },
                    ScenarioVisual,
                ))
                .with_children(|tile| {
                    tile.spawn((
                        Text2d::new(match keep.owner {
                            Player::North => "N",
                            Player::South => "S",
                        }),
                        TextFont {
                            font_size: FontSize::Px(8.0),
                            ..default()
                        },
                        TextColor(match keep.owner {
                            Player::North => Color::srgba(0.92, 0.97, 1.0, 0.82),
                            Player::South => Color::srgba(0.12, 0.07, 0.03, 0.82),
                        }),
                        TextLayout::justify(Justify::Center),
                        Transform::from_xyz(10.0, -10.0, 0.02),
                        KeepOwnerMark,
                    ));
                });
        }
    }
    for settlement in &state.settlements {
        spawn_settlement(commands, scenario, settlement);
    }
    for (index, site) in scenario.promotion_sites.iter().enumerate() {
        spawn_promotion_site(
            commands,
            scenario,
            u16::try_from(index).expect("validated site count fits u16"),
            site.at,
        );
    }
    for (index, fortification) in scenario.fortifications.iter().enumerate() {
        spawn_fortification(
            commands,
            scenario,
            u16::try_from(index).expect("validated fortification count fits u16"),
            fortification.tower,
            fortification.owner,
        );
    }
    for (edge, kind) in &scenario.edges {
        spawn_edge(commands, scenario, *edge, *kind);
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn sync_settlement_visuals(
    mut commands: Commands,
    game: Res<DisplayedGame>,
    visuals: Query<(Entity, &SettlementVisual)>,
) {
    let mut current = BTreeSet::new();
    for (entity, visual) in &visuals {
        let Some(settlement) = game.state.settlements.get(usize::from(visual.index)) else {
            commands.entity(entity).despawn();
            continue;
        };
        if visual.owner != settlement.owner {
            commands.entity(entity).despawn();
            continue;
        }
        current.insert(visual.index);
    }
    for settlement in game
        .state
        .settlements
        .iter()
        .filter(|settlement| !current.contains(&settlement.site_index))
    {
        spawn_settlement(&mut commands, &game.scenario, settlement);
    }
}

fn spawn_settlement(
    commands: &mut Commands,
    scenario: &ScenarioDefinition,
    settlement: &SettlementState,
) {
    let at = scenario.settlements[usize::from(settlement.site_index)].at;
    let [x, y] = tile_position(at, scenario);
    let (color, emblem, rotation) = settlement_style(settlement.owner);
    commands
        .spawn((
            Transform::from_xyz(x, y, SITE_Z).with_rotation(rotation),
            Visibility::default(),
            SettlementVisual {
                index: settlement.site_index,
                owner: settlement.owner,
            },
            ScenarioVisual,
        ))
        .with_children(|ring| {
            for offset in [-SITE_SPAN / 2.0, SITE_SPAN / 2.0] {
                ring.spawn((
                    Sprite::from_color(color, Vec2::new(SITE_SPAN, RING_THICKNESS)),
                    Transform::from_xyz(0.0, offset, 0.0),
                ));
                ring.spawn((
                    Sprite::from_color(color, Vec2::new(RING_THICKNESS, SITE_SPAN)),
                    Transform::from_xyz(offset, 0.0, 0.0),
                ));
            }
            ring.spawn((
                Text2d::new(emblem),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(color),
                TextLayout::justify(Justify::Center),
                Transform::from_xyz(0.0, 0.0, 0.1),
                SiteLabel,
            ));
        });
}

fn spawn_promotion_site(
    commands: &mut Commands,
    scenario: &ScenarioDefinition,
    index: u16,
    at: Coord,
) {
    let [x, y] = tile_position(at, scenario);
    commands
        .spawn((
            Transform::from_xyz(x, y, SITE_Z),
            Visibility::default(),
            PromotionSiteVisual { index },
            ScenarioVisual,
        ))
        .with_children(|mark| {
            for rotation in [std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4] {
                mark.spawn((
                    Sprite::from_color(
                        Color::srgba(0.72, 0.28, 0.88, 0.92),
                        Vec2::new(SITE_SPAN, RING_THICKNESS),
                    ),
                    Transform::from_rotation(Quat::from_rotation_z(rotation)),
                ));
            }
        });
}

fn spawn_fortification(
    commands: &mut Commands,
    scenario: &ScenarioDefinition,
    index: u16,
    at: Coord,
    owner: Player,
) {
    let [x, y] = tile_position(at, scenario);
    commands.spawn((
        Text2d::new("T"),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(owner_color(owner)),
        TextLayout::justify(Justify::Center),
        Transform::from_xyz(x, y, EDGE_Z + 0.1),
        FortificationVisual { index },
        ScenarioVisual,
    ));
}

fn spawn_edge(commands: &mut Commands, scenario: &ScenarioDefinition, edge: Edge, kind: EdgeKind) {
    let [first_x, first_y] = tile_position(edge.first, scenario);
    let [second_x, second_y] = tile_position(edge.second, scenario);
    let horizontal = edge.first.x == edge.second.x;
    let size = if horizontal {
        Vec2::new(TILE_SIZE, edge_thickness(kind))
    } else {
        Vec2::new(edge_thickness(kind), TILE_SIZE)
    };
    commands
        .spawn((
            Sprite::from_color(edge_color(kind), size),
            Transform::from_xyz(
                f32::midpoint(first_x, second_x),
                f32::midpoint(first_y, second_y),
                EDGE_Z,
            ),
            EdgeVisual { edge, kind },
            ScenarioVisual,
        ))
        .with_children(|barrier| {
            if let Some(label) = edge_label(kind) {
                barrier.spawn((
                    Text2d::new(label),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.08, 0.08, 0.1)),
                    TextLayout::justify(Justify::Center),
                    Transform::from_xyz(0.0, 0.0, 0.1),
                ));
            }
        });
}

fn settlement_style(owner: Option<Player>) -> (Color, &'static str, Quat) {
    match owner {
        None => (Color::srgba(0.92, 0.92, 0.88, 0.9), "·", Quat::IDENTITY),
        Some(Player::North) => (owner_color(Player::North), "N", Quat::IDENTITY),
        Some(Player::South) => (
            owner_color(Player::South),
            "S",
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
        ),
    }
}

fn owner_color(owner: Player) -> Color {
    match owner {
        Player::North => Color::srgb(0.36, 0.78, 1.0),
        Player::South => Color::srgb(0.98, 0.66, 0.18),
    }
}

fn keep_tint(owner: Player) -> Color {
    match owner {
        Player::North => Color::srgba(0.16, 0.48, 0.72, 0.22),
        Player::South => Color::srgba(0.78, 0.38, 0.08, 0.22),
    }
}

const fn edge_thickness(kind: EdgeKind) -> f32 {
    match kind {
        EdgeKind::River => EDGE_THICKNESS,
        EdgeKind::Bridge => 8.0,
        EdgeKind::Ford => 6.0,
        EdgeKind::Wall => 5.0,
        EdgeKind::Gate => 7.0,
    }
}

fn edge_color(kind: EdgeKind) -> Color {
    match kind {
        EdgeKind::River => Color::srgb(0.12, 0.48, 0.88),
        EdgeKind::Bridge => Color::srgb(0.78, 0.57, 0.27),
        EdgeKind::Ford => Color::srgb(0.42, 0.72, 0.88),
        EdgeKind::Wall => Color::srgb(0.28, 0.3, 0.36),
        EdgeKind::Gate => Color::srgb(0.92, 0.74, 0.24),
    }
}

const fn edge_label(kind: EdgeKind) -> Option<&'static str> {
    match kind {
        EdgeKind::Bridge => Some("="),
        EdgeKind::Ford => Some("··"),
        EdgeKind::Gate => Some("/"),
        EdgeKind::River | EdgeKind::Wall => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::BoardRenderingPlugin;

    #[test]
    fn standard_features_spawn_at_documented_layers() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let world = app.world_mut();

        let mut settlements = world.query::<&SettlementVisual>();
        assert_eq!(settlements.iter(world).count(), 6);
        let mut promotions = world.query::<&PromotionSiteVisual>();
        assert_eq!(promotions.iter(world).count(), 4);
        let mut keeps = world.query::<&KeepTileVisual>();
        assert_eq!(keeps.iter(world).count(), 32);
        let mut towers = world.query::<&FortificationVisual>();
        assert_eq!(towers.iter(world).count(), 4);
        let mut edges = world.query::<&EdgeVisual>();
        assert_eq!(edges.iter(world).count(), 32);

        const {
            assert!(KEEP_Z > super::super::TILE_Z);
            assert!(SITE_Z > KEEP_Z);
            assert!(EDGE_Z > SITE_Z);
            assert!(super::super::PIECE_Z > EDGE_Z);
            assert!(SITE_SPAN > super::super::PIECE_BACKPLATE_SIZE);
        }
    }

    #[test]
    fn canonical_edges_map_to_exact_shared_boundaries() {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../../assets/scenarios/standard.ron")).unwrap();
        for edge in scenario.edges.keys() {
            let [first_x, first_y] = tile_position(edge.first, &scenario);
            let [second_x, second_y] = tile_position(edge.second, &scenario);
            let midpoint = [
                f32::midpoint(first_x, second_x),
                f32::midpoint(first_y, second_y),
            ];
            if edge.first.x == edge.second.x {
                assert!((midpoint[1] % TILE_SIZE).abs() < f32::EPSILON);
            } else {
                assert!((midpoint[0] % TILE_SIZE).abs() < f32::EPSILON);
            }
        }
    }

    #[test]
    fn settlement_ownership_rebuilds_color_shape_and_emblem_cue() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        app.world_mut()
            .resource_mut::<DisplayedGame>()
            .state
            .settlements[0]
            .owner = Some(Player::South);
        app.update();

        let world = app.world_mut();
        let mut settlements = world.query::<&SettlementVisual>();
        assert!(
            settlements
                .iter(world)
                .any(|visual| { visual.index == 0 && visual.owner == Some(Player::South) })
        );
    }

    #[test]
    fn keep_and_settlement_ownership_have_text_cues_independent_of_hue() {
        assert_eq!(settlement_style(None).1, "·");
        assert_eq!(settlement_style(Some(Player::North)).1, "N");
        assert_eq!(settlement_style(Some(Player::South)).1, "S");
        assert_ne!(
            settlement_style(Some(Player::North)).2,
            settlement_style(Some(Player::South)).2
        );

        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let world = app.world_mut();
        let mut marks = world.query_filtered::<&Text2d, With<KeepOwnerMark>>();
        let observed: BTreeSet<_> = marks.iter(world).map(|text| text.0.clone()).collect();
        assert_eq!(observed, BTreeSet::from(["N".to_owned(), "S".to_owned()]));
    }
}
