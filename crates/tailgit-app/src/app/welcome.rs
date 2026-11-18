//! Welcome screen shown while no repository is open.

use std::path::PathBuf;

use eframe::egui::{self, RichText};

use super::TailApp;

impl TailApp {
    pub(super) fn welcome(&mut self, ui: &mut egui::Ui) {
        use egui::{Align2, CornerRadius, FontId, Frame, Margin, Rect, Sense, Stroke, UiBuilder, Vec2};
        let mut open: Option<PathBuf> = None;
        let dark = ui.visuals().dark_mode;
        let accent = crate::theme::sys::blue(dark);
        egui::CentralPanel::default().show(ui, |ui| {
            // Whole background drags the (title-bar-less) window.
            let bg = ui.interact(ui.max_rect(), egui::Id::unique("welcome_drag"), Sense::drag());
            if bg.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            let size = Vec2::new(760.0, 440.0).min(ui.available_size() - Vec2::splat(32.0));
            let card = Rect::from_center_size(ui.max_rect().center(), size);
            let v = ui.visuals().clone();
            ui.scope_builder(UiBuilder::new().max_rect(card), |ui| {
                Frame::new()
                    .fill(v.window_fill)
                    .stroke(v.window_stroke)
                    .corner_radius(CornerRadius::same(12))
                    .shadow(v.window_shadow)
                    .show(ui, |ui| {
                        ui.set_min_size(card.size());
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            // ── Left: identity + primary actions ──
                            let left_w = 340.0;
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, card.height()),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.set_width(left_w);
                                    ui.add_space(44.0);
                                    // App icon.
                                    let tex = crate::brand::texture(ui.ctx());
                                    let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
                                    ui.painter().image(
                                        tex.id(),
                                        r,
                                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                        egui::Color32::WHITE,
                                    );
                                    ui.add_space(18.0);
                                    ui.label(RichText::new("TailGIT").size(26.0).strong());
                                    ui.add_space(2.0);
                                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).small().weak());
                                    ui.add_space(32.0);

                                    let primary = egui::Button::new(RichText::new("Open Repository…").size(13.0).color(egui::Color32::WHITE))
                                        .min_size(Vec2::new(240.0, 28.0))
                                        .fill(accent)
                                        .stroke(Stroke::NONE);
                                    if ui.add(primary)
                                        .clicked()
                                    {
                                        open = rfd::FileDialog::new().pick_folder();
                                    }
                                    ui.add_space(10.0);
                                    let field = ui.add(
                                        egui::TextEdit::singleline(&mut self.path_input)
                                            .hint_text("Or enter a path and press Return")
                                            .desired_width(240.0)
                                            .margin(Margin::symmetric(8, 5)),
                                    );
                                    if field.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                        && !self.path_input.trim().is_empty()
                                    {
                                        open = Some(PathBuf::from(self.path_input.trim()));
                                    }
                                    if let Some(e) = &self.error {
                                        ui.add_space(8.0);
                                        ui.add_sized(
                                            [240.0, 0.0],
                                            egui::Label::new(RichText::new(e).small().color(crate::theme::sys::red(dark))).wrap(),
                                        );
                                    }
                                },
                            );

                            // ── Right: recent repositories (sidebar material) ──
                            let right = Rect::from_min_max(
                                egui::pos2(card.left() + left_w, card.top()),
                                card.right_bottom(),
                            );
                            let side = if dark { egui::Color32::from_white_alpha(6) } else { egui::Color32::from_black_alpha(10) };
                            ui.painter().rect_filled(
                                right,
                                CornerRadius { nw: 0, sw: 0, ne: 12, se: 12 },
                                side,
                            );
                            ui.painter().vline(right.left(), right.y_range(), Stroke::new(1.0, v.window_stroke.color));
                            ui.scope_builder(UiBuilder::new().max_rect(right.shrink2(Vec2::new(12.0, 16.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                                ui.label(RichText::new("  Recent Repositories").small().strong().weak());
                                ui.add_space(6.0);
                                if self.settings.recent.is_empty() {
                                    ui.add_space(120.0);
                                    ui.vertical_centered(|ui| ui.weak("No recent repositories"));
                                    return;
                                }
                                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                    for r in self.settings.recent.clone() {
                                        let path = std::path::Path::new(&r);
                                        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(r.clone());
                                        let home = std::env::var("HOME").unwrap_or_default();
                                        let shown = if !home.is_empty() && r.starts_with(&home) { format!("~{}", &r[home.len()..]) } else { r.clone() };
                                        let w = ui.available_width();
                                        let (row, resp) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::click());
                                        let p = ui.painter();
                                        if resp.is_pointer_button_down_on() {
                                            p.rect_filled(row, 8.0, accent);
                                        } else if resp.hovered() {
                                            p.rect_filled(row, 8.0, ui.visuals().widgets.hovered.bg_fill);
                                        }
                                        let pressed = resp.is_pointer_button_down_on();
                                        let icon = row.left_center() + Vec2::new(20.0, 0.0);
                                        p.text(icon, Align2::CENTER_CENTER, "\u{f07b}", FontId::proportional(18.0), if pressed { egui::Color32::WHITE } else { accent });
                                        let tx = row.left() + 40.0;
                                        let fg = if pressed { egui::Color32::WHITE } else { ui.visuals().strong_text_color() };
                                        let weak = if pressed { egui::Color32::from_white_alpha(200) } else { ui.visuals().weak_text_color() };
                                        let max_w = (right.right() - 22.0).min(row.right()) - tx;
                                        let line = |text: &str, size: f32, color: egui::Color32| {
                                            let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(size), color);
                                            job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
                                            ui.fonts_mut(|f| f.layout_job(job))
                                        };
                                        let g1 = line(&name, 13.0, fg);
                                        let g2 = line(&shown, 11.0, weak);
                                        p.galley(egui::pos2(tx, row.top() + 13.0 - g1.size().y / 2.0), g1, fg);
                                        p.galley(egui::pos2(tx, row.top() + 30.0 - g2.size().y / 2.0), g2, weak);
                                        if resp.on_hover_text(&r).clicked() {
                                            open = Some(PathBuf::from(r));
                                        }
                                    }
                                });
                            });
                        });
                    });
            });
        });
        if let Some(p) = open {
            self.open(p, ui.ctx());
        }
    }
}
