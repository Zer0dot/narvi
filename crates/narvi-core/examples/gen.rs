//! Print a generated shader: `cargo run -p narvi-core --example gen [vibrance]`.
fn main() {
    let v: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let p = narvi_core::ColorParams {
        vibrance: v,
        ..Default::default()
    };
    print!("{}", narvi_core::shader::render_shader(&p));
}
