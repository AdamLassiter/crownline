use bevy::prelude::*;
use crownline_core::scenario::{BoardSize, Coord};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoardOrientation {
    #[default]
    NorthAtTop,
    SouthAtTop,
}

#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct BoardGeometry {
    pub board: BoardSize,
    pub tile_size: f32,
    pub orientation: BoardOrientation,
}

impl BoardGeometry {
    pub const fn new(board: BoardSize, tile_size: f32, orientation: BoardOrientation) -> Self {
        Self {
            board,
            tile_size,
            orientation,
        }
    }

    pub fn board_to_world(self, at: Coord) -> Option<Vec2> {
        if !at.is_within(self.board) || self.tile_size <= 0.0 || !self.tile_size.is_finite() {
            return None;
        }
        let display = self.orient(at);
        let half_width = f32::from(self.board.width) * self.tile_size / 2.0;
        let half_height = f32::from(self.board.height) * self.tile_size / 2.0;
        Some(Vec2::new(
            -half_width + (f32::from(display.x) + 0.5) * self.tile_size,
            half_height - (f32::from(display.y) + 0.5) * self.tile_size,
        ))
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn world_to_board(self, world: Vec2) -> Option<Coord> {
        if self.tile_size <= 0.0 || !self.tile_size.is_finite() || !world.is_finite() {
            return None;
        }
        let half_width = f32::from(self.board.width) * self.tile_size / 2.0;
        let half_height = f32::from(self.board.height) * self.tile_size / 2.0;
        if world.x < -half_width
            || world.x >= half_width
            || world.y > half_height
            || world.y <= -half_height
        {
            return None;
        }
        let display = Coord::new(
            ((world.x + half_width) / self.tile_size).floor() as u16,
            ((half_height - world.y) / self.tile_size).floor() as u16,
        );
        Some(self.orient(display))
    }

    fn orient(self, at: Coord) -> Coord {
        match self.orientation {
            BoardOrientation::NorthAtTop => at,
            BoardOrientation::SouthAtTop => {
                Coord::new(self.board.width - 1 - at.x, self.board.height - 1 - at.y)
            }
        }
    }
}

pub fn file_label(mut x: u16) -> String {
    let mut reversed = String::new();
    loop {
        let remainder = u8::try_from(x % 26).expect("remainder is below 26");
        reversed.push(char::from(b'a' + remainder));
        if x < 26 {
            break;
        }
        x = x / 26 - 1;
    }
    reversed.chars().rev().collect()
}

pub fn coordinate_label(at: Coord) -> String {
    format!("{}{}", file_label(at.x), at.y + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(orientation: BoardOrientation) -> BoardGeometry {
        BoardGeometry::new(
            BoardSize {
                width: 24,
                height: 24,
            },
            32.0,
            orientation,
        )
    }

    #[test]
    fn corners_and_representative_coordinates_round_trip_in_both_orientations() {
        for orientation in [BoardOrientation::NorthAtTop, BoardOrientation::SouthAtTop] {
            let geometry = geometry(orientation);
            for at in [
                Coord::new(0, 0),
                Coord::new(23, 0),
                Coord::new(0, 23),
                Coord::new(23, 23),
                Coord::new(7, 16),
            ] {
                let world = geometry.board_to_world(at).unwrap();
                assert_eq!(geometry.world_to_board(world), Some(at));
            }
        }
    }

    #[test]
    fn borders_negative_space_and_out_of_board_points_are_explicit() {
        let geometry = geometry(BoardOrientation::NorthAtTop);
        assert_eq!(
            geometry.world_to_board(Vec2::new(-384.0, 384.0)),
            Some(Coord::new(0, 0))
        );
        assert_eq!(
            geometry.world_to_board(Vec2::new(-0.1, -0.1)),
            Some(Coord::new(11, 12))
        );
        for outside in [
            Vec2::new(384.0, 0.0),
            Vec2::new(0.0, -384.0),
            Vec2::new(-384.1, 0.0),
            Vec2::new(0.0, 384.1),
        ] {
            assert_eq!(geometry.world_to_board(outside), None);
        }
        assert_eq!(geometry.board_to_world(Coord::new(24, 0)), None);
    }

    #[test]
    fn labels_follow_canonical_files_and_forward_rank_direction() {
        assert_eq!(file_label(0), "a");
        assert_eq!(file_label(25), "z");
        assert_eq!(file_label(26), "aa");
        assert_eq!(file_label(63), "bl");
        assert_eq!(coordinate_label(Coord::new(0, 0)), "a1");
        assert_eq!(coordinate_label(Coord::new(23, 23)), "x24");
        assert_eq!(coordinate_label(Coord::new(4, 2)), "e3");
    }
}
