//! Saturn theme: deep navy + amber-gold, monospace, bracket-corner frames. See THEME.md.
//!
//! M5: full palette, custom slider, bracket corners, glow orbs via `egui::Painter`.

use eframe::egui;

/// Parse `#RRGGBB` to an egui color.
pub fn hex(s: &str) -> egui::Color32 {
    let h = s.trim_start_matches('#');
    let n = u32::from_str_radix(h, 16).unwrap_or(0);
    egui::Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

/// Apply the Saturn visuals to the context. Expanded in M5.
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = hex("#121722");
    v.window_fill = hex("#0B0E14");
    v.extreme_bg_color = hex("#0B0E14");
    v.faint_bg_color = hex("#1A2130");
    v.override_text_color = Some(hex("#EDE8DC"));
    v.hyperlink_color = hex("#E0A23C");
    ctx.set_visuals(v);
}
