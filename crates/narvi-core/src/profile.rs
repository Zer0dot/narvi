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
pub fn builtin_presets() -> Vec<Profile> {
    let p = |vibrance, saturation, temperature, brightness, contrast, gamma| ColorParams {
        vibrance,
        saturation,
        temperature,
        brightness,
        contrast,
        gamma,
        rgb: [1.0, 1.0, 1.0],
    };
    let mut gaming = Profile::new("Gaming", p(1.5, 1.1, 6500, 1.0, 1.05, 1.0));
    gaming.r#match = vec!["steam_app_*".into(), "gamescope".into()];
    let mut movie = Profile::new("Movie", p(1.2, 1.05, 6000, 1.0, 1.1, 1.0));
    movie.r#match = vec!["mpv".into()];
    vec![
        Profile::new("Default", p(1.25, 1.0, 6500, 1.0, 1.0, 1.0)),
        gaming,
        movie,
        Profile::new("Photo", ColorParams::default()),
        Profile::new("Night", p(1.1, 1.0, 3800, 0.95, 1.0, 1.0)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_seeded_and_in_range() {
        let presets = builtin_presets();
        let names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Default", "Gaming", "Movie", "Photo", "Night"]);
        for p in &presets {
            assert_eq!(p.params, p.params.clamped(), "{} out of range", p.name);
        }
    }
}
