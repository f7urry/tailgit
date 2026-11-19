//! Column layout, order, widths and visibility (persisted in egui memory).

use eframe::egui;

use super::MIN_SUBJECT_W;
use crate::settings::{LabelPosition, LabelStyle, LogColumns};

/// User-resizable widths of the columns right of the subject.
#[derive(Clone, Copy)]
pub(super) struct ColWidths {
    pub branches: f32,
    pub author: f32,
    pub hash: f32,
    pub date: f32,
}

impl ColWidths {
    pub fn meta(&self) -> f32 {
        self.author + self.hash + self.date
    }

    /// Width of the Branches column when it is shown.
    pub fn with_branches(&self, shown: bool) -> f32 {
        if shown { self.branches } else { 0.0 }
    }
}

/// Which of Author / Hash / Date are shown (persisted in egui memory).
pub(super) fn col_visibility(ctx: &egui::Context) -> [bool; 3] {
    log_columns(ctx).visible
}

/// Current column state (seeded from Settings at startup, saved back by the app).
pub fn log_columns(ctx: &egui::Context) -> LogColumns {
    ctx.data(|d| d.get_temp::<LogColumns>(egui::Id::NULL)).unwrap_or_default()
}

pub fn set_log_columns(ctx: &egui::Context, c: LogColumns) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, c));
}

/// Label placement / look, published from Settings into egui temp data.
pub(super) fn label_prefs(ctx: &egui::Context) -> (LabelPosition, LabelStyle) {
    ctx.data(|d| {
        (
            d.get_temp(egui::Id::NULL.with(crate::ui::settings_window::LABEL_POSITION)).unwrap_or_default(),
            d.get_temp(egui::Id::NULL.with(crate::ui::settings_window::LABEL_STYLE)).unwrap_or_default(),
        )
    })
}

/// Column ids: 0 Subject (flexible), 1 Branches, 2 Author, 3 Hash, 4 Date.
pub(super) const COL_NAMES: [&str; 5] = ["Subject", "Branches", "Author", "Hash", "Date"];

/// Left-to-right order of all columns (persisted).
pub(super) fn col_order(ctx: &egui::Context) -> [usize; 5] {
    log_columns(ctx).order
}

/// Left edge (relative to the row's left) and width of every column; Subject takes what is left.
pub(super) fn col_layout(order: [usize; 5], fixed: [f32; 5], total: f32) -> ([f32; 5], [f32; 5]) {
    let mut w = fixed;
    w[0] = (total - fixed.iter().sum::<f32>()).max(MIN_SUBJECT_W.min(total));
    let mut left = [0.0; 5];
    let mut x = 0.0;
    for &k in &order {
        left[k] = x;
        x += w[k];
    }
    (left, w)
}
