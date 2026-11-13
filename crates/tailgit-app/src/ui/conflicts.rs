//! Conflict resolution, following IntelliJ:
//!
//! * the **Conflicts** dialog (`MultipleFileMergeDialog`): description of the
//!   operation, a Name | Yours (…) | Theirs (…) table, Accept Yours / Accept Theirs /
//!   Merge…, and Close; it closes itself once every file is resolved;
//! * the **merge window** (`MergeThreesideViewer`): left (yours) | Result | right
//!   (theirs). The result starts as the base text; each change is accepted (`>>`,
//!   `<<`; a second accept on a conflict appends), ignored (`✖`) or resolved
//!   automatically (`✨`), and finished with Cancel / Accept Left / Accept Right /
//!   Apply Changes.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use eframe::egui::{self, Color32, RichText, Stroke};
use git_core::conflicts::{ConflictFile, UnmergedPath};
use git_core::merge_context::{MergeContext, Span};
use git_core::merge_model::{ApplyFrom, MergeModel, Side};
use git_core::repo_state::RepoState;

use crate::ops::Op;
use crate::repo::RepoView;
use crate::settings::Settings;

// ---------------------------------------------------------------- dialog state

#[derive(Default)]
pub struct ConflictsDialog {
    pub info: Option<Result<(MergeContext, Vec<UnmergedPath>), String>>,
    pub selected: BTreeSet<String>,
    anchor: Option<String>,
    /// Files still to open in the merge window after the current one (multi-file Merge…).
    pub queue: VecDeque<String>,
    /// Position of the first selected row, to reselect after the list shrinks.
    pub last_index: usize,
}

/// A confirmation asked by the merge window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Apply,
    Accept(Side),
    Cancel,
    Revert,
}

pub struct MergeEditor {
    pub file: ConflictFile,
    pub context: MergeContext,
    /// `None` for binary files: only whole-side acceptance is possible.
    model: Option<MergeModel>,
    /// Unchanged regions the user expanded (by gap index).
    expanded: HashSet<usize>,
    scroll_to: Option<usize>,
    current: Option<usize>,
    confirm: Option<Confirm>,
    /// Horizontal scroll offset shared by the three panes.
    hscroll: f32,
    /// Scroll position requested by the stripe at the right edge.
    vscroll_req: Option<f32>,
    /// Fold long unchanged regions.
    collapse: bool,
    /// Share of the text width taken by the left and the result pane (the right one gets the rest).
    split: [f32; 2],
    /// Which pane is maximized (left = yours, center = result, right = theirs).
    pub layout: crate::ui::PaneLayout,
}

impl MergeEditor {
    pub fn new(file: ConflictFile, context: MergeContext, auto_apply: bool) -> MergeEditor {
        let text = |v: &Option<Vec<u8>>| String::from_utf8(v.clone().unwrap_or_default()).ok();
        let model = if file.is_binary() {
            None
        } else {
            match (text(&file.base), text(&file.yours), text(&file.theirs)) {
                (Some(b), Some(l), Some(r)) => {
                    let deleted = file.yours.is_none() || file.theirs.is_none();
                    let mut m = MergeModel::new(&b, &l, &r, deleted);
                    if auto_apply {
                        m.apply_non_conflicting(ApplyFrom::All);
                        m.modified = false;
                    }
                    Some(m)
                }
                _ => None,
            }
        };
        let mut ed = MergeEditor {
            file,
            context,
            model,
            expanded: HashSet::new(),
            scroll_to: None,
            current: None,
            confirm: None,
            hscroll: 0.0,
            vscroll_req: None,
            collapse: true,
            split: [0.328, 0.350],
            layout: crate::ui::PaneLayout::All,
        };
        ed.scroll_to = ed.next_unresolved(None, true, false);
        ed
    }

