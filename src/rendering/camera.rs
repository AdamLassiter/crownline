use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    window::PrimaryWindow,
};

use crate::{
    config::{CameraBindingsSettings, CameraKey, ClientSettings, camera_modifier_down},
    lifecycle::ClientFlow,
};

use super::{PointerCapture, TILE_SIZE, coordinates::BoardGeometry};

const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 2.0;
const FIT_FRACTION: f32 = 0.9;
const PAN_PIXELS_PER_SECOND: f32 = 520.0;
const ZOOM_FACTOR: f32 = 1.16;
const MAX_TEXT_RASTER_MULTIPLIER: u8 = 4;

#[derive(Debug, Clone, Copy, Component)]
struct WorldTextRaster {
    base_font_size: f32,
    base_scale: Vec3,
    multiplier: u8,
}

#[derive(Default)]
struct CameraGestureState {
    initialized: bool,
    last_drag_cursor: Option<Vec2>,
}

#[derive(Debug, Clone, Copy)]
struct CameraBindings {
    pan_up: KeyCode,
    pan_down: KeyCode,
    pan_left: KeyCode,
    pan_right: KeyCode,
    zoom_in: KeyCode,
    zoom_out: KeyCode,
    reset: KeyCode,
}

impl From<&CameraBindingsSettings> for CameraBindings {
    fn from(settings: &CameraBindingsSettings) -> Self {
        Self {
            pan_up: key_code(settings.pan_up),
            pan_down: key_code(settings.pan_down),
            pan_left: key_code(settings.pan_left),
            pan_right: key_code(settings.pan_right),
            zoom_in: key_code(settings.zoom_in),
            zoom_out: key_code(settings.zoom_out),
            reset: key_code(settings.reset),
        }
    }
}

pub struct CameraControlPlugin;

impl Plugin for CameraControlPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PointerCapture>().add_systems(
            Update,
            (
                camera_controls,
                register_world_text_raster,
                update_world_text_raster,
            )
                .chain(),
        );
    }
}

#[allow(clippy::needless_pass_by_value)]
fn register_world_text_raster(
    mut commands: Commands,
    text: Query<(Entity, &TextFont, &Transform), Added<Text2d>>,
) {
    for (entity, font, transform) in &text {
        let FontSize::Px(base_font_size) = font.font_size else {
            continue;
        };
        commands.entity(entity).insert(WorldTextRaster {
            base_font_size,
            base_scale: transform.scale,
            multiplier: 0,
        });
    }
}

#[allow(clippy::needless_pass_by_value)]
fn update_world_text_raster(
    cameras: Query<&Projection, With<Camera2d>>,
    mut text: Query<(&mut WorldTextRaster, &mut TextFont, &mut Transform)>,
) {
    let Ok(Projection::Orthographic(projection)) = cameras.single() else {
        return;
    };
    let multiplier = text_raster_multiplier(projection.scale);
    for (mut raster, mut font, mut transform) in &mut text {
        if raster.multiplier != multiplier {
            apply_text_raster_multiplier(multiplier, &mut raster, &mut font, &mut transform);
        }
    }
}

fn text_raster_multiplier(camera_scale: f32) -> u8 {
    if camera_scale >= 1.0 {
        1
    } else if camera_scale >= 0.5 {
        2
    } else {
        MAX_TEXT_RASTER_MULTIPLIER
    }
}

