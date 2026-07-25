use bevy::prelude::*;
use std::collections::BTreeSet;

use crownline_core::{
    scenario::{Coord, PieceKind, Player, ScenarioDefinition, TileTerrain},
    state::{MatchState, Piece, PieceId},
};

use crate::{BoardCamera, ChessFontText};

mod camera;
pub(crate) mod coordinates;
mod features;
mod overlays;
mod transitions;

use bevy::window::PrimaryWindow;
use coordinates::{BoardGeometry, BoardOrientation};
use features::{spawn_scenario_features, sync_settlement_visuals};
use overlays::{OverlayCache, sync_overlays};
pub(crate) use overlays::{OverlayLegend, OverlaySelection, OverlayText, overlay_legend_symbol};
pub(crate) use transitions::LocalTransitionRecord;
pub(crate) use transitions::TransitionEventQueue as LocalTransitionEventQueue;
pub(crate) use transitions::TransitionNoticeLog as LocalTransitionNoticeLog;
use transitions::{
    PiecePresentation, PresentationMotionQueue, PresentationPlayback, TransitionEventQueue,
    animate_piece_presentations, process_piece_motion_requests, process_transition_events,
};

pub use camera::CameraControlPlugin;

pub const TILE_SIZE: f32 = 32.0;
const TILE_Z: f32 = 0.0;
const PIECE_Z: f32 = 2.0;
const PIECE_FONT_SIZE: f32 = 26.0;
pub(super) const PIECE_BACKPLATE_SIZE: f32 = TILE_SIZE * 0.72;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
struct TerrainMark(TileTerrain);

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

#[derive(Component)]
pub(super) struct ScenarioVisual;

