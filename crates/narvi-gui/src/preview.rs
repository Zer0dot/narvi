//! Live preview: a reference image run through the REAL pipeline math
//! (`narvi_core::pipeline`), so the swatch matches actual screen output.

use eframe::egui::{self, Color32, ColorImage};
use narvi_core::{ColorParams, pipeline::apply_pipeline};

pub const W: usize = 288;
pub const H: usize = 180;

pub struct Preview {
    base: Vec<[f32; 3]>, // linear-ish 0..1 source pixels
    texture: Option<egui::TextureHandle>,
    last: Option<ColorParams>,
}

impl Preview {
    /// `path`: optional `[ui].preview_image` override; falls back to the
    /// procedural sample (sky / foliage / skin tones / gray + hue ramps).
    pub fn new(path: Option<&str>, gradient_only: bool) -> Self {
        let base = path
            .and_then(load_image)
            .unwrap_or_else(|| if gradient_only { gradient() } else { sample() });
        Self {
            base,
            texture: None,
            last: None,
        }
    }

    /// Recompute the texture when params change; cheap (`W*H` pixels).
    pub fn texture(&mut self, ctx: &egui::Context, params: &ColorParams) -> &egui::TextureHandle {
        if self.last.as_ref() != Some(params) || self.texture.is_none() {
            let mut img = ColorImage::new([W, H], Color32::BLACK);
            for (i, px) in self.base.iter().enumerate() {
                let c = apply_pipeline(params, *px);
                img.pixels[i] = Color32::from_rgb(
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                );
            }
            self.texture = Some(ctx.load_texture("narvi-preview", img, Default::default()));
            self.last = Some(*params);
        }
        self.texture.as_ref().unwrap_or_else(|| unreachable!())
    }
}

fn load_image(path: &str) -> Option<Vec<[f32; 3]>> {
    let bytes = std::fs::read(narvi_core::config::expand_tilde(path)).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgb8();
    let img = image::imageops::resize(
        &img,
        W as u32,
        H as u32,
        image::imageops::FilterType::Triangle,
    );
    Some(
        img.pixels()
            .map(|p| {
                [
                    p.0[0] as f32 / 255.0,
                    p.0[1] as f32 / 255.0,
                    p.0[2] as f32 / 255.0,
                ]
            })
            .collect(),
    )
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Plain hue/lightness gradient (preview = "gradient").
fn gradient() -> Vec<[f32; 3]> {
    let mut px = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let h = x as f32 / W as f32 * 360.0;
            let l = 0.15 + 0.7 * (1.0 - y as f32 / H as f32);
            px.push(hsl(h, 0.8, l));
        }
    }
    px
}

/// Procedural reference scene: sky band, sun glow, foliage, earth,
/// skin-tone patches, gray ramp and hue sweep.
fn sample() -> Vec<[f32; 3]> {
    let mut px = vec![[0.0f32; 3]; W * H];
    let fw = W as f32;
    let fh = H as f32;

    for y in 0..H {
        let fy = y as f32 / fh;
        for x in 0..W {
            let fx = x as f32 / fw;
            let mut c;
            if fy < 0.45 {
                // Sky: zenith blue → warm horizon.
                c = lerp([0.16, 0.32, 0.62], [0.72, 0.62, 0.52], fy / 0.45);
                let d = ((fx - 0.72).powi(2) * 2.2 + (fy - 0.30).powi(2)).sqrt();
                let sun = (1.0 - (d / 0.22)).clamp(0.0, 1.0);
                c = lerp(c, [1.0, 0.85, 0.55], sun * sun);
            } else if fy < 0.62 {
                // Foliage band with variation.
                let v = ((x * 7 + y * 13) % 17) as f32 / 17.0;
                c = lerp([0.10, 0.30, 0.10], [0.30, 0.48, 0.16], v);
            } else {
                // Earth / wood.
                let v = ((x * 5 + y * 3) % 13) as f32 / 13.0;
                c = lerp([0.28, 0.18, 0.10], [0.45, 0.32, 0.18], v);
            }
            px[y * W + x] = c;
        }
    }

    // Skin-tone patches (light → deep), right edge.
    let skins = [
        [0.96, 0.80, 0.68],
        [0.87, 0.67, 0.53],
        [0.72, 0.51, 0.38],
        [0.55, 0.37, 0.26],
        [0.38, 0.26, 0.18],
    ];
    let pw = W / 8;
    let ph = (H as f32 * 0.62) as usize / skins.len();
    for (i, s) in skins.iter().enumerate() {
        for y in i * ph..(i + 1) * ph {
            for x in W - pw..W {
                px[y * W + x] = *s;
            }
        }
    }

    // Bottom strips: gray ramp + hue sweep (banding/clipping made visible).
    let strip = H / 12;
    for y in H - 2 * strip..H - strip {
        for x in 0..W {
            let g = x as f32 / fw;
            px[y * W + x] = [g, g, g];
        }
    }
    for y in H - strip..H {
        for x in 0..W {
            px[y * W + x] = hsl(x as f32 / fw * 360.0, 0.85, 0.5);
        }
    }
    px
}

fn hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}
