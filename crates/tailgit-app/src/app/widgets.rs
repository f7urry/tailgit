//! Small shared toolbar widgets.

use eframe::egui::{self, RichText};

use crate::repo::RepoView;
use crate::settings::Settings;
use crate::ui;

/// Rounded dark container grouping toolbar buttons (Fork-style).
pub(super) fn group_frame<R>(ui: &mut egui::Ui, _dark: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let dark = ui.visuals().dark_mode;
    let fill = if dark { egui::Color32::from_white_alpha(10) } else { egui::Color32::from_black_alpha(10) };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(255.0)
        .inner_margin(egui::Margin::symmetric(4, 3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(add).inner
        })
        .inner
}

/// Compact horizontal toolbar button: icon + label, highlighted when selected.
pub(super) fn bar_button(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool, enabled: bool) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let fill = if selected {
        if dark { egui::Color32::from_white_alpha(24) } else { egui::Color32::from_black_alpha(24) }
    } else {
        egui::Color32::TRANSPARENT
    };
    let color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color().gamma_multiply(0.6) };
    let text = RichText::new(format!("{icon} {label}")).size(12.5).color(color);
    let mut b = egui::Button::new(text).corner_radius(255.0).min_size(egui::vec2(0.0, 26.0));
    b = if selected { b.fill(fill) } else { b.frame_when_inactive(false) };
    ui.add_enabled(enabled, b)
}

/// Sidebar header + branch tree.
pub(super) fn sidebar(ui: &mut egui::Ui, view: &mut RepoView, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.label(RichText::new(&view.name).size(15.0).strong());
    ui.add_space(4.0);
    ui.add_space(4.0);
    ui.separator();
    ui::branch_tree::show(ui, view, settings);
}
