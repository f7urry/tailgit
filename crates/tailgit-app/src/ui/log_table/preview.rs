//! Live mock of the log rows used by the Settings window.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use super::paint::{paint_pills, paint_subject_and_labels};
use super::{LABEL_LINE_H, ROW_H};
use crate::settings::{LabelPosition, LabelStyle};

/// Small mock of the log rows for the Settings window, so label options can be judged live.
pub fn label_preview(ui: &mut egui::Ui, pos: LabelPosition, style: LabelStyle) {
    use git_core::refs::{RefKind, RefLabel};
    let dark = ui.visuals().dark_mode;
    let lbl = |kind, name: &str, is_current| RefLabel { kind, name: name.into(), is_current };
    let rows: [(&str, Vec<RefLabel>); 3] = [
        (
            "[PROJ-101] Add version history",
            vec![
                lbl(RefKind::LocalBranch, "feat/release-2.9", true),
                lbl(RefKind::RemoteBranch, "origin/feat/release-2.9", false),
                lbl(RefKind::Tag, "v2.9.3", false),
            ],
        ),
        ("fix: wrong version in import", vec![lbl(RefKind::LocalBranch, "bugfix/import", false)]),
        ("Update README", vec![]),
    ];
    let two_line = matches!(pos, LabelPosition::Top | LabelPosition::Bottom);
    let hidden = pos == LabelPosition::Hidden;
    let col_w = if pos == LabelPosition::Column { 150.0 } else { 0.0 };
    let width = ui.available_width().min(300.0);
    let heights: Vec<f32> =
        rows.iter().map(|(_, l)| if two_line && !hidden && !l.is_empty() { ROW_H + LABEL_LINE_H } else { ROW_H }).collect();
    let total: f32 = heights.iter().sum();
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, total), Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 6.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(area, 6.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
    let text = ui.visuals().text_color();
    let mut top = area.top();
    for (i, ((subject, labels), h)) in rows.iter().zip(&heights).enumerate() {
        let cy = top + if pos == LabelPosition::Top && *h > ROW_H { h - ROW_H } else { 0.0 } + ROW_H / 2.0;
        let nx = area.left() + 14.0;
        if i > 0 {
            painter.line_segment([Pos2::new(nx, top), Pos2::new(nx, cy)], Stroke::new(1.5, Color32::from_gray(150)));
        }
        if i + 1 < rows.len() {
            painter.line_segment([Pos2::new(nx, cy), Pos2::new(nx, top + h)], Stroke::new(1.5, Color32::from_gray(150)));
        }
        painter.circle_filled(Pos2::new(nx, cy), 5.0, crate::theme::sys::blue(dark));
        painter.circle_stroke(Pos2::new(nx, cy), 5.0, Stroke::new(1.5, Color32::from_gray(60)));
        let sub_rect = Rect::from_min_max(Pos2::new(area.left() + 28.0, top), Pos2::new(area.right() - col_w - 4.0, top + h));
        let shown: &[RefLabel] = if hidden { &[] } else { labels };
        paint_subject_and_labels(&ui.painter_at(sub_rect), sub_rect.left(), top, cy, subject, text, true, shown, pos, style, dark);
        if col_w > 0.0 && !shown.is_empty() {
            let col = Rect::from_min_max(Pos2::new(area.right() - col_w, top), Pos2::new(area.right() - 4.0, top + h));
            paint_pills(&ui.painter_at(col), col.left(), cy, shown, style, dark, text, ui.visuals().weak_text_color());
        }
        top += h;
    }
}