#[derive(Resource)]
struct RenderedScenarioId(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct PieceVisual {
    pub id: PieceId,
    pub kind: PieceKind,
    pub owner: Player,
}

#[derive(Component)]
struct PieceBackplate;

#[derive(Resource)]
pub(crate) struct DisplayedGame {
    pub(crate) scenario: ScenarioDefinition,
    pub(crate) state: MatchState,
}

#[derive(Resource)]
pub(crate) struct ChessPieceFont(pub(crate) Handle<Font>);

#[derive(Resource, Default)]
pub struct HoveredBoardSquare(pub Option<Coord>);

#[derive(Resource, Default)]
pub struct PointerCapture {
    pub ui_has_pointer: bool,
}

#[derive(Component)]
struct CoordinateLabel;

pub struct BoardRenderingPlugin;

impl Plugin for BoardRenderingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardPalette>()
            .init_resource::<HoveredBoardSquare>()
            .init_resource::<PointerCapture>()
            .init_resource::<OverlaySelection>()
            .init_resource::<OverlayCache>()
            .init_resource::<OverlayLegend>()
            .init_resource::<OverlayText>()
            .init_resource::<PresentationMotionQueue>()
            .init_resource::<PresentationPlayback>()
            .init_resource::<TransitionEventQueue>()
            .init_resource::<LocalTransitionNoticeLog>()
            .add_systems(Startup, spawn_default_board)
            .add_systems(
                Update,
                (
                    rebuild_changed_scenario,
                    sync_piece_visuals,
                    sync_settlement_visuals,
                    update_hovered_square,
                    sync_overlays.after(update_hovered_square),
                    process_piece_motion_requests.after(sync_piece_visuals),
                    animate_piece_presentations.after(process_piece_motion_requests),
                    process_transition_events,
                ),
            );
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
    let orientation = if scenario
        .rules
        .pawn_forward_y
        .get(&Player::North)
        .is_some_and(|direction| *direction > 0)
    {
        BoardOrientation::NorthAtTop
    } else {
        BoardOrientation::SouthAtTop
    };
    let geometry = BoardGeometry::new(scenario.board, TILE_SIZE, orientation);
    spawn_board(&mut commands, &palette, &scenario);
    spawn_coordinate_labels(&mut commands, &geometry);
    spawn_scenario_features(&mut commands, &scenario, &state);
    for piece in state.pieces.values() {
        spawn_piece(&mut commands, &font, &scenario, piece);
    }
    let scenario_id = scenario.id.clone();
    commands.insert_resource(ChessPieceFont(font));
    commands.insert_resource(DisplayedGame { scenario, state });
    commands.insert_resource(geometry);
    commands.insert_resource(RenderedScenarioId(scenario_id));
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn rebuild_changed_scenario(
    mut commands: Commands,
    game: Res<DisplayedGame>,
    palette: Res<BoardPalette>,
    font: Res<ChessPieceFont>,
    mut rendered: ResMut<RenderedScenarioId>,
    mut geometry: ResMut<BoardGeometry>,
    existing: Query<Entity, With<ScenarioVisual>>,
) {
    if rendered.0 == game.scenario.id {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let orientation = if game
        .scenario
        .rules
        .pawn_forward_y
        .get(&Player::North)
        .is_some_and(|direction| *direction > 0)
    {
        BoardOrientation::NorthAtTop
    } else {
        BoardOrientation::SouthAtTop
    };
    *geometry = BoardGeometry::new(game.scenario.board, TILE_SIZE, orientation);
    spawn_board(&mut commands, &palette, &game.scenario);
    spawn_coordinate_labels(&mut commands, &geometry);
    spawn_scenario_features(&mut commands, &game.scenario, &game.state);
    for piece in game.state.pieces.values() {
        spawn_piece(&mut commands, &font.0, &game.scenario, piece);
    }
    rendered.0.clone_from(&game.scenario.id);
}

#[allow(clippy::needless_pass_by_value)]
fn update_hovered_square(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<BoardCamera>>,
    geometry: Res<BoardGeometry>,
    capture: Res<PointerCapture>,
    mut hovered: ResMut<HoveredBoardSquare>,
) {
    if capture.ui_has_pointer {
        hovered.0 = None;
        return;
    }
    let Ok(window) = windows.single() else {
        hovered.0 = None;
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        hovered.0 = None;
        return;
    };
    hovered.0 = window.cursor_position().and_then(|cursor| {
        camera
            .viewport_to_world_2d(camera_transform, cursor)
            .ok()
            .and_then(|world| geometry.world_to_board(world))
    });
}

fn spawn_coordinate_labels(commands: &mut Commands, geometry: &BoardGeometry) {
    for x in 0..geometry.board.width {
        let at = Coord::new(x, geometry.board.height - 1);
        let world = geometry
            .board_to_world(at)
            .expect("label coordinate is valid");
        commands.spawn((
            Text2d::new(coordinates::file_label(x)),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(Color::srgb(0.82, 0.84, 0.9)),
            TextLayout::justify(Justify::Center),
            Transform::from_xyz(world.x, world.y - TILE_SIZE * 0.68, 4.0),
            Name::new(format!("coordinate {}", coordinates::coordinate_label(at))),
            CoordinateLabel,
            ScenarioVisual,
        ));
    }
    for y in 0..geometry.board.height {
        let at = Coord::new(0, y);
        let world = geometry
            .board_to_world(at)
            .expect("label coordinate is valid");
        commands.spawn((
            Text2d::new((y + 1).to_string()),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(Color::srgb(0.82, 0.84, 0.9)),
            TextLayout::justify(Justify::Center),
            Transform::from_xyz(world.x - TILE_SIZE * 0.68, world.y, 4.0),
            Name::new(format!("coordinate {}", coordinates::coordinate_label(at))),
            CoordinateLabel,
            ScenarioVisual,
        ));
    }
}

#[allow(clippy::needless_pass_by_value)]
fn sync_piece_visuals(
    mut commands: Commands,
    game: Res<DisplayedGame>,
    font: Res<ChessPieceFont>,
    mut visuals: Query<(Entity, &mut PieceVisual, &mut Transform)>,
    mut motion: ResMut<PresentationMotionQueue>,
) {
    let mut existing = BTreeSet::new();
    for (entity, visual, mut transform) in &mut visuals {
        let Some(piece) = game.state.pieces.get(&visual.id) else {
            motion.retire(visual.id, visual.kind, visual.owner, transform.translation);
            commands.entity(entity).despawn();
            continue;
        };
        if piece.kind != visual.kind || piece.owner != visual.owner {
            motion.retire(visual.id, visual.kind, visual.owner, transform.translation);
            commands.entity(entity).despawn();
            continue;
        }
        let [x, y] = tile_position(piece.at, &game.scenario);
        let target = Vec2::new(x, y);
        let previous = transform.translation.truncate();
        if (previous - target).length_squared() > f32::EPSILON {
            motion.move_piece(piece.id, previous - target);
        }
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
            ScenarioVisual,
        ))
        .with_children(|visual| {
            visual
                .spawn((
                    PiecePresentation { id: piece.id },
                    Transform::default(),
                    Visibility::default(),
                ))
                .with_children(|presentation| {
                    presentation.spawn((
                        Sprite::from_color(backplate, Vec2::splat(PIECE_BACKPLATE_SIZE)),
                        Transform::from_rotation(rotation),
                        PieceBackplate,
                    ));
                    presentation.spawn((
                        Text2d::new(piece_glyph(piece.kind)),
                        TextFont {
                            font: FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(PIECE_FONT_SIZE),
                            ..default()
                        },
                        TextColor(text),
                        TextLayout::justify(Justify::Center),
                        Transform::from_xyz(0.0, piece_glyph_vertical_offset(piece.kind), 1.0),
                        ChessFontText,
                    ));
                });
        });
}

pub(super) fn player_piece_style(player: Player) -> (Color, Color, Quat) {
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

pub(super) const fn piece_glyph(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::King => "♔",
        PieceKind::Queen => "♕",
        PieceKind::Rook => "♖",
        PieceKind::Bishop => "♗",
        PieceKind::Knight => "♘",
        PieceKind::Pawn => "♙",
    }
}

/// Compensates for the visual ink bounds of the bundled Noto chess glyphs.
///
/// Noto places the glyph ink above the font layout centre. These values are
/// the negated ink-centre offsets measured at 4x raster resolution and scaled
/// back into the 26 px world-space font size.
pub(super) const fn piece_glyph_vertical_offset(kind: PieceKind) -> f32 {
    match kind {
        PieceKind::King => -3.625,
        PieceKind::Queen | PieceKind::Rook => -3.75,
        PieceKind::Bishop => -3.125,
        PieceKind::Knight => -4.25,
        PieceKind::Pawn => -4.125,
    }
}

fn spawn_board(commands: &mut Commands, palette: &BoardPalette, scenario: &ScenarioDefinition) {
    commands
        .spawn((
            BoardRoot,
            ScenarioVisual,
            Transform::default(),
            Visibility::default(),
        ))
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
                    board
                        .spawn((
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
                        ))
                        .with_children(|tile| {
                            if let Some(symbol) = terrain_symbol(terrain) {
                                tile.spawn((
                                    Text2d::new(symbol),
                                    TextFont {
                                        font_size: FontSize::Px(8.0),
                                        ..default()
                                    },
                                    TextColor(match parity {
                                        TileParity::Light => Color::srgba(0.05, 0.06, 0.08, 0.72),
                                        TileParity::Dark => Color::srgba(0.96, 0.96, 0.92, 0.78),
                                    }),
                                    TextLayout::justify(Justify::Center),
                                    Transform::from_xyz(-10.0, 10.0, 0.02),
                                    TerrainMark(terrain),
                                ));
                            }
                        });
                }
            }
        });
}