fn apply_text_raster_multiplier(
    multiplier: u8,
    raster: &mut WorldTextRaster,
    font: &mut TextFont,
    transform: &mut Transform,
) {
    let multiplier = multiplier.clamp(1, MAX_TEXT_RASTER_MULTIPLIER);
    let factor = f32::from(multiplier);
    font.font_size = FontSize::Px(raster.base_font_size * factor);
    transform.scale = raster.base_scale / factor;
    raster.multiplier = multiplier;
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn camera_controls(
    time: Res<Time>,
    settings: Res<ClientSettings>,
    flow: Res<ClientFlow>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    geometry: Res<BoardGeometry>,
    capture: Res<PointerCapture>,
    mut cameras: Query<
        (&Camera, &mut Transform, &GlobalTransform, &mut Projection),
        With<Camera2d>,
    >,
    mut gesture: Local<CameraGestureState>,
) {
    if !matches!(*flow, ClientFlow::Playing | ClientFlow::OnlinePlaying) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, mut transform, global_transform, mut projection)) = cameras.single_mut() else {
        return;
    };
    let Projection::Orthographic(orthographic) = projection.as_mut() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    if viewport.min_element() <= 0.0 {
        return;
    }
    let bindings = CameraBindings::from(&settings.camera_bindings);
    let keyboard_camera = camera_modifier_down(&keys);

    if !gesture.initialized || (keyboard_camera && keys.just_pressed(bindings.reset)) {
        orthographic.scale = fit_scale(*geometry, viewport);
        transform.translation.x = 0.0;
        transform.translation.y = 0.0;
        gesture.initialized = true;
        gesture.last_drag_cursor = None;
        return;
    }

    let cursor = window.cursor_position();
    let old_scale = orthographic.scale;
    let mut zoom_steps = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
        })
        .sum::<f32>();
    if keyboard_camera && keys.just_pressed(bindings.zoom_in) {
        zoom_steps += 1.0;
    }
    if keyboard_camera && keys.just_pressed(bindings.zoom_out) {
        zoom_steps -= 1.0;
    }
    if capture.ui_has_pointer {
        zoom_steps = 0.0;
    }
    if zoom_steps.abs() > f32::EPSILON {
        let new_scale = (old_scale / ZOOM_FACTOR.powf(zoom_steps)).clamp(MIN_SCALE, MAX_SCALE);
        if let Some(anchor) =
            cursor.and_then(|position| camera.viewport_to_world_2d(global_transform, position).ok())
        {
            let center = transform.translation.truncate();
            let adjusted = zoom_center(center, anchor, old_scale, new_scale);
            transform.translation.x = adjusted.x;
            transform.translation.y = adjusted.y;
        }
        orthographic.scale = new_scale;
    }

    let mut direction = Vec2::ZERO;
    if keyboard_camera && keys.pressed(bindings.pan_up) {
        direction.y += 1.0;
    }
    if keyboard_camera && keys.pressed(bindings.pan_down) {
        direction.y -= 1.0;
    }
    if keyboard_camera && keys.pressed(bindings.pan_left) {
        direction.x -= 1.0;
    }
    if keyboard_camera && keys.pressed(bindings.pan_right) {
        direction.x += 1.0;
    }
    if direction != Vec2::ZERO {
        let movement =
            direction.normalize() * PAN_PIXELS_PER_SECOND * time.delta_secs() * orthographic.scale;
        transform.translation.x += movement.x;
        transform.translation.y += movement.y;
    }

    let dragging = !capture.ui_has_pointer
        && (mouse_buttons.pressed(MouseButton::Middle)
            || mouse_buttons.pressed(MouseButton::Right));
    if dragging {
        if let (Some(current), Some(previous)) = (cursor, gesture.last_drag_cursor) {
            let delta = current - previous;
            transform.translation.x -= delta.x * orthographic.scale;
            transform.translation.y += delta.y * orthographic.scale;
        }
        gesture.last_drag_cursor = cursor;
    } else {
        gesture.last_drag_cursor = None;
    }

    let clamped = clamp_center(
        transform.translation.truncate(),
        *geometry,
        viewport,
        orthographic.scale,
    );
    transform.translation.x = clamped.x;
    transform.translation.y = clamped.y;
}

fn fit_scale(geometry: BoardGeometry, viewport: Vec2) -> f32 {
    let board = Vec2::new(
        f32::from(geometry.board.width) * geometry.tile_size,
        f32::from(geometry.board.height) * geometry.tile_size,
    );
    (board.x / (viewport.x * FIT_FRACTION))
        .max(board.y / (viewport.y * FIT_FRACTION))
        .clamp(MIN_SCALE, MAX_SCALE)
}

fn zoom_center(center: Vec2, anchor: Vec2, old_scale: f32, new_scale: f32) -> Vec2 {
    if old_scale <= 0.0 {
        return center;
    }
    center + (anchor - center) * (1.0 - new_scale / old_scale)
}

fn clamp_center(center: Vec2, geometry: BoardGeometry, viewport: Vec2, scale: f32) -> Vec2 {
    let board_half = Vec2::new(
        f32::from(geometry.board.width) * geometry.tile_size / 2.0,
        f32::from(geometry.board.height) * geometry.tile_size / 2.0,
    );
    let view_half = viewport * scale / 2.0;
    let visible_margin = Vec2::splat(TILE_SIZE);
    let limit = (board_half + view_half - visible_margin).max(Vec2::ZERO);
    center.clamp(-limit, limit)
}

const fn key_code(key: CameraKey) -> KeyCode {
    match key {
        CameraKey::W => KeyCode::KeyW,
        CameraKey::A => KeyCode::KeyA,
        CameraKey::S => KeyCode::KeyS,
        CameraKey::D => KeyCode::KeyD,
        CameraKey::Q => KeyCode::KeyQ,
        CameraKey::E => KeyCode::KeyE,
        CameraKey::F => KeyCode::KeyF,
        CameraKey::Up => KeyCode::ArrowUp,
        CameraKey::Down => KeyCode::ArrowDown,
        CameraKey::Left => KeyCode::ArrowLeft,
        CameraKey::Right => KeyCode::ArrowRight,
        CameraKey::Minus => KeyCode::Minus,
        CameraKey::Equal => KeyCode::Equal,
    }
}

