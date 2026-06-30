//! Kelvin → linear white-balance gain (Tanner Helland approximation).
//!
//! Normalized so 6500 K maps to `[1.0, 1.0, 1.0]` (neutral). Implemented in M1.

/// Convert a color temperature (Kelvin) to per-channel white-balance gain,
/// normalized to neutral at 6500 K. Result components are >= 0.0.
pub fn kelvin_to_rgb(_kelvin: u32) -> [f32; 3] {
    // M1: Tanner Helland curve, clamp [0,255], divide by value at 6500 K.
    todo!("M1: implement Tanner Helland kelvin_to_rgb")
}
