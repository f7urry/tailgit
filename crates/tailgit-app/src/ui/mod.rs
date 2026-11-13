pub mod branch_tree;
pub mod commit_panel;
pub mod conflicts;
pub mod details;
pub mod dialogs;
pub mod diff_view;
pub mod log_table;
pub mod settings_window;
pub mod stash_panel;

use chrono::{Local, TimeZone};
use eframe::egui::{self, Color32, RichText};
use git_core::log::ChangeKind;
use git_core::refs::{RefKind, RefLabel};

/// Fork-like log date: `Today at 16.40`, `Yesterday at 13.33`, `11 Nov 2026 at 21.23`.
pub fn format_time(ts: i64) -> String {
    let Some(t) = Local.timestamp_opt(ts, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let d = t.date_naive();
    if d == today {
        t.format("Today at %H.%M").to_string()
    } else if Some(d) == today.pred_opt() {
        t.format("Yesterday at %H.%M").to_string()
    } else {
        t.format("%-d %b %Y at %H.%M").to_string()
    }
}

pub fn format_full_time(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S %z").to_string())
        .unwrap_or_default()
}

pub fn ref_colors(label: &RefLabel, dark: bool) -> (Color32, Color32) {
    use crate::theme::sys;
    let tint = match label.kind {
        RefKind::Head => sys::blue(dark),
        RefKind::LocalBranch if label.is_current => sys::green(dark),
        RefKind::LocalBranch => sys::teal(dark),
        RefKind::RemoteBranch => sys::purple(dark),
        RefKind::Tag => sys::orange(dark),
    };
    // Fork-style tag: soft tinted fill, outlined in the tint (outline drawn by callers).
    let bg = tint.gamma_multiply(if dark { 0.22 } else { 0.12 });
    (bg, tint)
}

pub fn ref_icon(kind: RefKind) -> &'static str {
    match kind {
        RefKind::Head => "\u{f140}",
        RefKind::LocalBranch => "\u{f126}",
        RefKind::RemoteBranch => "\u{f0c2}",
        RefKind::Tag => "\u{f02b}",
    }
}

/// Draws a compact ref badge.
pub fn ref_badge(ui: &mut egui::Ui, label: &RefLabel) -> egui::Response {
    let (bg, fg) = ref_colors(label, ui.visuals().dark_mode);
    let mut text = RichText::new(format!("{} {}", ref_icon(label.kind), label.name))
        .color(fg)
        .small();
    if label.is_current {
        text = text.strong();
    }
    egui::Frame::new()
        .fill(bg)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(text))
        .response
}

/// Stable avatar color for an author (identicon-like tile).
pub fn avatar_color(seed: &str, dark: bool) -> Color32 {
    use crate::theme::sys;
    let h = seed.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    let palette = [sys::purple, sys::red, sys::green, sys::orange, sys::blue, sys::teal, sys::indigo];
    palette[(h as usize) % palette.len()](dark)
}

/// Paints a rounded avatar tile with the author's initial.
pub fn paint_avatar(p: &egui::Painter, rect: egui::Rect, name: &str, dark: bool) {
    let c = avatar_color(name, dark);
    p.rect_filled(rect, 3.0, c);
    let initial: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default();
    p.text(rect.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(rect.height() * 0.62), Color32::WHITE);
}

pub fn change_color(kind: ChangeKind, dark: bool) -> Color32 {
    use crate::theme::sys;
    match kind {
        ChangeKind::Added => sys::green(dark),
        ChangeKind::Deleted => sys::red(dark),
        ChangeKind::Renamed | ChangeKind::Copied => sys::indigo(dark),
        ChangeKind::Unmerged => sys::orange(dark),
        _ => sys::blue(dark),
    }
}

/// `path/to/file.rs` → (`file.rs`, `path/to`).
pub fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (name, dir),
        None => (path, ""),
    }
}

/// A file row: colored status letter, file name, dimmed directory.
pub fn file_label(ui: &mut egui::Ui, kind: ChangeKind, path: &str, selected: bool) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let (name, dir) = split_path(path);
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Body.resolve(ui.style());
    job.append(
        &format!("{} ", kind.letter()),
        0.0,
        egui::TextFormat::simple(font.clone(), change_color(kind, dark)),
    );
    job.append(
        name,
        0.0,
        egui::TextFormat::simple(font.clone(), change_color(kind, dark)),
    );
    if !dir.is_empty() {
        job.append(
            &format!("  {dir}"),
            0.0,
            egui::TextFormat::simple(font, ui.visuals().weak_text_color()),
        );
    }
    ui.selectable_label(selected, job)
}

pub fn status_kind(code: char) -> ChangeKind {
    match code {
        '?' => ChangeKind::Added,
        'U' => ChangeKind::Unmerged,
        c => ChangeKind::from_code(c),
    }
}

/// Which pane fills the window (`All` shows every pane).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PaneLayout {
    All,
    Left,
    Center,
    Right,
}

/// Window glyph with the maximized pane shaded (`All` shows the plain split).
pub fn layout_icon(ui: &egui::Ui, r: egui::Rect, l: PaneLayout, selected: bool, hovered: bool) {
    let v = ui.visuals();
    let p = ui.painter();
    if selected || hovered {
        let a = if selected { 24 } else { 12 };
        p.rect_filled(r, 5.0, if v.dark_mode { egui::Color32::from_white_alpha(a) } else { egui::Color32::from_black_alpha(a) });
    }
    let fg = if selected { v.strong_text_color() } else { v.weak_text_color() };
    let b = egui::Rect::from_center_size(r.center(), egui::vec2(17.0, 14.0));
    let (x1, x2) = (b.left() + 5.0, b.right() - 5.0);
    let fill = fg.gamma_multiply(0.45);
    let seg = |a: f32, c: f32| egui::Rect::from_min_max(egui::pos2(a, b.top()), egui::pos2(c, b.bottom())).shrink(1.5);
    match l {
        PaneLayout::All => {}
        PaneLayout::Left => { p.rect_filled(seg(b.left(), x1), 1.5, fill); }
        PaneLayout::Center => { p.rect_filled(seg(x1, x2), 0.0, fill); }
        PaneLayout::Right => { p.rect_filled(seg(x2, b.right()), 1.5, fill); }
    }
    let s = egui::Stroke::new(1.2, fg);
    p.rect_stroke(b, 3.5, s, egui::StrokeKind::Inside);
    p.vline(x1, b.y_range(), s);
    p.vline(x2, b.y_range(), s);
}

/// Four layout buttons (All / Left / Center / Right) centered on `center_x`.
pub fn layout_switcher(ui: &mut egui::Ui, center_x: f32, lay: &mut PaneLayout, id: &str) {
    let row = ui.max_rect();
    let start = egui::pos2(center_x - 60.0, row.center().y - 11.0);
    for (k, l) in [PaneLayout::All, PaneLayout::Left, PaneLayout::Center, PaneLayout::Right].into_iter().enumerate() {
        let r = egui::Rect::from_min_size(start + egui::vec2(k as f32 * 30.0, 0.0), egui::vec2(28.0, 22.0));
        let tip = match l {
            PaneLayout::All => "Show all panes",
            PaneLayout::Left => "Maximize left pane",
            PaneLayout::Center => "Maximize center pane",
            PaneLayout::Right => "Maximize right pane",
        };
        let resp = ui.interact(r, egui::Id::unique(("pane_layout", id, k)), egui::Sense::click()).on_hover_text(tip);
        if resp.clicked() {
            *lay = l;
        }
        layout_icon(ui, r, l, *lay == l, resp.hovered());
    }
}
