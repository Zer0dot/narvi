//! `ColorParams` — the single source of truth for color state.
//!
//! Ranges are fixed by PROTOCOL.md. All mutation clamps here, never per-caller.

use serde::{Deserialize, Serialize};

/// Inclusive valid range for each param. Defaults are all-neutral.
pub mod range {
    pub const VIBRANCE: (f32, f32) = (0.0, 2.0);
    pub const SATURATION: (f32, f32) = (0.0, 2.0);
    pub const TEMPERATURE: (u32, u32) = (1000, 10000);
    pub const BRIGHTNESS: (f32, f32) = (0.5, 1.5);
    pub const CONTRAST: (f32, f32) = (0.5, 2.0);
    pub const GAMMA: (f32, f32) = (0.5, 2.0);
    pub const RGB: (f32, f32) = (0.0, 2.0);
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ColorParams {
    #[serde(serialize_with = "ser_f32")]
    pub vibrance: f32,
    #[serde(serialize_with = "ser_f32")]
    pub saturation: f32,
    pub temperature: u32,
    #[serde(serialize_with = "ser_f32")]
    pub brightness: f32,
    #[serde(serialize_with = "ser_f32")]
    pub contrast: f32,
    #[serde(serialize_with = "ser_f32")]
    pub gamma: f32,
    #[serde(serialize_with = "ser_rgb")]
    pub rgb: [f32; 3],
}

/// f32 → f64 via the shortest decimal repr, so JSON/TOML show `1.3`,
/// not `1.2999999523162842` (serde converts f32 through `as f64`).
fn clean(v: f32) -> f64 {
    v.to_string().parse().unwrap_or(v as f64)
}

fn ser_f32<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(clean(*v))
}

fn ser_rgb<S: serde::Serializer>(v: &[f32; 3], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = s.serialize_seq(Some(3))?;
    for c in v {
        seq.serialize_element(&clean(*c))?;
    }
    seq.end()
}

impl Default for ColorParams {
    fn default() -> Self {
        Self {
            vibrance: 1.0,
            saturation: 1.0,
            temperature: 6500,
            brightness: 1.0,
            contrast: 1.0,
            gamma: 1.0,
            rgb: [1.0, 1.0, 1.0],
        }
    }
}

/// Addressable params, matching the CLI/protocol names (`r`/`g`/`b` index `rgb`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Param {
    Vibrance,
    Saturation,
    Temperature,
    Brightness,
    Contrast,
    Gamma,
    R,
    G,
    B,
}

impl std::str::FromStr for Param {
    type Err = crate::Error;
    fn from_str(s: &str) -> crate::Result<Self> {
        Ok(match s {
            "vibrance" => Param::Vibrance,
            "saturation" => Param::Saturation,
            "temperature" => Param::Temperature,
            "brightness" => Param::Brightness,
            "contrast" => Param::Contrast,
            "gamma" => Param::Gamma,
            "r" => Param::R,
            "g" => Param::G,
            "b" => Param::B,
            other => return Err(crate::Error::UnknownParam(other.to_string())),
        })
    }
}

impl ColorParams {
    /// Set one param to an absolute value, clamped to its range.
    pub fn set(&mut self, param: Param, value: f32) {
        match param {
            Param::Vibrance => self.vibrance = clamp(value, range::VIBRANCE),
            Param::Saturation => self.saturation = clamp(value, range::SATURATION),
            Param::Temperature => {
                let (lo, hi) = range::TEMPERATURE;
                self.temperature = (value.round() as i64).clamp(lo as i64, hi as i64) as u32;
            }
            Param::Brightness => self.brightness = clamp(value, range::BRIGHTNESS),
            Param::Contrast => self.contrast = clamp(value, range::CONTRAST),
            Param::Gamma => self.gamma = clamp(value, range::GAMMA),
            Param::R => self.rgb[0] = clamp(value, range::RGB),
            Param::G => self.rgb[1] = clamp(value, range::RGB),
            Param::B => self.rgb[2] = clamp(value, range::RGB),
        }
    }