    /// Next/previous unresolved change (or conflict only), wrapping around.
    fn next_unresolved(&self, from: Option<usize>, forward: bool, conflicts_only: bool) -> Option<usize> {
        let m = self.model.as_ref()?;
        let open: Vec<usize> = (0..m.changes.len())
            .filter(|&i| !m.changes[i].is_resolved() && (!conflicts_only || m.changes[i].is_conflict()))
            .collect();
        let t = match (from, forward) {
            (Some(c), true) => open.iter().find(|&&i| i > c).or(open.first()),
            (Some(c), false) => open.iter().rev().find(|&&i| i < c).or(open.last()),
            (None, true) => open.first(),
            (None, false) => open.last(),
        };
        t.copied()
    }

    /// Pane split after applying the maximized pane, if any.
    fn view_split(&self) -> [f32; 2] {
        use crate::ui::PaneLayout::*;
        match self.layout {
            All => self.split,
            Left => [0.96, 0.02],
            Center => [0.02, 0.96],
            Right => [0.02, 0.02],
        }
    }

    fn modified(&self) -> bool {
        self.model.as_ref().is_some_and(|m| m.modified)
    }
}

// ---------------------------------------------------------------- helpers

fn rich(ui: &mut egui::Ui, spans: &[Span]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for s in spans {
            match s {
                Span::Plain(t) => ui.label(t),
                Span::Bold(t) => ui.label(RichText::new(t).strong()),
                Span::Code(t) => ui.label(RichText::new(t).code()),
            };
        }
    });
}

/// One line of text cut with "…" to `max_w`; `suffix` adds a second, differently coloured part.
fn elided(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    max_w: f32,
    suffix: Option<(&str, Color32)>,
) -> std::sync::Arc<egui::Galley> {
    use egui::text::{LayoutJob, TextFormat, TextWrapping};
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat::simple(font.clone(), color));
    if let Some((s, c)) = suffix {
        job.append(s, 10.0, TextFormat::simple(font, c));
    }
    job.wrap = TextWrapping {
        max_width: max_w.max(10.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

fn file_name(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    }
}

/// A two-button confirmation drawn as a modal; returns Some(true) for the first button.
fn ask(ctx: &egui::Context, id: &str, title: &str, text: &str, yes: &str, no: &str) -> Option<bool> {
    let mut answer = None;
    let r = egui::Modal::new(egui::Id::unique(("merge_confirm", id))).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading(title);
        ui.add_space(4.0);
        ui.label(text);
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(no).clicked() {
                answer = Some(false);
            }
            if ui.add(egui::Button::new(RichText::new(yes).strong())).clicked() {
                answer = Some(true);
            }
        });
    });
    if r.should_close() && answer.is_none() {
        answer = Some(false);
    }
    answer
}

// ---------------------------------------------------------------- entry point

pub fn show(ctx: &egui::Context, view: &mut RepoView, settings: &mut Settings) {
    if view.merge_editor.is_some() {
        merge_window::show(ctx, view, settings);
    } else if view.conflicts_dialog.is_some() {
        conflicts_dialog(ctx, view, settings);
    }
}

// ---------------------------------------------------------------- Conflicts dialog

enum DialogAction {
    AcceptOne(String, bool),
    ResolveSimple,
    Merge,
    Close,
}

/// Bordered, rounded dialog button (primary = filled accent).
fn dlg_btn(ui: &mut egui::Ui, pal: &syntax::Palette, label: &str, primary: bool, enabled: bool) -> egui::Response {
    let (fill, stroke, text) = if primary {
        (pal.primary, Stroke::NONE, Color32::WHITE)
    } else {
        (pal.bg, Stroke::new(1.0, pal.btn_border), pal.text)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).color(text))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(7.0)
            .min_size(egui::vec2(0.0, 30.0)),
    )
}

