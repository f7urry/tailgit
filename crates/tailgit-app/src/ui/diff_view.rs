//! Text diff viewer: unified or side-by-side. Working tree diffs in the unified
//! view act as a staging area: each change block has Stage / Unstage / Rollback
//! actions, and single lines can be selected and staged on their own.

use eframe::egui::{self, Color32, FontId, RichText};
use git_core::content::{DiffLine, Hunkish, LineTag, Source};
use git_core::partial::{LineKey, PartialKind};

use crate::ops::Op;
use crate::repo::DiffView;
use crate::ui::dialogs::{self, Dialog};

fn colors(dark: bool) -> (Color32, Color32, Color32) {
    use crate::theme::sys;
    let a = if dark { 0.22 } else { 0.14 };
    (sys::green(dark).gamma_multiply(a), sys::red(dark).gamma_multiply(a), sys::blue(dark).gamma_multiply(a))
}

/// Which partial actions a diff offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Staging {
    None,
    /// Index ↔ work tree: stage and roll back.
    Unstaged,
    /// Untracked file: stage only.
    Untracked,
    /// HEAD ↔ index: unstage.
    Staged,
}

fn staging_mode(d: &DiffView) -> Staging {
    let r = &d.request;
    if r.old_path != r.new_path {
        return Staging::None;
    }
    match (&r.old, &r.new) {
        (Source::Index, Source::WorkTree) => Staging::Unstaged,
        (Source::Empty, Source::WorkTree) => Staging::Untracked,
        (Source::Revision(h), Source::Index) if h == "HEAD" => Staging::Staged,
        (Source::Empty, Source::Index) => Staging::Staged,
        _ => Staging::None,
    }
}

fn line_key(line: &DiffLine) -> Option<LineKey> {
    match line.tag {
        LineTag::Equal => None,
        LineTag::Delete => line.old_no.map(|n| LineKey::Old(n - 1)),
        LineTag::Insert => line.new_no.map(|n| LineKey::New(n - 1)),
    }
}

pub enum DiffAction {
    Run(Op),
    Confirm(Dialog),
}

pub struct DiffOutcome {
    /// False when the user closed the viewer.
    pub keep: bool,
    pub action: Option<DiffAction>,
}

