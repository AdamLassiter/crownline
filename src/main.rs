mod config;
mod help;
mod lifecycle;
mod local_interaction;
mod local_persistence;
mod online_connection;
mod online_lifecycle;
mod online_lobby;
mod online_status;
mod panels;
mod rendering;

use bevy::{
    asset::{AssetPlugin, LoadState},
    prelude::*,
    ui::UiScale,
    window::WindowResolution,
};
use config::ClientSettings;
use help::RulesHelpPlugin;
use lifecycle::LocalLifecyclePlugin;
use local_interaction::LocalInteractionPlugin;
use local_persistence::LocalPersistencePlugin;
use online_connection::OnlineConnectionPlugin;
use online_lifecycle::OnlineLifecyclePlugin;
use online_lobby::OnlineLobbyPlugin;
use online_status::OnlineStatusPlugin;
use panels::InformationPanelsPlugin;
use rendering::{BoardRenderingPlugin, CameraControlPlugin};

const WINDOW_TITLE: &str = "Crownlines";
const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_REVISION: Option<&str> = option_env!("CROWNLINE_BUILD_REVISION");

fn main() {
    if std::env::args_os()
        .skip(1)
        .any(|argument| argument == "--version" || argument == "-V")
    {
        println!("{}", version_line());
        return;
    }
    let settings = ClientSettings::load_or_default();
    App::new()
        .insert_resource(settings.clone())
        .insert_resource(configured_ui_scale(&settings))
        .insert_resource(ClearColor(Color::srgb(0.055, 0.059, 0.071)))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: runtime_asset_root(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: window_title(),
                        resolution: WindowResolution::new(
                            settings.window_width,
                            settings.window_height,
                        ),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(BoardRenderingPlugin)
        .add_plugins(CameraControlPlugin)
        .add_plugins(LocalInteractionPlugin)
        .add_plugins(InformationPanelsPlugin)
        .add_plugins(RulesHelpPlugin)
        .add_plugins(LocalLifecyclePlugin)
        .add_plugins(OnlineLobbyPlugin)
        .add_plugins(OnlineConnectionPlugin)
        .add_plugins(OnlineStatusPlugin)
        .add_plugins(OnlineLifecyclePlugin)
        .add_plugins(LocalPersistencePlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, monitor_chess_font)
        .run();
}

fn version_line() -> String {
    format!(
        "crownline {APPLICATION_VERSION} (revision {})",
        BUILD_REVISION.unwrap_or("development")
    )
}

fn window_title() -> String {
    let revision = BUILD_REVISION.map_or("development", |revision| {
        revision.get(..revision.len().min(12)).unwrap_or(revision)
    });
    format!("{WINDOW_TITLE} {APPLICATION_VERSION} · {revision}")
}

fn runtime_asset_root() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(|parent| parent.join("assets")))
        .filter(|assets| assets.is_dir())
        .map_or_else(
            || "assets".to_owned(),
            |assets| assets.to_string_lossy().into_owned(),
        )
}

fn configured_ui_scale(settings: &ClientSettings) -> UiScale {
    UiScale(settings.ui_scale)
}

#[derive(Component)]
pub(crate) struct ChessFontText;

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
        Text2d::new(
            "CROWNLINES\nChess font could not be loaded.\nCheck the assets/fonts installation.",
        ),
        TextFont {
            font_size: FontSize::Px(36.0),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.75, 0.3)),
        TextLayout::justify(Justify::Center),
        Transform::from_xyz(0.0, 0.0, 10.0),
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
    if !matches!(
        asset_server.load_state(status.handle.id()),
        LoadState::Failed(_)
    ) {
        return;
    }

    let first_failure = !status.fallback_active;
    status.fallback_active = true;
    for (mut visibility, chess, fallback) in &mut text {
        *visibility = font_visibility(true, chess.is_some(), fallback.is_some());
    }
    if first_failure {
        error!("bundled chess font failed to load; showing readable fallback");
    }
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
mod accessibility_tests {
    use super::*;

    #[test]
    fn configured_ui_scale_reaches_bevy_layout() {
        for scale in [0.75, 1.0, 1.5, 2.0, 2.5] {
            let settings = ClientSettings {
                ui_scale: scale,
                ..ClientSettings::default()
            };
            assert!((configured_ui_scale(&settings).0 - scale).abs() < f32::EPSILON);
        }
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

    #[test]
    fn version_metadata_is_visible_without_starting_bevy() {
        let line = version_line();
        assert!(line.contains(env!("CARGO_PKG_VERSION")));
        assert!(line.contains("revision"));
        assert!(window_title().contains(env!("CARGO_PKG_VERSION")));
    }
}