#[cfg(test)]
mod tests {
    use crownline_core::scenario::BoardSize;

    use super::*;

    fn geometry(size: u16) -> BoardGeometry {
        BoardGeometry::new(
            BoardSize {
                width: size,
                height: size,
            },
            TILE_SIZE,
            super::super::coordinates::BoardOrientation::default(),
        )
    }

    #[test]
    fn fit_scale_covers_all_authored_map_sizes_with_readable_bounds() {
        for size in [16, 20, 24] {
            for viewport in [
                Vec2::new(800.0, 600.0),
                Vec2::new(1280.0, 720.0),
                Vec2::new(1920.0, 1080.0),
                Vec2::new(2560.0, 1440.0),
            ] {
                let scale = fit_scale(geometry(size), viewport);
                assert!((MIN_SCALE..=MAX_SCALE).contains(&scale));
                assert!(
                    f32::from(size) * TILE_SIZE
                        <= viewport.min_element() * FIT_FRACTION * scale + 0.01
                );
            }
        }
    }

    #[test]
    fn zoom_preserves_the_world_point_under_the_cursor() {
        let center = Vec2::new(100.0, -40.0);
        let anchor = Vec2::new(220.0, 80.0);
        let adjusted = zoom_center(center, anchor, 1.0, 0.5);
        let anchor_after = adjusted + (anchor - center) * 0.5;
        assert!((anchor_after - anchor).length() < f32::EPSILON);
    }

    #[test]
    fn pan_clamp_keeps_at_least_one_tile_visible() {
        let geometry = geometry(24);
        let viewport = Vec2::new(640.0, 480.0);
        let clamped = clamp_center(Vec2::splat(100_000.0), geometry, viewport, 1.0);
        let board_half = 24.0 * TILE_SIZE / 2.0;
        let view_half = viewport / 2.0;
        assert!(clamped.x - view_half.x < board_half);
        assert!(clamped.y - view_half.y < board_half);
        assert!(clamped.x + view_half.x > -board_half);
        assert!(clamped.y + view_half.y > -board_half);
    }

    #[test]
    fn alternate_keyboard_bindings_map_to_bevy_keys() {
        let bindings = CameraBindings::from(&CameraBindingsSettings {
            pan_up: CameraKey::Up,
            pan_down: CameraKey::Down,
            pan_left: CameraKey::Left,
            pan_right: CameraKey::Right,
            zoom_in: CameraKey::Equal,
            zoom_out: CameraKey::Minus,
            reset: CameraKey::F,
        });
        assert_eq!(bindings.pan_up, KeyCode::ArrowUp);
        assert_eq!(bindings.zoom_in, KeyCode::Equal);
    }

    #[test]
    fn raster_levels_cover_every_supported_zoom_without_changing_world_size() {
        for scale in [MIN_SCALE, 0.3, 0.49, 0.5, 0.9, 1.0, MAX_SCALE] {
            let multiplier = text_raster_multiplier(scale);
            assert!(multiplier <= MAX_TEXT_RASTER_MULTIPLIER);
            assert!(
                f32::from(multiplier) * scale >= 1.0,
                "scale {scale} would magnify a {multiplier}x glyph atlas"
            );
        }

        let original_scale = Vec3::new(1.5, 0.75, 1.0);
        let mut raster = WorldTextRaster {
            base_font_size: 26.0,
            base_scale: original_scale,
            multiplier: 0,
        };
        let mut font = TextFont::from_font_size(26.0);
        let mut transform = Transform::from_scale(original_scale);
        apply_text_raster_multiplier(4, &mut raster, &mut font, &mut transform);
        assert_eq!(font.font_size, FontSize::Px(104.0));
        assert_eq!(transform.scale, original_scale / 4.0);
        apply_text_raster_multiplier(1, &mut raster, &mut font, &mut transform);
        assert_eq!(font.font_size, FontSize::Px(26.0));
        assert_eq!(transform.scale, original_scale);
    }

    #[test]
    fn every_new_pixel_sized_world_text_is_registered_automatically() {
        let mut app = App::new();
        app.add_systems(Update, register_world_text_raster);
        let entity = app
            .world_mut()
            .spawn((
                Text2d::new("zoom-safe"),
                TextFont::from_font_size(13.0),
                Transform::from_scale(Vec3::splat(0.75)),
            ))
            .id();
        app.update();

        let raster = app.world().get::<WorldTextRaster>(entity).unwrap();
        assert!((raster.base_font_size - 13.0).abs() < f32::EPSILON);
        assert_eq!(raster.base_scale, Vec3::splat(0.75));
    }
}
