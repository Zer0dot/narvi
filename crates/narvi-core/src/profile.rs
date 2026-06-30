//! Named profiles and built-in presets.

use serde::{Deserialize, Serialize};

use crate::params::ColorParams;

/// A named color profile with optional window-class globs for auto-switch.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub name: String,
    #[serde(flatten)]
    pub params: ColorParams,
    /// Window classes (globs, e.g. `steam_app_*`) that auto-activate this profile.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub r#match: Vec<String>,
}

impl Profile {
    pub fn new(name: impl Into<String>, params: ColorParams) -> Self {
        Self {
            name: name.into(),
            params,
            r#match: Vec::new(),
        }
    }
}

/// Built-in presets seeded on first run: Default, Gaming, Movie, Photo, Night.
/// Implemented in M4.
pub fn builtin_presets() -> Vec<Profile> {
    todo!("M4: seed Default, Gaming, Movie, Photo, Night presets")
}
