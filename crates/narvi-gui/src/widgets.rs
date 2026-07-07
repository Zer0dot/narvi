//! Custom Saturn widgets: thin line slider + numeric field, pill buttons.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::theme;

/// Thin-track slider with a round handle and an editable numeric field.
/// Returns true if the value changed this frame.
pub fn line_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
    accent: Color32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            [118.0, 22.0],
            egui::Label::new(
                egui::RichText::new(theme::tracked(label))
                    .size(13.0)
                    .color(theme::TEXT_MUTED),
            ),
        );

        // Numeric field on the right, slider fills the middle.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let decimals = if step >= 1.0 { 0 } else { 2 }; // Kelvin is integer
            let field = egui::DragValue::new(value)
                .speed(step)
                .range(range.clone())
                .min_decimals(decimals)
                .max_decimals(decimals);
            changed |= ui.add_sized([64.0, 22.0], field).changed();
            changed |= track(ui, value, range, accent);
        });
    });
    changed
}

fn track(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    accent: Color32,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let desired = Vec2::new(ui.available_width(), 18.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click_and_drag());
    let mut changed = false;

    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let new = lo + t * (hi - lo);
            if new != *value {
                *value = new;
                changed = true;
            }
        }
    }

    let painter = ui.painter();
    let y = rect.center().y;
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let x = rect.left() + t * rect.width();
    painter.line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(2.0, theme::BORDER),
    );
    painter.line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(x, y)],
        Stroke::new(2.0, accent.linear_multiply(0.6)),
    );
    let hot = resp.hovered() || resp.dragged();
    let color = if hot { theme::ACCENT_GLOW } else { accent };
    painter.circle_filled(Pos2::new(x, y), if hot { 6.0 } else { 5.0 }, color);
    changed
}

/// Primary: filled accent with navy text. Secondary: outlined pill, accent text.
pub fn button(ui: &mut egui::Ui, label: &str, accent: Color32, primary: bool) -> egui::Response {
    let text = egui::RichText::new(theme::tracked(label)).size(11.0);
    let b = if primary {
        egui::Button::new(text.color(theme::BG).strong()).fill(accent)
    } else {
        egui::Button::new(text.color(accent))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, theme::BORDER))
    };
    ui.add(b.corner_radius(10.0))
}

/// Small filled status dot.
pub fn status_dot(ui: &mut egui::Ui, on: bool, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    let color = if on { accent } else { theme::TEXT_DIM };
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

/// Card frame with bracket corners; returns the inner response.
pub fn bracket_panel<R>(
    ui: &mut egui::Ui,
    accent: Color32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let frame = egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::same(14));
    let out = frame.show(ui, add);
    let rect: Rect = out.response.rect;
    theme::bracket_corners(ui.painter(), rect.shrink(2.0), accent.linear_multiply(0.8));
    out.inner
}
