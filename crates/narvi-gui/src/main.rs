//! `narvi-gui` — egui dashboard. Subscribes to the daemon so external changes move sliders.
//!
//! M5: live-preview swatch (real pipeline math on a sample image), line-sliders + numeric
//! fields per control, profile picker, Reset/Save. Saturn theme (THEME.md).

mod theme;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Narvi")
            .with_inner_size([720.0, 460.0]),
        ..Default::default()
    };
    eframe::run_native("narvi-gui", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

struct App;

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        Self
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("NARVI");
            ui.label("GUI not yet implemented (M5).");
        });
    }
}
