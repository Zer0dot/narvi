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
    pub vibrance: f32,
    pub saturation: f32,
    pub temperature: u32,
    pub brightness: f32,
    pub contrast: f32,
    pub gamma: f32,
    pub rgb: [f32; 3],
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
