//! `~/.config/narvi/config.toml` schema + load/save. See PROTOCOL.md / SPEC.md.
//!
//! Validation policy: unknown keys → warn + ignore; out-of-range → clamp + warn;
//! missing `active_profile` → first profile or neutral.

use std::io::Write;
use std::path::{Path, PathBuf};

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

const KNOWN_KEYS: [&str; 5] = ["general", "hyprland", "scheduling", "ui", "profiles"];

impl Config {
    /// Default config path: `$XDG_CONFIG_HOME/narvi/config.toml`.
    pub fn default_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("dev", "zer0dot", "narvi")
            .ok_or_else(|| crate::Error::Config("cannot resolve config dir".into()))?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Load + validate: warn on unknown top-level keys, clamp out-of-range params.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let table: toml::Table = text.parse()?;
        for key in table.keys() {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                log::warn!("config: unknown key `{key}` ignored");
            }
        }
        let mut cfg: Config = table.try_into()?;
        for p in &mut cfg.profiles {
            let clamped = p.params.clamped();
            if clamped != p.params {
                log::warn!(
                    "config: profile `{}` had out-of-range params; clamped",
                    p.name
                );
                p.params = clamped;
            }
        }
        Ok(cfg)
    }

    /// Persist atomically: write `.tmp`, fsync, rename over `path`.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Find a profile by name, case-insensitively.
    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

/// Expand a leading `~/` to `$HOME`. Paths in config are user-written.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_clamp() {
        let dir = std::env::temp_dir().join("narvi-core-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let mut cfg = Config::default();
        cfg.profiles
            .push(Profile::new("default", Default::default()));
        cfg.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.general.active_profile, "default");
        assert_eq!(loaded.profiles.len(), 1);

        // Out-of-range values in the file clamp on load.
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("vibrance = 1.0", "vibrance = 9.0");
        std::fs::write(&path, text).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.profiles[0].params.vibrance, 2.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn profile_lookup_is_case_insensitive() {
        let mut cfg = Config::default();
        cfg.profiles
            .push(Profile::new("Gaming", Default::default()));
        assert!(cfg.profile("gaming").is_some());
        assert!(cfg.profile("nope").is_none());
    }
}