fn conflicts_dialog(ctx: &egui::Context, view: &mut RepoView, settings: &mut Settings) {
    let busy = view.busy.is_some();
    let mut action: Option<DialogAction> = None;
    let mut group_changed = false;
    let mut chosen: VecDeque<String> = VecDeque::new();
    let Some(dialog) = view.conflicts_dialog.as_mut() else { return };

    let builder = egui::ViewportBuilder::default()
        .with_title("Conflicts")
        .with_inner_size([900.0, 540.0])
        .with_min_inner_size([640.0, 360.0]);
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("tailgit_conflicts"), builder, |ui, _| {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            action = Some(DialogAction::Close);
        }
        conflicts_body(ui, dialog, settings, busy, &mut action, &mut group_changed, &mut chosen);
    });
    if group_changed {
        settings.save();
    }
    match action {
        Some(DialogAction::AcceptOne(path, yours)) => {
            view.run(Op::AcceptSide { paths: vec![path], yours });
        }
        Some(DialogAction::ResolveSimple) => {
            let paths = match &dialog.info {
                Some(Ok((_, files))) => files.iter().map(|f| f.path.clone()).collect(),
                _ => Vec::new(),
            };
            view.run(Op::ResolveSimple { paths });
        }
        Some(DialogAction::Merge) => {
            dialog.queue = chosen;
            view.merge_next_queued();
        }
        Some(DialogAction::Close) => {
            let remaining = matches!(&dialog.info, Some(Ok((_, f))) if !f.is_empty());
            view.conflicts_dialog = None;
            if remaining {
                view.notify(false, "There are pending unresolved conflicts.".into());
            }
        }
        None => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn conflicts_body(
    ui: &mut egui::Ui,
    dialog: &mut ConflictsDialog,
    settings: &mut Settings,
    busy: bool,
    action: &mut Option<DialogAction>,
    group_changed: &mut bool,
    chosen: &mut VecDeque<String>,
) {
    let dark = ui.visuals().dark_mode;
    let pal = syntax::palette(dark);
    let (context, files) = match &dialog.info {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading merge details…");
            });
            return;
        }
        Some(Err(e)) => {
            ui.colored_label(
                crate::theme::sys::red(dark),
                format!("Cannot check the working tree for unmerged files because of an error.\n{e}"),
            );
            return;
        }
        Some(Ok((c, f))) => (c.clone(), f.clone()),
    };
    let win_fill = ui.visuals().window_fill;
    let frame = |m: egui::Margin| egui::Frame::new().fill(win_fill).inner_margin(m);

    // Rows in display order: (Some(dir header) for group rows, file).
    let mut sorted: Vec<&UnmergedPath> = files.iter().collect();
    sorted.sort_by(|a, b| file_name(&a.path).1.cmp(file_name(&b.path).1).then(a.path.cmp(&b.path)));
    let mut rows: Vec<(Option<String>, Option<&UnmergedPath>)> = Vec::new();
    if settings.conflicts_group_by_dir {
        let mut by_dir: BTreeMap<&str, Vec<&UnmergedPath>> = BTreeMap::new();
        for f in &sorted {
            by_dir.entry(file_name(&f.path).0).or_default().push(f);
        }
        for (dir, fs) in by_dir {
            if !dir.is_empty() {
                rows.push((Some(dir.to_string()), None));
            }
            rows.extend(fs.into_iter().map(|f| (None, Some(f))));
        }
    } else {
        rows.extend(sorted.iter().map(|f| (None, Some(*f))));
    }
    let order: Vec<String> = rows.iter().filter_map(|(_, f)| f.map(|f| f.path.clone())).collect();

    // ---- footer: Close, Accept and Finish, Resolve Manually
    egui::Panel::bottom("conflicts_footer")
        .frame(frame(egui::Margin { left: 18, right: 18, top: 10, bottom: 16 }))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.spacing_mut().button_padding = egui::vec2(16.0, 6.0);
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.horizontal(|ui| {
                if ui.checkbox(&mut settings.conflicts_group_by_dir, "Group by directory").changed() {
                    *group_changed = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let any = !dialog.selected.is_empty() && !busy;
                if dlg_btn(ui, &pal, "Resolve Manually", true, any).clicked() {
                    *action = Some(DialogAction::Merge);
                }
                // Enabled only once everything is resolved; the dialog then closes by itself.
                dlg_btn(ui, &pal, "Accept and Finish", false, false);
                if dlg_btn(ui, &pal, "Close", false, true).clicked() {
                    *action = Some(DialogAction::Close);
                }
            });
            });
        });

    // ---- description, commit message, Resolve All Simple Conflicts
    egui::Panel::top("conflicts_header")
        .frame(frame(egui::Margin { left: 18, right: 18, top: 16, bottom: 4 }))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            rich(ui, &context.description);
            if let Some(msg) = &context.commit_message {
                ui.label(RichText::new(msg).monospace());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
                if dlg_btn(ui, &pal, "\u{f0d0}  Resolve All Simple Conflicts", false, !busy && !files.is_empty())
                    .on_hover_text("Resolve conflicts whose changes do not overlap at word level or differ only in whitespace")
                    .clicked()
                {
                    *action = Some(DialogAction::ResolveSimple);
                }
                let eye = ui.add(
                    egui::Button::new(RichText::new("\u{f06e} \u{f0d7}").color(pal.icon)).frame(false).min_size(egui::vec2(36.0, 30.0)),
                );
                egui::Popup::from_toggle_button_response(&eye)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if ui.checkbox(&mut settings.conflicts_group_by_dir, "Group by directory").changed() {
                            *group_changed = true;
                        }
                    });
            });
        });

    // ---- table
    egui::CentralPanel::default().frame(frame(egui::Margin { left: 18, right: 18, top: 8, bottom: 0 })).show(ui, |ui| {
        egui::Frame::new().stroke(Stroke::new(1.0, pal.sep)).inner_margin(0.0).show(ui, |ui| {
            let table_w = ui.available_width();
            let name_w = table_w * 0.634;
            let col_w = (table_w - name_w) / 2.0;
            let cols = [(0.0, name_w), (name_w, name_w + col_w), (name_w + col_w, table_w)];
            let font = egui::TextStyle::Body.resolve(ui.style());
            let text = pal.text;
            let weak = pal.lineno;
            let sel_fill = if dark { ui.visuals().selection.bg_fill } else { Color32::from_rgb(179, 208, 242) };
            let cell = |rect: egui::Rect, i: usize| {
                egui::Rect::from_x_y_ranges(
                    (rect.left() + cols[i].0 + 8.0)..=(rect.left() + cols[i].1 - 6.0),
                    rect.y_range(),
                )
            };

            // Header: only the two side columns are titled, split by thin lines.
            let (hrect, _) = ui.allocate_exact_size(egui::vec2(table_w, 30.0), egui::Sense::hover());
            let titles = ["", context.yours_column.as_str(), context.theirs_column.as_str()];
            for (i, t) in titles.iter().enumerate() {
                if i > 0 {
                    let x = hrect.left() + cols[i].0;
                    ui.painter().vline(x, hrect.y_range(), Stroke::new(1.0, pal.sep));
                }
                if t.is_empty() {
                    continue;
                }
                let r = cell(hrect, i);
                let g = elided(ui, t, font.clone(), weak, r.width(), None);
                ui.painter_at(r).galley(egui::pos2(r.left(), r.center().y - g.size().y / 2.0), g, weak);
                ui.interact(r, ui.scope_id().with(("conflict_header", i)), egui::Sense::hover()).on_hover_text(*t);
            }
            ui.painter().hline(hrect.x_range(), hrect.bottom(), Stroke::new(1.0, pal.sep));

            egui::ScrollArea::vertical().id_salt("conflict_rows").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // "Unresolved  N files" group row.
                let (g, _) = ui.allocate_exact_size(egui::vec2(table_w, 30.0), egui::Sense::hover());
                let cy = g.center().y;
                let p = ui.painter();
                p.text(egui::pos2(g.left() + 14.0, cy), egui::Align2::CENTER_CENTER, "\u{f078}", egui::FontId::proportional(10.0), pal.icon);
                p.text(egui::pos2(g.left() + 38.0, cy), egui::Align2::CENTER_CENTER, "\u{f00d}", egui::FontId::proportional(14.0), crate::theme::sys::red(dark));
                let t = p.layout_no_wrap("Unresolved".into(), font.clone(), text);
                let tw = t.size().x;
                p.galley(egui::pos2(g.left() + 58.0, cy - t.size().y / 2.0), t, text);
                let n = files.len();
                p.text(
                    egui::pos2(g.left() + 58.0 + tw + 8.0, cy),
                    egui::Align2::LEFT_CENTER,
                    format!("{n} file{}", if n == 1 { "" } else { "s" }),
                    font.clone(),
                    weak,
                );

                for (dir, file) in &rows {
                    if let Some(d) = dir {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(table_w, 26.0), egui::Sense::hover());
                        let r0 = cell(r, 0).union(cell(r, 2));
                        let g = elided(ui, &format!("\u{f07b} {d}"), font.clone(), weak, r0.width(), None);
                        ui.painter_at(r0).galley(egui::pos2(r0.left() + 36.0, r0.center().y - g.size().y / 2.0), g, weak);
                        continue;
                    }
                    let Some(f) = file else { continue };
                    let sel = dialog.selected.contains(&f.path);
                    let (yours, theirs) = context.statuses(f);
                    let (dirname, name) = file_name(&f.path);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(table_w, 30.0), egui::Sense::click());
                    if sel {
                        ui.painter().rect_filled(rect, 0.0, sel_fill);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, pal.hover);
                    }
                    // Icon, name and (when flat) the directory in grey.
                    let indent = if settings.conflicts_group_by_dir && !dirname.is_empty() { 18.0 } else { 0.0 };
                    let icon_x = rect.left() + 46.0 + indent;
                    let (glyph, color) = if name.ends_with(".java") {
                        ("\u{f0f4}", crate::theme::sys::orange(dark))
                    } else {
                        ("\u{f15b}", pal.icon)
                    };
                    ui.painter().text(egui::pos2(icon_x, rect.center().y), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(14.0), color);
                    let mut r = cell(rect, 0);
                    r.min.x = icon_x + 14.0;
                    let dir = (!settings.conflicts_group_by_dir && !dirname.is_empty()).then_some((dirname, weak));
                    let g = elided(ui, name, font.clone(), text, r.width(), dir);
                    ui.painter_at(r).galley(egui::pos2(r.left(), r.center().y - g.size().y / 2.0), g, text);
                    // Per-side status and Accept button.
                    for (i, s, yours_side) in [(1, yours, true), (2, theirs, false)] {
                        let r = cell(rect, i);
                        let c = match s {
                            git_core::merge_context::SideStatus::Modified => text,
                            git_core::merge_context::SideStatus::Deleted => weak,
                        };
                        let g = elided(ui, s.label(), font.clone(), c, r.width() - 90.0, None);
                        ui.painter_at(r).galley(egui::pos2(r.left(), r.center().y - g.size().y / 2.0), g, c);
                        let b = egui::Rect::from_min_size(
                            egui::pos2(r.right() - 74.0, rect.center().y - 12.0),
                            egui::vec2(74.0, 24.0),
                        );
                        let click = ui
                            .put(
                                b,
                                egui::Button::new(RichText::new("Accept").color(text))
                                    .fill(pal.bg)
                                    .stroke(Stroke::new(1.0, pal.btn_border))
                                    .corner_radius(6.0),
                            )
                            .on_hover_text(if yours_side { "Accept Yours" } else { "Accept Theirs" })
                            .clicked();
                        if click && !busy {
                            *action = Some(DialogAction::AcceptOne(f.path.clone(), yours_side));
                        }
                    }
                    let resp = resp.on_hover_text(&f.path);
                    if resp.clicked() {
                        let m = ui.input(|i| i.modifiers);
                        if m.shift {
                            let a = dialog.anchor.as_ref().and_then(|a| order.iter().position(|p| p == a)).unwrap_or(0);
                            let b = order.iter().position(|p| *p == f.path).unwrap_or(0);
                            dialog.selected = order[a.min(b)..=a.max(b)].iter().cloned().collect();
                        } else if m.command || m.ctrl {
                            if !dialog.selected.remove(&f.path) {
                                dialog.selected.insert(f.path.clone());
                            }
                            dialog.anchor = Some(f.path.clone());
                        } else {
                            dialog.selected = [f.path.clone()].into();
                            dialog.anchor = Some(f.path.clone());
                        }
                    }
                    if resp.double_clicked() {
                        dialog.selected = [f.path.clone()].into();
                        *action = Some(DialogAction::Merge);
                    }
                }
            });
        });
    });

    // Keyboard: arrows move the selection, Enter merges (the default button).
    if ui.ctx().memory(|m| m.focused().is_none()) {
        let (up, down, enter) = ui.input(|i| {
            (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Enter))
        });
        if (up || down) && !order.is_empty() {
            let cur = order.iter().position(|p| dialog.selected.contains(p));
            let next = match (cur, down) {
                (None, _) => 0,
                (Some(c), true) => (c + 1).min(order.len() - 1),
                (Some(c), false) => c.saturating_sub(1),
            };
            dialog.selected = [order[next].clone()].into();
            dialog.anchor = Some(order[next].clone());
        }
        if enter && !dialog.selected.is_empty() && !busy {
            *action = Some(DialogAction::Merge);
        }
    }
    dialog.last_index = order.iter().position(|p| dialog.selected.contains(p)).unwrap_or(dialog.last_index);
    // The selection in display order (multi-file Merge… opens them in turn).
    *chosen = order.iter().filter(|p| dialog.selected.contains(*p)).cloned().collect();
}

