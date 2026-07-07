//! CPU mirror of the shader pipeline, for the GUI live preview.
//!
//! MUST match `shaders/narvi.frag` op-for-op so the preview equals screen output.

use crate::kelvin::kelvin_to_rgb;
use crate::params::ColorParams;

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722]; // Rec.709

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Process one RGB pixel (0..1) through the full pipeline.
pub fn apply_pipeline(p: &ColorParams, rgb: [f32; 3]) -> [f32; 3] {
    let wb = kelvin_to_rgb(p.temperature);
    let mut c = rgb;

    for i in 0..3 {
        c[i] *= p.brightness;
        c[i] = (c[i] - 0.5) * p.contrast + 0.5;
        c[i] *= wb[i];
        c[i] = c[i].max(0.0).powf(1.0 / p.gamma);
        c[i] *= p.rgb[i];
    }

    let luma = dot(c, LUMA);
    for i in 0..3 {
        c[i] = luma + (c[i] - luma) * p.saturation;
    }

    // Protective vibrance: boost muted colors, spare saturated + skin (red-dominant).
    let luma = dot(c, LUMA);
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let sat = mx - mn;
    let mut boost = (p.vibrance - 1.0) * (1.0 - smoothstep(0.0, 1.0, sat));
    let red_dominant = c[1].max(c[2]) <= c[0];
    boost *= if red_dominant { 0.5 } else { 1.0 };
    for i in 0..3 {
        c[i] = luma + (c[i] - luma) * (1.0 + boost);
        c[i] = c[i].clamp(0.0, 1.0);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_identity() {
        let p = ColorParams::default();
        for px in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.3, 0.6, 0.9]] {
            let out = apply_pipeline(&p, px);
            for i in 0..3 {
                assert!((out[i] - px[i]).abs() < 1e-5, "{px:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn vibrance_boosts_muted_more_than_vivid() {
        let p = ColorParams {
            vibrance: 1.5,
            ..Default::default()
        };
        let muted = [0.45, 0.5, 0.55];
        let vivid = [0.05, 0.5, 0.95];
        let m = apply_pipeline(&p, muted);
        let v = apply_pipeline(&p, vivid);
        let spread = |c: [f32; 3]| c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2]);
        let m_gain = spread(m) - spread(muted);
        let v_gain = spread(v) - spread(vivid);
        assert!(m_gain > v_gain, "muted +{m_gain}, vivid +{v_gain}");
    }

    #[test]
    fn warm_temperature_tints_red() {
        let p = ColorParams {
            temperature: 4000,
            ..Default::default()
        };
        let out = apply_pipeline(&p, [0.5, 0.5, 0.5]);
        assert!(out[0] > out[2], "warm should tint red over blue: {out:?}");
    }
}
