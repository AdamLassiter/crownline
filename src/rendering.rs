use bevy::prelude::*;
use crownline_core::scenario::{Coord, ScenarioDefinition, TileTerrain};

pub const TILE_SIZE: f32 = 32.0;
const TILE_Z: f32 = 0.0;

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

pub struct BoardRenderingPlugin;

impl Plugin for BoardRenderingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardPalette>()
            .add_systems(Startup, spawn_default_board);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_default_board(mut commands: Commands, palette: Res<BoardPalette>) {
    let scenario: ScenarioDefinition =
        ron::from_str(include_str!("../assets/scenarios/standard.ron"))
            .expect("bundled standard scenario must pass build-time fixture tests");
    spawn_board(&mut commands, &palette, &scenario);
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
}