// ---------------------------------------------------------------- merge window

mod merge_window;
mod syntax;

/// Accept Left / Accept Right: the whole side (deleting the file if that side deleted it).
fn accept_op(path: &str, side: Side) -> Op {
    Op::AcceptSide {
        paths: vec![path.to_string()],
        yours: side == Side::Left,
    }
}

/// Continue / Skip / Abort buttons for the operation in progress (state banner).
pub fn continue_buttons(ui: &mut egui::Ui, state: RepoState, busy: bool, op: &mut Option<Op>) {
    ui.horizontal(|ui| {
        let (label, o) = match state {
            RepoState::Merging => ("Commit Merge", Some(Op::MergeContinue)),
            RepoState::Rebasing => ("Continue Rebase", Some(Op::RebaseContinue)),
            RepoState::CherryPicking => ("Continue Cherry-Pick", Some(Op::CherryPickContinue)),
            RepoState::Reverting => ("Continue Revert", Some(Op::RevertContinue)),
            RepoState::Normal => ("", None),
        };
        if let Some(o) = o {
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                *op = Some(o);
            }
        }
        let aborts: Vec<(&str, Op)> = match state {
            RepoState::Merging => vec![("Abort Merge", Op::MergeAbort)],
            RepoState::Rebasing => vec![("Skip Commit", Op::RebaseSkip), ("Abort Rebase", Op::RebaseAbort)],
            RepoState::CherryPicking => vec![("Skip", Op::CherryPickSkip), ("Abort Cherry-Pick", Op::CherryPickAbort)],
            RepoState::Reverting => vec![("Abort Revert", Op::RevertAbort)],
            RepoState::Normal => vec![],
        };
        for (label, o) in aborts {
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                *op = Some(o);
            }
        }
    });
}
