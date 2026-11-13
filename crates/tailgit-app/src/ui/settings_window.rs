//! Preferences window (Cmd+,), modeled on Fork's General tab.

use eframe::egui::{self, RichText};

use crate::settings::{Appearance, Settings, UiFont};

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
    let before = (s.appearance, s.ui_font, s.diff_font_size, s.diff_marks, s.sort_topo, s.side_by_side, s.merge_auto_apply);
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
                egui::ComboBox::from_id_salt("ui_font")
                    .selected_text(match s.ui_font {
                        UiFont::Inter => "Inter",
                        UiFont::System => "System (SF Pro)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.ui_font, UiFont::Inter, "Inter");
                        ui.selectable_value(&mut s.ui_font, UiFont::System, "System (SF Pro)");
                    });
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
    let after = (s.appearance, s.ui_font, s.diff_font_size, s.diff_marks, s.sort_topo, s.side_by_side, s.merge_auto_apply);
    let changed = before != after;
    if changed {
        s.save();
    }
    changed
}
