use std::{fs, path::PathBuf};

use bevy::prelude::Resource;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SETTINGS_FILE: &str = "settings.ron";

#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    pub window_width: u32,
    pub window_height: u32,
    pub ui_scale: f32,
    pub server_url: String,
    pub reduced_motion: bool,
    pub camera_bindings: CameraBindingsSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraKey {
    W,
    A,
    S,
    D,
    Q,
    E,
    F,
    Up,
    Down,
    Left,
    Right,
    Minus,
    Equal,
}

#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraBindingsSettings {
    pub pan_up: CameraKey,
    pub pan_down: CameraKey,
    pub pan_left: CameraKey,
    pub pan_right: CameraKey,
    pub zoom_in: CameraKey,
    pub zoom_out: CameraKey,
    pub reset: CameraKey,
}

impl Default for CameraBindingsSettings {
    fn default() -> Self {
        Self {
            pan_up: CameraKey::W,
            pan_down: CameraKey::S,
            pan_left: CameraKey::A,
            pan_right: CameraKey::D,
            zoom_in: CameraKey::E,
            zoom_out: CameraKey::Q,
            reset: CameraKey::F,
        }
    }
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            window_width: 1280,
            window_height: 800,
            ui_scale: 1.0,
            server_url: "ws://127.0.0.1:5000".to_owned(),
            reduced_motion: false,
            camera_bindings: CameraBindingsSettings::default(),
        }
    }
}

impl ClientSettings {
    pub fn load_or_default() -> Self {
        match Self::load() {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(%error, "using default client settings");
                Self::default()
            }
        }
    }

    pub fn load() -> Result<Self, SettingsError> {
        let path = settings_path()?;
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(SettingsError::Read { path, error }),
        };
        let settings: Self = ron::from_str(&source).map_err(|error| SettingsError::Parse {
            path: path.clone(),
            error,
        })?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<(), SettingsError> {
        if !(640..=7680).contains(&self.window_width) {
            return Err(SettingsError::InvalidField(
                "window_width must be between 640 and 7680",
            ));
        }
        if !(480..=4320).contains(&self.window_height) {
            return Err(SettingsError::InvalidField(
                "window_height must be between 480 and 4320",
            ));
        }
        if !(0.75..=2.5).contains(&self.ui_scale) {
            return Err(SettingsError::InvalidField(
                "ui_scale must be between 0.75 and 2.5",
            ));
        }
        if !(self.server_url.starts_with("ws://") || self.server_url.starts_with("wss://")) {
            return Err(SettingsError::InvalidField(
                "server_url must begin with ws:// or wss://",
            ));
        }
        Ok(())
    }
}

fn settings_path() -> Result<PathBuf, SettingsError> {
    ProjectDirs::from("org", "Crownlines", "Crownlines")
        .map(|dirs| dirs.config_dir().join(SETTINGS_FILE))
        .ok_or(SettingsError::NoProjectDirectory)
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("the operating system did not provide a configuration directory")]
    NoProjectDirectory,
    #[error("could not read settings at {path}: {error}")]
    Read {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
    #[error("could not parse settings at {path}: {error}")]
    Parse {
        path: PathBuf,
        #[source]
        error: ron::error::SpannedError,
    },
    #[error("invalid client setting: {0}")]
    InvalidField(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        ClientSettings::default().validate().unwrap();
    }

    #[test]
    fn rejects_non_websocket_server_url() {
        let settings = ClientSettings {
            server_url: "https://example.invalid".to_owned(),
            ..ClientSettings::default()
        };
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::InvalidField(_))
        ));
    }
}
