//! `~/.config/narvi/config.toml` schema + load/save. See PROTOCOL.md / SPEC.md.
//!
//! Validation policy: unknown keys → warn + ignore; out-of-range → clamp + warn;
//! missing `active_profile` → first profile or neutral. Load/save land in M2.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::profile::Profile;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub hyprland: Hyprland,
    #[serde(default)]
    pub scheduling: Scheduling,
    #[serde(default)]
    pub ui: Ui,
    #[serde(default, rename = "profiles")]
    pub profiles: Vec<Profile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct General {
    pub active_profile: String,
    pub restore_on_start: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            active_profile: "default".into(),
            restore_on_start: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hyprland {
    pub shader_path: String,
}

impl Default for Hyprland {
    fn default() -> Self {
        Self {
            shader_path: "~/.config/hypr/shaders/narvi.frag".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleMode {
    Sun,
    Fixed,
    Off,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scheduling {
    pub enabled: bool,
    pub mode: ScheduleMode,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub day_profile: String,
    pub night_profile: String,
    pub transition_minutes: u32,
    /// `fixed` mode only, `"HH:MM"`.
    pub day_time: Option<String>,
    pub night_time: Option<String>,
}

impl Default for Scheduling {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ScheduleMode::Off,
            latitude: None,
            longitude: None,
            day_profile: "default".into(),
            night_profile: "night".into(),
            transition_minutes: 30,
            day_time: None,
            night_time: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ui {
    pub theme: String,
    pub accent: String,
    pub preview: String,
    pub preview_image: String,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            theme: "saturn".into(),
            accent: "#E0A23C".into(),
            preview: "sample".into(),
            preview_image: String::new(),
        }
    }
}

impl Config {
    /// Default config path: `$XDG_CONFIG_HOME/narvi/config.toml`.
    pub fn default_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("dev", "zer0dot", "narvi")
            .ok_or_else(|| crate::Error::Config("cannot resolve config dir".into()))?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Load + validate from `path` (clamp out-of-range, warn on unknown). M2.
    pub fn load(_path: &std::path::Path) -> Result<Self> {
        todo!("M2: read TOML, clamp params, warn on unknown keys")
    }

    /// Persist to `path` atomically. M2.
    pub fn save(&self, _path: &std::path::Path) -> Result<()> {
        todo!("M2: atomic TOML write")
    }
}
