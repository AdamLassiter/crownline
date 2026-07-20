mod config;

use bevy::{prelude::*, window::WindowResolution};
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
        .run();
}

#[allow(clippy::needless_pass_by_value)]
fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text2d::new("CROWNLINES\n♔ ♕ ♖ ♗ ♘ ♙"),
        TextFont {
            font: FontSource::Handle(asset_server.load("fonts/NotoSansSymbols2-Regular.ttf")),
            font_size: FontSize::Px(64.0),
            ..default()
        },
        TextColor(Color::srgb(0.88, 0.72, 0.25)),
        TextLayout::justify(Justify::Center),
    ));
}