    /// Current value of one param as `f32` (temperature widened).
    pub fn get(&self, param: Param) -> f32 {
        match param {
            Param::Vibrance => self.vibrance,
            Param::Saturation => self.saturation,
            Param::Temperature => self.temperature as f32,
            Param::Brightness => self.brightness,
            Param::Contrast => self.contrast,
            Param::Gamma => self.gamma,
            Param::R => self.rgb[0],
            Param::G => self.rgb[1],
            Param::B => self.rgb[2],
        }
    }

    /// Relative change, clamped. Temperature delta is in Kelvin.
    pub fn nudge(&mut self, param: Param, delta: f32) {
        self.set(param, self.get(param) + delta);
    }

    /// Linear blend between two param sets (`t` in 0..1), for day/night transitions.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let f = |x: f32, y: f32| x + (y - x) * t;
        Self {
            vibrance: f(a.vibrance, b.vibrance),
            saturation: f(a.saturation, b.saturation),
            temperature: f(a.temperature as f32, b.temperature as f32).round() as u32,
            brightness: f(a.brightness, b.brightness),
            contrast: f(a.contrast, b.contrast),
            gamma: f(a.gamma, b.gamma),
            rgb: [
                f(a.rgb[0], b.rgb[0]),
                f(a.rgb[1], b.rgb[1]),
                f(a.rgb[2], b.rgb[2]),
            ],
        }
        .clamped()
    }

    /// Clamp every field back into range (e.g. after loading untrusted config).
    pub fn clamped(mut self) -> Self {
        self.vibrance = clamp(self.vibrance, range::VIBRANCE);
        self.saturation = clamp(self.saturation, range::SATURATION);
        let (lo, hi) = range::TEMPERATURE;
        self.temperature = self.temperature.clamp(lo, hi);
        self.brightness = clamp(self.brightness, range::BRIGHTNESS);
        self.contrast = clamp(self.contrast, range::CONTRAST);
        self.gamma = clamp(self.gamma, range::GAMMA);
        for c in &mut self.rgb {
            *c = clamp(*c, range::RGB);
        }
        self
    }
}

fn clamp(v: f32, (lo, hi): (f32, f32)) -> f32 {
    v.clamp(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_clamps_to_range() {
        let mut p = ColorParams::default();
        p.set(Param::Vibrance, 5.0);
        assert_eq!(p.vibrance, 2.0);
        p.set(Param::Temperature, 99999.0);
        assert_eq!(p.temperature, 10000);
        p.set(Param::Brightness, 0.0);
        assert_eq!(p.brightness, 0.5);
        p.set(Param::B, -1.0);
        assert_eq!(p.rgb[2], 0.0);
    }

    #[test]
    fn nudge_is_relative_and_clamped() {
        let mut p = ColorParams::default();
        p.nudge(Param::Vibrance, 0.05);
        assert!((p.vibrance - 1.05).abs() < 1e-6);
        p.nudge(Param::Gamma, 10.0);
        assert_eq!(p.gamma, 2.0);
    }

    #[test]
    fn lerp_blends_and_hits_endpoints() {
        let day = ColorParams::default();
        let night = ColorParams {
            temperature: 3500,
            brightness: 0.9,
            ..Default::default()
        };
        assert_eq!(ColorParams::lerp(&day, &night, 0.0), day);
        assert_eq!(ColorParams::lerp(&day, &night, 1.0), night);
        let mid = ColorParams::lerp(&day, &night, 0.5);
        assert_eq!(mid.temperature, 5000);
        assert!((mid.brightness - 0.95).abs() < 1e-6);
    }

    #[test]
    fn clamped_repairs_loaded_values() {
        let p = ColorParams {
            contrast: 9.0,
            temperature: 12,
            ..Default::default()
        }
        .clamped();
        assert_eq!(p.contrast, 2.0);
        assert_eq!(p.temperature, 1000);
    }
}
