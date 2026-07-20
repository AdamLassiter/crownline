mod config;

use bevy::{asset::LoadState, prelude::*, window::WindowResolution};
use config::ClientSettings;

const WINDOW_TITLE: &str = "Crownlines";

fn main() {
    let settings = ClientSettings::load_or_default();
    App::new()
        .insert_resource(settings.clone())
        .insert_resource(ClearColor(Color::srgb(0.055, 0.059, 0.071)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: WINDOW_TITLE.to_owned(),
                resolution: WindowResolution::new(settings.window_width, settings.window_height),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, monitor_chess_font)
        .run();
}

#[derive(Component)]
struct ChessFontText;

#[derive(Component)]
struct FontFallbackText;

#[derive(Resource)]
struct ChessFontStatus {
    handle: Handle<Font>,
    fallback_active: bool,
}

#[allow(clippy::needless_pass_by_value)]
fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);
    let chess_font = asset_server.load("fonts/NotoSansSymbols2-Regular.ttf");
    commands.spawn((
        Text2d::new("CROWNLINES\n♔ ♕ ♖ ♗ ♘ ♙"),
        TextFont {
            font: FontSource::Handle(chess_font.clone()),
            font_size: FontSize::Px(64.0),
            ..default()
        },
        TextColor(Color::srgb(0.88, 0.72, 0.25)),
        TextLayout::justify(Justify::Center),
        ChessFontText,
    ));
    commands.spawn((
        Text2d::new(
            "CROWNLINES\nChess font could not be loaded.\nCheck the assets/fonts installation.",
        ),
        TextFont {
            font_size: FontSize::Px(36.0),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.75, 0.3)),
        TextLayout::justify(Justify::Center),
        Visibility::Hidden,
        FontFallbackText,
    ));
    commands.insert_resource(ChessFontStatus {
        handle: chess_font,
        fallback_active: false,
    });
}

#[allow(clippy::needless_pass_by_value)]
fn monitor_chess_font(
    asset_server: Res<AssetServer>,
    mut status: ResMut<ChessFontStatus>,
    mut text: Query<(
        &mut Visibility,
        Option<&ChessFontText>,
        Option<&FontFallbackText>,
    )>,
) {
    if status.fallback_active
        || !matches!(
            asset_server.load_state(status.handle.id()),
            LoadState::Failed(_)
        )
    {
        return;
    }

    status.fallback_active = true;
    for (mut visibility, chess, fallback) in &mut text {
        *visibility = font_visibility(true, chess.is_some(), fallback.is_some());
    }
    error!("bundled chess font failed to load; showing readable fallback");
}

fn font_visibility(load_failed: bool, chess_text: bool, fallback_text: bool) -> Visibility {
    match (load_failed, chess_text, fallback_text) {
        (true, true, _) => Visibility::Hidden,
        (true, _, true) | (false, true, _) => Visibility::Visible,
        (false, _, true) => Visibility::Hidden,
        _ => Visibility::Inherited,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_failure_hides_glyphs_and_shows_ascii_fallback() {
        assert_eq!(font_visibility(true, true, false), Visibility::Hidden);
        assert_eq!(font_visibility(true, false, true), Visibility::Visible);
    }
}
