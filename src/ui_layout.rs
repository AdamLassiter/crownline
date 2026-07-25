use bevy::{camera::Viewport, prelude::*};

/// Stable screen regions shared by the board camera and screen-space UI.
const SIDE_REGION_PERCENT_U32: u32 = 22;
const BOARD_REGION_HEIGHT_PERCENT_U32: u32 = 80;
pub(crate) const SIDE_REGION_PERCENT: f32 = 22.0;
pub(crate) const BOTTOM_REGION_PERCENT: f32 = 20.0;
pub(crate) const BOARD_REGION_WIDTH_PERCENT: f32 = 100.0 - SIDE_REGION_PERCENT * 2.0;
pub(crate) const BOARD_REGION_HEIGHT_PERCENT: f32 = 100.0 - BOTTOM_REGION_PERCENT;

pub(crate) fn board_viewport_physical(window: &Window) -> Viewport {
    board_viewport_for_size(UVec2::new(
        window.physical_width(),
        window.physical_height(),
    ))
}

pub(crate) fn board_viewport_logical(window: &Window) -> Vec2 {
    Vec2::new(
        window.width() * BOARD_REGION_WIDTH_PERCENT / 100.0,
        window.height() * BOARD_REGION_HEIGHT_PERCENT / 100.0,
    )
}

fn board_viewport_for_size(window: UVec2) -> Viewport {
    let left = window.x * SIDE_REGION_PERCENT_U32 / 100;
    let width = window.x.saturating_sub(left * 2);
    let height = window.y * BOARD_REGION_HEIGHT_PERCENT_U32 / 100;
    Viewport {
        physical_position: UVec2::new(left, 0),
        physical_size: UVec2::new(width.max(1), height.max(1)),
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_viewport_reserves_two_sides_and_the_bottom() {
        for window in [UVec2::new(800, 480), UVec2::new(1920, 1080)] {
            let viewport = board_viewport_for_size(window);
            assert_eq!(viewport.physical_position.y, 0);
            assert_eq!(
                viewport.physical_position.x * 2 + viewport.physical_size.x,
                window.x
            );
            assert_eq!(viewport.physical_size.y, window.y * 4 / 5);
            assert!(viewport.physical_size.x > 0);
            assert!(viewport.physical_size.y > 0);
        }
    }
}
