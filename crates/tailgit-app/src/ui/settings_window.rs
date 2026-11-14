//! Preferences window (Cmd+,), modeled on Fork's General tab.

use eframe::egui::{self, RichText};

use crate::settings::{Appearance, Settings};

/// Egui temp-data keys read by the diff view.
pub const DIFF_FONT_SIZE: &str = "diff_font_size";
pub const DIFF_MARKS: &str = "diff_marks";

/// Pushes diff preferences to where the renderers read them.
pub fn publish(ctx: &egui::Context, s: &Settings) {
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::NULL.with(DIFF_FONT_SIZE), s.diff_font_size);
        d.insert_temp(egui::Id::NULL.with(DIFF_MARKS), s.diff_marks);
    });
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(130.0, 22.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(label);
        });
        ui.vertical(add);
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(2.0);
}

/// Shows the window; returns true when any setting changed (already saved).
pub fn show(ctx: &egui::Context, open: &mut bool, s: &mut Settings) -> bool {
    let before = (s.appearance, s.ui_font.clone(), s.ui_font_size, s.text_hinting, s.text_subpixel, s.ui_font_weight, s.diff_font_size, s.diff_marks, s.sort_topo, s.side_by_side, s.merge_auto_apply);
    egui::Window::new("Settings")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .show(ctx, |ui| {
            section(ui, "Appearance");
            row(ui, "Theme:", |ui| {
                egui::ComboBox::from_id_salt("appearance")
                    .selected_text(format!("{:?}", s.appearance))
                    .show_ui(ui, |ui| {
                        for a in [Appearance::System, Appearance::Light, Appearance::Dark] {
                            ui.selectable_value(&mut s.appearance, a, format!("{a:?}"));
                        }
                    });
            });
            row(ui, "Interface font:", |ui| {
                let filter_id = egui::Id::NULL.with("font_filter");
                egui::ComboBox::from_id_salt("ui_font")
                    .selected_text(match s.ui_font.as_str() {
                        "Inter" => "Inter (bundled)",
                        "System" => "System (SF Pro)",
                        n => n,
                    })
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
                        ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search fonts…").desired_width(180.0));
                        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
                        let q = filter.to_lowercase();
                        for (id, label) in [("Inter", "Inter (bundled)"), ("System", "System (SF Pro)")] {
                            if q.is_empty() || label.to_lowercase().contains(&q) {
                                ui.selectable_value(&mut s.ui_font, id.to_string(), label);
                            }
                        }
                        ui.separator();
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            for name in crate::theme::system_families() {
                                if q.is_empty() || name.to_lowercase().contains(&q) {
                                    ui.selectable_value(&mut s.ui_font, name.clone(), name);
                                }
                            }
                        });
                    });
            });
            row(ui, "Font size:", |ui| {
                ui.add(egui::DragValue::new(&mut s.ui_font_size).range(10.0..=20.0).speed(0.1).suffix(" pt"));
            });

            row(ui, "", |ui| {
                ui.checkbox(&mut s.text_hinting, "Sharp text (font hinting)")
                    .on_hover_text("Snap glyph outlines to the pixel grid for crisper text.");
                ui.checkbox(&mut s.text_subpixel, "Sub-pixel text positioning")
                    .on_hover_text("Smoother letter spacing, but text looks slightly blurrier.");
            });

            ui.separator();
            section(ui, "Diff View");
            row(ui, "Font size:", |ui| {
                ui.add(egui::DragValue::new(&mut s.diff_font_size).range(9.0..=24.0).speed(0.1).suffix(" pt"));
            });
            row(ui, "", |ui| {
                ui.checkbox(&mut s.diff_marks, "Show -+ marks");
                ui.checkbox(&mut s.side_by_side, "Side-by-side");
            });

            ui.separator();
            section(ui, "History");
            row(ui, "Sort commits:", |ui| {
                ui.radio_value(&mut s.sort_topo, true, "Topologically");
                ui.radio_value(&mut s.sort_topo, false, "By date");
            });

            ui.separator();
            section(ui, "Merge");
            row(ui, "", |ui| {
                ui.checkbox(&mut s.merge_auto_apply, "Automatically apply non-conflicting changes");
            });
        });
    let after = (s.appearance, s.ui_font.clone(), s.ui_font_size, s.text_hinting, s.text_subpixel, s.ui_font_weight, s.diff_font_size, s.diff_marks, s.sort_topo, s.side_by_side, s.merge_auto_apply);
    let changed = before != after;
    if changed {
        s.save();
    }
    changed
}
