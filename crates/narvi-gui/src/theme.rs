//! Saturn theme: deep navy + amber-gold, monospace, bracket-corner frames. See THEME.md.

use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Stroke, TextStyle, Vec2};

pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0E, 0x14);
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x17, 0x22);
pub const CARD: Color32 = Color32::from_rgb(0x1A, 0x21, 0x30);
pub const BORDER: Color32 = Color32::from_rgb(0x2A, 0x34, 0x47);
pub const ACCENT: Color32 = Color32::from_rgb(0xE0, 0xA2, 0x3C);
pub const ACCENT_GLOW: Color32 = Color32::from_rgb(0xF0, 0xB8, 0x60);
pub const TEXT: Color32 = Color32::from_rgb(0xED, 0xE8, 0xDC);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8A, 0x93, 0xA6);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x5C, 0x66, 0x78);

/// Parse `#RRGGBB` (the `[ui].accent` override) to a color.
pub fn hex(s: &str) -> Color32 {
    let h = s.trim_start_matches('#');
    match u32::from_str_radix(h, 16) {
        Ok(n) if h.len() == 6 => Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8),
        _ => ACCENT,
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = BG;
    v.extreme_bg_color = BG;
    v.faint_bg_color = CARD;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.hovered.bg_fill = CARD;
    v.widgets.hovered.weak_bg_fill = CARD;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.bg_fill = CARD;
    v.widgets.active.weak_bg_fill = CARD;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_GLOW);
    ctx.set_visuals(v);

    // Monospace throughout.
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Monospace)),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Monospace)),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
        (TextStyle::Button, FontId::new(13.0, FontFamily::Monospace)),
        (TextStyle::Small, FontId::new(10.5, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 5.0);
    ctx.set_style(style);
}

/// UPPERCASE with hair-space tracking (egui has no letter-spacing).
pub fn tracked(s: &str) -> String {
    let up = s.to_uppercase();
    let mut out = String::with_capacity(up.len() * 2);
    for (i, ch) in up.chars().enumerate() {
        if i > 0 {
            out.push('\u{200A}');
        }
        out.push(ch);
    }
    out
}

/// Bracket corners `⌜ ⌝ ⌞ ⌟` drawn at the four corners of `rect`.
pub fn bracket_corners(painter: &egui::Painter, rect: Rect, color: Color32) {
    let len = 10.0;
    let s = Stroke::new(1.5, color);
    let c = [
        (rect.left_top(), Vec2::X, Vec2::Y),
        (rect.right_top(), -Vec2::X, Vec2::Y),
        (rect.left_bottom(), Vec2::X, -Vec2::Y),
        (rect.right_bottom(), -Vec2::X, -Vec2::Y),
    ];
    for (corner, dx, dy) in c {
        painter.line_segment([corner, corner + dx * len], s);
        painter.line_segment([corner, corner + dy * len], s);
    }
}

/// Faint radial glow orb + thin orbital ring, behind content.
pub fn background_flourish(painter: &egui::Painter, rect: Rect, accent: Color32) {
    let center = Pos2::new(
        rect.right() - rect.width() * 0.22,
        rect.top() + rect.height() * 0.25,
    );
    for i in 0..14 {
        let t = i as f32 / 14.0;
        let alpha = (4.0 * (1.0 - t)) as u8;
        painter.circle_filled(
            center,
            30.0 + t * 150.0,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
        );
    }
    // Orbital ring: a wide, flat ellipse of short line segments.
    let ring = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 14);
    let (rx, ry) = (rect.width() * 0.55, rect.height() * 0.16);
    let mut prev: Option<Pos2> = None;
    for i in 0..=72 {
        let a = i as f32 / 72.0 * std::f32::consts::TAU;
        let p = Pos2::new(center.x + rx * a.cos(), center.y + ry * a.sin() + 40.0);
        if let Some(q) = prev {
            painter.line_segment([q, p], Stroke::new(1.0, ring));
        }
        prev = Some(p);
    }
}
