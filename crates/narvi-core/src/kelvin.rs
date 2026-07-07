//! Kelvin → linear white-balance gain (Tanner Helland approximation).
//!
//! Normalized so 6500 K maps to `[1.0, 1.0, 1.0]` (neutral).

/// Raw Tanner Helland curve: Kelvin → RGB in [0, 255].
fn raw(kelvin: u32) -> [f64; 3] {
    let t = kelvin as f64 / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698727446 * (t - 60.0).powf(-0.1332047592)
    };
    let g = if t <= 66.0 {
        99.4708025861 * t.ln() - 161.1195681661
    } else {
        288.1221695283 * (t - 60.0).powf(-0.0755148492)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    [
        r.clamp(0.0, 255.0),
        g.clamp(0.0, 255.0),
        b.clamp(0.0, 255.0),
    ]
}

/// Convert a color temperature (Kelvin) to per-channel white-balance gain,
/// normalized to neutral at 6500 K. Result components are >= 0.0.
pub fn kelvin_to_rgb(kelvin: u32) -> [f32; 3] {
    let v = raw(kelvin);
    let n = raw(6500);
    [
        (v[0] / n[0]) as f32,
        (v[1] / n[1]) as f32,
        (v[2] / n[2]) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_at_6500k() {
        assert_eq!(kelvin_to_rgb(6500), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn warm_at_4000k() {
        let [r, _, b] = kelvin_to_rgb(4000);
        assert!(r > b, "4000 K must be warm (R > B): r={r} b={b}");
    }

    #[test]
    fn cool_at_10000k() {
        let [r, _, b] = kelvin_to_rgb(10000);
        assert!(b > r, "10000 K must be cool (B > R): r={r} b={b}");
    }

    #[test]
    fn extremes_finite_and_nonnegative() {
        for k in [1000, 1900, 2000, 6600, 10000] {
            for c in kelvin_to_rgb(k) {
                assert!(c.is_finite() && c >= 0.0, "{k} K produced {c}");
            }
        }
        assert_eq!(kelvin_to_rgb(1000)[2], 0.0); // blue cuts off below 2000 K
    }
}