pub fn show(ui: &mut egui::Ui, diff: &mut DiffView, side_by_side: &mut bool, closable: bool) -> DiffOutcome {
    let mut out = DiffOutcome { keep: true, action: None };
    let mode = staging_mode(diff);
    let partial_ok = mode != Staging::None
        && matches!(&diff.data, Some(Ok(d)) if d.new_bytes.is_some() && !d.diff.binary && !d.diff.too_large);
    let mut request: Option<(PartialKind, Vec<LineKey>)> = None;

    ui.horizontal(|ui| {
        ui.strong(&diff.request.title);
        if diff.request.old_path != diff.request.new_path {
            ui.weak(format!("(renamed from {})", diff.request.old_path));
        }
        if let Some(Ok(d)) = &diff.data {
            ui.weak(format!("{} → {}", d.old_label, d.new_label));
            ui.label(RichText::new(format!("+{}", d.diff.added)).color(crate::theme::sys::green(ui.visuals().dark_mode)));
            ui.label(RichText::new(format!("-{}", d.diff.removed)).color(crate::theme::sys::red(ui.visuals().dark_mode)));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if closable && ui.button("\u{f00d}").on_hover_text("Close diff").clicked() {
                out.keep = false;
            }
            ui.toggle_value(side_by_side, "Side-by-side");
            if partial_ok && !diff.selected.is_empty() {
                ui.separator();
                let n = diff.selected.len();
                if ui.button("Clear").on_hover_text("Clear the line selection").clicked() {
                    diff.selected.clear();
                }
                let keys: Vec<LineKey> = diff.selected.iter().copied().collect();
                if mode == Staging::Unstaged && ui.button(format!("Rollback {n} line(s)…")).clicked() {
                    request = Some((PartialKind::Rollback, keys.clone()));
                }
                match mode {
                    Staging::Staged => {
                        if ui.button(format!("Unstage {n} line(s)")).clicked() {
                            request = Some((PartialKind::Unstage, keys));
                        }
                    }
                    _ => {
                        if ui.button(format!("Stage {n} line(s)")).clicked() {
                            request = Some((PartialKind::Stage, keys));
                        }
                    }
                }
            }
        });
    });
    ui.separator();
    let data = match &diff.data {
        None => {
            ui.spinner();
            return out;
        }
        Some(Err(e)) => {
            ui.colored_label(crate::theme::sys::red(ui.visuals().dark_mode), e);
            return out;
        }
        Some(Ok(d)) => &d.diff,
    };
    if data.binary {
        ui.weak(if data.identical { "Binary files are identical" } else { "Binary files differ" });
        return out;
    }
    if data.too_large {
        ui.weak("File is too large to diff");
        return out;
    }
    if data.identical {
        ui.weak("Contents are identical");
        return out;
    }
    let (add_bg, del_bg, mod_bg) = colors(ui.visuals().dark_mode);
    let (font_size, marks) = ui.ctx().data(|d| {
        (
            d.get_temp::<f32>(egui::Id::NULL.with(super::settings_window::DIFF_FONT_SIZE)).unwrap_or(12.5),
            d.get_temp::<bool>(egui::Id::NULL.with(super::settings_window::DIFF_MARKS)).unwrap_or(true),
        )
    });
    let font = FontId::monospace(font_size);
    let row_h = (ui.fonts_mut(|f| f.row_height(&font)) + 3.0).max(18.0);
    let weak = ui.visuals().weak_text_color();

    if *side_by_side {
        if partial_ok {
            ui.weak("Switch off Side-by-side to stage single changes or lines.");
        }
        let rows = &data.side_by_side;
        egui::ScrollArea::both()
            .id_salt("diff_sbs")
            .auto_shrink([false, false])
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let half = (ui.available_width() / 2.0).max(300.0);
                for row in &rows[range] {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (side, bg) in [(&row.left, del_bg), (&row.right, add_bg)] {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(half, row_h), egui::Sense::hover());
                            let p = ui.painter_at(rect);
                            match side {
                                Some((no, text)) => {
                                    if row.changed {
                                        let fill = if row.left.is_some() && row.right.is_some() { mod_bg } else { bg };
                                        p.rect_filled(rect, 0.0, fill);
                                    }
                                    p.text(rect.left_center(), egui::Align2::LEFT_CENTER, format!("{no:>5} "), font.clone(), weak);
                                    p.text(
                                        rect.left_center() + egui::vec2(48.0, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        text.replace('\t', "    "),
                                        font.clone(),
                                        ui.visuals().text_color(),
                                    );
                                }
                                None => {
                                    p.rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                                }
                            }
                        }
                    });
                }
            });
    } else {
        // Flatten hunks for virtual scrolling; each contiguous run of changed
        // lines (a "change") gets a header row carrying its actions.
        enum L<'a> {
            Line(&'a DiffLine),
            Skip(usize),
            Header(usize),
        }
        let mut flat: Vec<L> = Vec::new();
        let mut blocks: Vec<Vec<LineKey>> = Vec::new();
        for h in &data.unified {
            match h {
                Hunkish::Skip(n) => flat.push(L::Skip(*n)),
                Hunkish::Lines(lines) => {
                    let mut in_block = false;
                    for line in lines {
                        match line_key(line) {
                            Some(k) => {
                                if !in_block && partial_ok {
                                    flat.push(L::Header(blocks.len()));
                                    blocks.push(Vec::new());
                                }
                                if let Some(b) = blocks.last_mut().filter(|_| partial_ok) {
                                    b.push(k);
                                }
                                in_block = true;
                            }
                            None => in_block = false,
                        }
                        flat.push(L::Line(line));
                    }
                }
            }
        }
        let accent = ui.visuals().hyperlink_color;
        let sel_bg = ui.visuals().selection.bg_fill.gamma_multiply(0.55);
        let mut toggle: Vec<LineKey> = Vec::new();
        let mut toggle_block: Option<usize> = None;
        egui::ScrollArea::both()
            .id_salt("diff_unified")
            .auto_shrink([false, false])
            .show_rows(ui, row_h, flat.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let width = ui.available_width().max(600.0);
                for (i, item) in flat[range.clone()].iter().enumerate() {
                    let row = range.start + i;
                    let sense = match item {
                        L::Line(l) if partial_ok && l.tag != LineTag::Equal => egui::Sense::click(),
                        _ => egui::Sense::hover(),
                    };
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), sense);
                    let p = ui.painter_at(rect);
                    match item {
                        L::Skip(n) => {
                            p.rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                            p.text(
                                rect.left_center() + egui::vec2(8.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                format!("⋯ {n} unchanged line(s)"),
                                font.clone(),
                                weak,
                            );
                        }
                        L::Header(b) => {
                            let keys = &blocks[*b];
                            p.rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                            let all_selected = keys.iter().all(|k| diff.selected.contains(k));
                            let mut x = rect.left() + 8.0;
                            let mut link = |label: &str, tip: &str| -> bool {
                                let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), accent);
                                let r = egui::Rect::from_min_size(
                                    egui::pos2(x, rect.top()),
                                    egui::vec2(galley.size().x + 4.0, row_h),
                                );
                                let resp = ui
                                    .interact(r, ui.scope_id().with(("hunk", row, label)), egui::Sense::click())
                                    .on_hover_text(tip);
                                if resp.hovered() {
                                    p.rect_filled(r, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
                                }
                                p.galley(egui::pos2(x + 2.0, rect.center().y - galley.size().y / 2.0), galley, accent);
                                x = r.right() + 10.0;
                                resp.clicked()
                            };
                            match mode {
                                Staging::Staged => {
                                    if link("\u{f056} Unstage", "Move this change out of the index") {
                                        request = Some((PartialKind::Unstage, keys.clone()));
                                    }
                                }
                                _ => {
                                    if link("\u{f055} Stage", "Stage this change") {
                                        request = Some((PartialKind::Stage, keys.clone()));
                                    }
                                }
                            }
                            if mode == Staging::Unstaged && link("\u{f0e2} Rollback…", "Discard this change from the working tree") {
                                request = Some((PartialKind::Rollback, keys.clone()));
                            }
                            let sel_label = if all_selected { "Deselect" } else { "Select lines" };
                            if link(sel_label, "Select the lines of this change, then stage part of them") {
                                toggle_block = Some(*b);
                            }
                            p.text(
                                egui::pos2(x, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{} line(s)", keys.len()),
                                font.clone(),
                                weak,
                            );
                        }
                        L::Line(line) => {
                            let (bg, sign) = match line.tag {
                                LineTag::Equal => (None, ' '),
                                LineTag::Delete => (Some(del_bg), '-'),
                                LineTag::Insert => (Some(add_bg), '+'),
                            };
                            if let Some(bg) = bg {
                                p.rect_filled(rect, 0.0, bg);
                            }
                            let key = line_key(line);
                            let selected = key.is_some_and(|k| diff.selected.contains(&k));
                            if selected {
                                p.rect_filled(rect, 0.0, sel_bg);
                                p.rect_filled(
                                    egui::Rect::from_min_size(rect.min, egui::vec2(3.0, row_h)),
                                    0.0,
                                    accent,
                                );
                            }
                            if resp.hovered() && partial_ok {
                                p.rect_stroke(rect, 0.0, (1.0, accent.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                            }
                            if resp.clicked() {
                                toggle.extend(key);
                            }
                            let no = |n: Option<usize>| n.map(|n| format!("{n:>5}")).unwrap_or_else(|| "     ".into());
                            p.text(
                                rect.left_center() + egui::vec2(4.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                format!("{} {} {}", no(line.old_no), no(line.new_no), if marks { sign } else { ' ' }),
                                font.clone(),
                                weak,
                            );
                            p.text(
                                rect.left_center() + egui::vec2(104.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                line.text.replace('\t', "    "),
                                font.clone(),
                                ui.visuals().text_color(),
                            );
                        }
                    }
                }
            });
        for k in toggle {
            if !diff.selected.remove(&k) {
                diff.selected.insert(k);
            }
        }
        if let Some(b) = toggle_block {
            let keys = &blocks[b];
            if keys.iter().all(|k| diff.selected.contains(k)) {
                keys.iter().for_each(|k| {
                    diff.selected.remove(k);
                });
            } else {
                diff.selected.extend(keys.iter().copied());
            }
        }
    }

    if let (Some((kind, keys)), Some(Ok(d))) = (request, &diff.data) {
        let path = diff.request.new_path.clone();
        let n = keys.len();
        let op = Op::PartialLines {
            kind,
            path: path.clone(),
            keys,
            old: d.old_bytes.clone(),
            new: d.new_bytes.clone(),
        };
        out.action = Some(if kind == PartialKind::Rollback {
            DiffAction::Confirm(
                dialogs::confirm(
                    "Rollback Lines",
                    format!("Discard {n} changed line(s) in '{path}' from the working tree? This cannot be undone."),
                    op,
                    true,
                )
                .expect("dialog"),
            )
        } else {
            DiffAction::Run(op)
        });
    }
    out
}
