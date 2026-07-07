//! `narvi-gui` — egui dashboard. Subscribes to the daemon so external changes
//! move the sliders; slider drags apply live (throttled).

mod conn;
mod preview;
mod theme;
mod widgets;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32};
use narvi_core::proto::Command;
use narvi_core::{ColorParams, Config, Param, params::range};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Narvi")
            .with_app_id("narvi-gui")
            .with_inner_size([840.0, 480.0])
            .with_min_inner_size([700.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native("narvi-gui", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

const SEND_EVERY: Duration = Duration::from_millis(45);

struct App {
    conn: conn::Conn,
    preview: preview::Preview,
    accent: Color32,
    params: ColorParams,
    enabled: bool,
    active_profile: Option<String>,
    profiles: Vec<String>,
    error: Option<String>,
    save_name: String,
    seen_stamp: u64,
    dirty: bool,
    last_sent: Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let ui_cfg = Config::default_path()
            .ok()
            .filter(|p| p.exists())
            .and_then(|p| Config::load(&p).ok())
            .map(|c| c.ui)
            .unwrap_or_default();
        let preview = preview::Preview::new(
            (!ui_cfg.preview_image.is_empty()).then_some(ui_cfg.preview_image.as_str()),
            ui_cfg.preview == "gradient",
        );
        Self {
            conn: conn::Conn::spawn(cc.egui_ctx.clone()),
            preview,
            accent: theme::hex(&ui_cfg.accent),
            params: ColorParams::default(),
            enabled: true,
            active_profile: None,
            profiles: Vec::new(),
            error: None,
            save_name: String::new(),
            seen_stamp: 0,
            dirty: false,
            last_sent: Instant::now(),
        }
    }

    /// Pull daemon-side changes into the sliders unless we have unsent edits.
    fn sync_from_daemon(&mut self) {
        let Ok(shared) = self.conn.shared.lock() else {
            return;
        };
        self.error = shared.error.clone();
        self.profiles = shared.profiles.clone();
        if shared.stamp != self.seen_stamp {
            self.seen_stamp = shared.stamp;
            if let Some(st) = &shared.status
                && !self.dirty
            {
                self.params = st.params;
                self.enabled = st.enabled;
                self.active_profile = st.active_profile.clone();
                if let Some(name) = &st.active_profile {
                    self.save_name = name.clone();
                }
            }
        }
    }

    /// Throttled live apply while dragging.
    fn flush(&mut self, ctx: &egui::Context) {
        if self.dirty && self.last_sent.elapsed() >= SEND_EVERY {
            self.conn.send(Command::Apply {
                params: self.params,
            });
            self.active_profile = None;
            self.dirty = false;
            self.last_sent = Instant::now();
        }
        if self.dirty {
            ctx.request_repaint_after(SEND_EVERY); // flush trailing edit
        }
    }

    fn sliders(&mut self, ui: &mut egui::Ui) {
        let a = self.accent;
        let p = &mut self.params;
        let mut ch = false;
        ch |= widgets::line_slider(
            ui,
            "Vibrance",
            &mut p.vibrance,
            range_of(range::VIBRANCE),
            0.01,
            a,
        );
        ch |= widgets::line_slider(
            ui,
            "Saturation",
            &mut p.saturation,
            range_of(range::SATURATION),
            0.01,
            a,
        );

        let mut temp = p.temperature as f32;
        if widgets::line_slider(ui, "Temp K", &mut temp, 1000.0..=10000.0, 50.0, a) {
            p.set(Param::Temperature, temp);
            ch = true;
        }

        ch |= widgets::line_slider(
            ui,
            "Brightness",
            &mut p.brightness,
            range_of(range::BRIGHTNESS),
            0.01,
            a,
        );
        ch |= widgets::line_slider(
            ui,
            "Contrast",
            &mut p.contrast,
            range_of(range::CONTRAST),
            0.01,
            a,
        );
        ch |= widgets::line_slider(ui, "Gamma", &mut p.gamma, range_of(range::GAMMA), 0.01, a);
        ui.add_space(4.0);
        ch |= widgets::line_slider(ui, "Red", &mut p.rgb[0], range_of(range::RGB), 0.01, a);
        ch |= widgets::line_slider(ui, "Green", &mut p.rgb[1], range_of(range::RGB), 0.01, a);
        ch |= widgets::line_slider(ui, "Blue", &mut p.rgb[2], range_of(range::RGB), 0.01, a);

        if ch {
            *p = p.clamped();
            self.dirty = true;
        }
    }

    fn left_column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let a = self.accent;
        widgets::bracket_panel(ui, a, |ui| {
            ui.label(
                egui::RichText::new(theme::tracked("Live Preview"))
                    .size(10.5)
                    .color(theme::TEXT_DIM),
            );
            ui.add_space(4.0);
            let tex = self.preview.texture(ctx, &self.params);
            let w = ui.available_width().min(300.0);
            ui.image((tex.id(), egui::vec2(w, w * 0.625)));
        });

        ui.add_space(8.0);
        let sel = self
            .active_profile
            .clone()
            .unwrap_or_else(|| "unsaved".into());
        egui::ComboBox::from_id_salt("profile")
            .selected_text(theme::tracked(&sel))
            .width(200.0)
            .show_ui(ui, |ui| {
                let names = self.profiles.clone();
                for name in names {
                    if ui
                        .selectable_label(self.active_profile.as_deref() == Some(&name), &name)
                        .clicked()
                    {
                        self.conn.send(Command::ProfileLoad { name });
                    }
                }
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Reset", a, false).clicked() {
                match &self.active_profile {
                    Some(name) => self.conn.send(Command::ProfileLoad { name: name.clone() }),
                    None => self.conn.send(Command::Apply {
                        params: ColorParams::default(),
                    }),
                }
            }
            ui.add_sized(
                [110.0, 24.0],
                egui::TextEdit::singleline(&mut self.save_name),
            );
            if widgets::button(ui, "Save", a, true).clicked() && !self.save_name.is_empty() {
                self.conn.send(Command::Apply {
                    params: self.params,
                });
                self.conn.send(Command::ProfileSave {
                    name: self.save_name.clone(),
                    params: None,
                });
            }
        });
    }
}

fn range_of((lo, hi): (f32, f32)) -> std::ops::RangeInclusive<f32> {
    lo..=hi
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_from_daemon();

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ctx, |ui| {
                theme::background_flourish(ui.painter(), ui.max_rect(), self.accent);
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    widgets::status_dot(ui, self.enabled, self.accent);
                    ui.heading(
                        egui::RichText::new(theme::tracked("Narvi"))
                            .color(theme::TEXT)
                            .size(17.0),
                    );
                    ui.label(
                        egui::RichText::new(theme::tracked("// color control"))
                            .size(10.5)
                            .color(theme::TEXT_MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(14.0);
                        let label = if self.enabled { "On" } else { "Off" };
                        if widgets::button(ui, label, self.accent, self.enabled).clicked() {
                            self.conn.send(Command::Toggle);
                        }
                        if let Some(err) = &self.error {
                            ui.label(
                                egui::RichText::new(err)
                                    .size(10.5)
                                    .color(Color32::from_rgb(0xC0, 0x60, 0x50)),
                            );
                        }
                    });
                });
                ui.add_space(8.0);

                ui.horizontal_top(|ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.set_width(320.0);
                        self.left_column(ui, ctx);
                    });
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 20.0);
                        widgets::bracket_panel(ui, self.accent, |ui| {
                            self.sliders(ui);
                        });
                    });
                });
            });

        self.flush(ctx);
    }
}
