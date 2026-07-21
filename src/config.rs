use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::PathBuf,
};

use bevy::prelude::Resource;
use crownline_core::scenario::Player;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

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
    pub saved_online_seat: Option<SavedOnlineSeat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedOnlineSeat {
    pub server_url: String,
    pub room_code: String,
    pub match_id: Uuid,
    pub seat: Player,
    pub credential_id: Uuid,
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
            saved_online_seat: None,
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

    pub fn save(&self) -> Result<(), SettingsError> {
        self.validate()?;
        let path = settings_path()?;
        let parent = path.parent().ok_or(SettingsError::NoProjectDirectory)?;
        fs::create_dir_all(parent).map_err(|error| SettingsError::Write {
            path: path.clone(),
            error,
        })?;
        let source = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|error| SettingsError::Serialize(error.to_string()))?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|error| SettingsError::Write {
            path: path.clone(),
            error,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| SettingsError::Write {
                    path: path.clone(),
                    error,
                })?;
        }
        file.write_all(source.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| SettingsError::Write { path, error })
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
    #[error("could not serialize client settings: {0}")]
    Serialize(String),
    #[error("could not write settings at {path}: {error}")]
    Write {
        path: PathBuf,
        #[source]
        error: std::io::Error,
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
