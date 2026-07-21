use bevy::prelude::*;
use std::collections::BTreeSet;

use crownline_core::{
    scenario::{Coord, PieceKind, Player, ScenarioDefinition, TileTerrain},
    state::{MatchState, Piece, PieceId},
};

use crate::ChessFontText;

pub const TILE_SIZE: f32 = 32.0;
const TILE_Z: f32 = 0.0;
const PIECE_Z: f32 = 2.0;
const PIECE_FONT_SIZE: f32 = 26.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum TileParity {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct BoardTile {
    pub at: Coord,
    pub parity: TileParity,
    pub terrain: TileTerrain,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PaletteColor([f32; 3]);

impl PaletteColor {
    const fn color(self) -> Color {
        Color::srgb(self.0[0], self.0[1], self.0[2])
    }
}

#[cfg(test)]
impl PaletteColor {
    const fn luminance(self) -> f32 {
        self.0[0] * 0.2126 + self.0[1] * 0.7152 + self.0[2] * 0.0722
    }
}

#[derive(Resource)]
pub struct BoardPalette {
    light: [PaletteColor; 4],
    dark: [PaletteColor; 4],
}

impl Default for BoardPalette {
    fn default() -> Self {
        Self {
            light: [
                PaletteColor([0.84, 0.80, 0.72]),
                PaletteColor([0.35, 0.58, 0.39]),
                PaletteColor([0.69, 0.69, 0.72]),
                PaletteColor([0.72, 0.58, 0.32]),
            ],
            dark: [
                PaletteColor([0.46, 0.42, 0.36]),
                PaletteColor([0.14, 0.25, 0.18]),
                PaletteColor([0.34, 0.36, 0.40]),
                PaletteColor([0.36, 0.28, 0.14]),
            ],
        }
    }
}

impl BoardPalette {
    fn color(&self, parity: TileParity, terrain: Option<TileTerrain>) -> Color {
        let index = terrain.map_or(0, terrain_index);
        match parity {
            TileParity::Light => self.light[index].color(),
            TileParity::Dark => self.dark[index].color(),
        }
    }
}

#[derive(Component)]
struct BoardRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct PieceVisual {
    pub id: PieceId,
    pub kind: PieceKind,
    pub owner: Player,
}

#[derive(Component)]
struct PieceBackplate;

#[derive(Resource)]
struct DisplayedGame {
    scenario: ScenarioDefinition,
    state: MatchState,
}

#[derive(Resource)]
struct ChessPieceFont(Handle<Font>);

pub struct BoardRenderingPlugin;

impl Plugin for BoardRenderingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardPalette>()
            .add_systems(Startup, spawn_default_board)
            .add_systems(Update, sync_piece_visuals);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_default_board(
    mut commands: Commands,
    palette: Res<BoardPalette>,
    asset_server: Option<Res<AssetServer>>,
) {
    let scenario: ScenarioDefinition =
        ron::from_str(include_str!("../assets/scenarios/standard.ron"))
            .expect("bundled standard scenario must pass build-time fixture tests");
    let state = MatchState::from_scenario(&scenario)
        .expect("bundled standard scenario must construct canonical state");
    let font = asset_server.map_or_else(Handle::default, |assets| {
        assets.load("fonts/NotoSansSymbols2-Regular.ttf")
    });
    spawn_board(&mut commands, &palette, &scenario);
    for piece in state.pieces.values() {
        spawn_piece(&mut commands, &font, &scenario, piece);
    }
    commands.insert_resource(ChessPieceFont(font));
    commands.insert_resource(DisplayedGame { scenario, state });
}

#[allow(clippy::needless_pass_by_value)]
fn sync_piece_visuals(
    mut commands: Commands,
    game: Res<DisplayedGame>,
    font: Res<ChessPieceFont>,
    mut visuals: Query<(Entity, &mut PieceVisual, &mut Transform)>,
) {
    let mut existing = BTreeSet::new();
    for (entity, visual, mut transform) in &mut visuals {
        let Some(piece) = game.state.pieces.get(&visual.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        if piece.kind != visual.kind || piece.owner != visual.owner {
            commands.entity(entity).despawn();
            continue;
        }
        let [x, y] = tile_position(piece.at, &game.scenario);
        transform.translation = Vec3::new(x, y, PIECE_Z);
        existing.insert(piece.id);
    }
    for piece in game
        .state
        .pieces
        .values()
        .filter(|piece| !existing.contains(&piece.id))
    {
        spawn_piece(&mut commands, &font.0, &game.scenario, piece);
    }
}

fn spawn_piece(
    commands: &mut Commands,
    font: &Handle<Font>,
    scenario: &ScenarioDefinition,
    piece: &Piece,
) {
    let [x, y] = tile_position(piece.at, scenario);
    let (text, backplate, rotation) = player_piece_style(piece.owner);
    commands
        .spawn((
            PieceVisual {
                id: piece.id,
                kind: piece.kind,
                owner: piece.owner,
            },
            Transform::from_xyz(x, y, PIECE_Z),
            Visibility::default(),
        ))
        .with_children(|visual| {
            visual.spawn((
                Sprite::from_color(backplate, Vec2::splat(TILE_SIZE * 0.72)),
                Transform::from_rotation(rotation),
                PieceBackplate,
            ));
            visual.spawn((
                Text2d::new(piece_glyph(piece.kind)),
                TextFont {
                    font: FontSource::Handle(font.clone()),
                    font_size: FontSize::Px(PIECE_FONT_SIZE),
                    ..default()
                },
                TextColor(text),
                TextLayout::justify(Justify::Center),
                Transform::from_xyz(0.0, 0.0, 1.0),
                ChessFontText,
            ));
        });
}

fn player_piece_style(player: Player) -> (Color, Color, Quat) {
    match player {
        Player::North => (
            Color::srgb(0.94, 0.97, 1.0),
            Color::srgba(0.04, 0.09, 0.18, 0.88),
            Quat::IDENTITY,
        ),
        Player::South => (
            Color::srgb(0.12, 0.07, 0.03),
            Color::srgba(0.94, 0.72, 0.25, 0.9),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
        ),
    }
}

const fn piece_glyph(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::King => "♔",
        PieceKind::Queen => "♕",
        PieceKind::Rook => "♖",
        PieceKind::Bishop => "♗",
        PieceKind::Knight => "♘",
        PieceKind::Pawn => "♙",
    }
}

fn spawn_board(commands: &mut Commands, palette: &BoardPalette, scenario: &ScenarioDefinition) {
    commands
        .spawn((BoardRoot, Transform::default(), Visibility::default()))
        .with_children(|board| {
            for y in 0..scenario.board.height {
                for x in 0..scenario.board.width {
                    let at = Coord::new(x, y);
                    let parity = tile_parity(at);
                    let terrain = scenario
                        .terrain
                        .get(&at)
                        .copied()
                        .unwrap_or(TileTerrain::Open);
                    let [world_x, world_y] = tile_position(at, scenario);
                    board.spawn((
                        Sprite::from_color(
                            palette.color(parity, scenario.terrain.get(&at).copied()),
                            Vec2::splat(TILE_SIZE),
                        ),
                        Transform::from_xyz(world_x, world_y, TILE_Z),
                        BoardTile {
                            at,
                            parity,
                            terrain,
                        },
                    ));
                }
            }
        });
}

const fn tile_parity(at: Coord) -> TileParity {
    if (at.x + at.y).is_multiple_of(2) {
        TileParity::Light
    } else {
        TileParity::Dark
    }
}

fn tile_position(at: Coord, scenario: &ScenarioDefinition) -> [f32; 2] {
    let left = -(f32::from(scenario.board.width) * TILE_SIZE) / 2.0 + TILE_SIZE / 2.0;
    let top = (f32::from(scenario.board.height) * TILE_SIZE) / 2.0 - TILE_SIZE / 2.0;
    [
        left + f32::from(at.x) * TILE_SIZE,
        top - f32::from(at.y) * TILE_SIZE,
    ]
}

const fn terrain_index(terrain: TileTerrain) -> usize {
    match terrain {
        TileTerrain::Open => 0,
        TileTerrain::Forest => 1,
        TileTerrain::Mountain => 2,
        TileTerrain::Road => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_depends_only_on_canonical_coordinate() {
        assert_eq!(tile_parity(Coord::new(0, 0)), TileParity::Light);
        assert_eq!(tile_parity(Coord::new(1, 0)), TileParity::Dark);
        assert_eq!(tile_parity(Coord::new(0, 1)), TileParity::Dark);
        assert_eq!(tile_parity(Coord::new(23, 23)), TileParity::Light);
    }

    #[test]
    fn terrain_palette_preserves_parity_and_grayscale_separation() {
        let palette = BoardPalette::default();
        for index in 0..4 {
            assert!(palette.light[index].luminance() - palette.dark[index].luminance() > 0.15);
        }
        for colors in [&palette.light, &palette.dark] {
            for left in 0..colors.len() {
                for right in (left + 1)..colors.len() {
                    assert!((colors[left].luminance() - colors[right].luminance()).abs() > 0.06);
                }
            }
        }
    }

    #[test]
    fn missing_terrain_metadata_falls_back_to_open_palette() {
        let palette = BoardPalette::default();
        assert_eq!(
            palette.color(TileParity::Light, None),
            palette.color(TileParity::Light, Some(TileTerrain::Open))
        );
    }

    #[test]
    fn standard_board_spawns_one_gapless_coplanar_tile_per_coordinate() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();

        let world = app.world_mut();
        let mut tiles = world.query::<(&BoardTile, &Sprite, &Transform)>();
        let mut count = 0;
        for (_, sprite, transform) in tiles.iter(world) {
            count += 1;
            assert_eq!(sprite.custom_size, Some(Vec2::splat(TILE_SIZE)));
            assert!((transform.translation.z - TILE_Z).abs() < f32::EPSILON);
            assert!(
                (transform.translation.x.rem_euclid(TILE_SIZE) - TILE_SIZE / 2.0).abs()
                    < f32::EPSILON
            );
            assert!(
                (transform.translation.y.rem_euclid(TILE_SIZE) - TILE_SIZE / 2.0).abs()
                    < f32::EPSILON
            );
        }
        assert_eq!(count, 20 * 20);
    }

    #[test]
    fn piece_kinds_use_the_unfilled_unicode_silhouette_range() {
        assert_eq!(piece_glyph(PieceKind::King), "♔");
        assert_eq!(piece_glyph(PieceKind::Queen), "♕");
        assert_eq!(piece_glyph(PieceKind::Rook), "♖");
        assert_eq!(piece_glyph(PieceKind::Bishop), "♗");
        assert_eq!(piece_glyph(PieceKind::Knight), "♘");
        assert_eq!(piece_glyph(PieceKind::Pawn), "♙");
    }

    #[test]
    fn default_pieces_spawn_once_and_follow_stable_ids() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();

        let (entity, piece_id) = {
            let world = app.world_mut();
            let mut visuals = world.query::<(Entity, &PieceVisual)>();
            let all: Vec<_> = visuals
                .iter(world)
                .map(|(entity, visual)| (entity, visual.id))
                .collect();
            assert_eq!(all.len(), 32);
            assert_eq!(
                all.iter().map(|(_, id)| *id).collect::<BTreeSet<_>>().len(),
                32
            );
            all[0]
        };

        let destination = Coord::new(0, 10);
        app.world_mut()
            .resource_mut::<DisplayedGame>()
            .state
            .pieces
            .get_mut(&piece_id)
            .unwrap()
            .at = destination;
        app.update();

        let game = app.world().resource::<DisplayedGame>();
        let expected = tile_position(destination, &game.scenario);
        let transform = app.world().entity(entity).get::<Transform>().unwrap();
        assert!((transform.translation.x - expected[0]).abs() < f32::EPSILON);
        assert!((transform.translation.y - expected[1]).abs() < f32::EPSILON);
    }

    #[test]
    fn promotion_replaces_only_the_retired_stable_piece_id() {
        use crownline_core::state::PieceOrigin;

        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let retired = {
            let world = app.world_mut();
            let mut visuals = world.query::<&PieceVisual>();
            visuals.iter(world).next().unwrap().id
        };
        let promoted = PieceId(10_000);
        {
            let mut game = app.world_mut().resource_mut::<DisplayedGame>();
            let pawn = game.state.pieces.remove(&retired).unwrap();
            game.state.pieces.insert(
                promoted,
                Piece {
                    id: promoted,
                    owner: pawn.owner,
                    kind: PieceKind::Queen,
                    at: pawn.at,
                    origin: PieceOrigin::Promoted { from: retired },
                    has_moved: true,
                },
            );
        }
        app.update();

        let world = app.world_mut();
        let mut visuals = world.query::<&PieceVisual>();
        let ids: BTreeSet<_> = visuals.iter(world).map(|visual| visual.id).collect();
        assert_eq!(ids.len(), 32);
        assert!(!ids.contains(&retired));
        assert!(ids.contains(&promoted));
    }
}