const fn terrain_symbol(terrain: TileTerrain) -> Option<&'static str> {
    match terrain {
        TileTerrain::Open => None,
        TileTerrain::Forest => Some("F"),
        TileTerrain::Mountain => Some("M"),
        TileTerrain::Road => Some("R"),
    }
}

const fn tile_parity(at: Coord) -> TileParity {
    if (at.x + at.y).is_multiple_of(2) {
        TileParity::Light
    } else {
        TileParity::Dark
    }
}

pub(super) fn tile_position(at: Coord, scenario: &ScenarioDefinition) -> [f32; 2] {
    BoardGeometry::new(scenario.board, TILE_SIZE, BoardOrientation::NorthAtTop)
        .board_to_world(at)
        .expect("validated render coordinate is within the board")
        .to_array()
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
    fn non_open_terrain_has_a_unique_non_hue_mark() {
        assert_eq!(terrain_symbol(TileTerrain::Open), None);
        assert_eq!(terrain_symbol(TileTerrain::Forest), Some("F"));
        assert_eq!(terrain_symbol(TileTerrain::Mountain), Some("M"));
        assert_eq!(terrain_symbol(TileTerrain::Road), Some("R"));

        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let world = app.world_mut();
        let mut marks = world.query::<(&TerrainMark, &Text2d)>();
        for (mark, text) in marks.iter(world) {
            assert_eq!(Some(text.0.as_str()), terrain_symbol(mark.0));
        }
        assert_eq!(
            marks.iter(world).count(),
            world
                .resource::<DisplayedGame>()
                .scenario
                .terrain
                .values()
                .filter(|terrain| **terrain != TileTerrain::Open)
                .count()
        );
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
    fn piece_glyph_offsets_compensate_bundled_font_ink_bounds() {
        for (kind, expected) in [
            (PieceKind::King, -3.625),
            (PieceKind::Queen, -3.75),
            (PieceKind::Rook, -3.75),
            (PieceKind::Bishop, -3.125),
            (PieceKind::Knight, -4.25),
            (PieceKind::Pawn, -4.125),
        ] {
            let actual = piece_glyph_vertical_offset(kind);
            assert!((actual - expected).abs() < f32::EPSILON);
            assert!((-4.5..=-2.5).contains(&actual));
        }
    }

    #[test]
    fn piece_ownership_uses_plate_orientation_and_contrast_not_only_hue() {
        let (north_text, north_plate, north_rotation) = player_piece_style(Player::North);
        let (south_text, south_plate, south_rotation) = player_piece_style(Player::South);
        assert_ne!(north_rotation, south_rotation);
        assert_ne!(north_text, north_plate);
        assert_ne!(south_text, south_plate);
        assert_ne!(north_plate, south_plate);
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

    #[test]
    fn changing_scenario_rebuilds_tiles_features_and_geometry_without_stale_visuals() {
        let mut app = App::new();
        app.add_plugins(BoardRenderingPlugin);
        app.update();
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../assets/scenarios/introductory.ron")).unwrap();
        let expected_tiles = usize::from(scenario.board.width) * usize::from(scenario.board.height);
        let state = MatchState::from_scenario(&scenario).unwrap();
        {
            let mut game = app.world_mut().resource_mut::<DisplayedGame>();
            game.scenario = scenario.clone();
            game.state = state;
        }
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world.query::<&BoardTile>().iter(world).count(),
            expected_tiles
        );
        assert_eq!(world.resource::<BoardGeometry>().board, scenario.board);
        assert_eq!(
            world.query::<&BoardRoot>().iter(world).count(),
            1,
            "old scenario root must be despawned"
        );
    }
}
