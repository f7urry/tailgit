//! The virtualized commit table: keyboard navigation, rows and action dispatch.

use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use git_core::log::Commit;
use git_core::refs::RefLabel;
use log_graph::RowPrint;

use super::columns::{col_layout, label_prefs};
use super::header::{header, Columns};
use super::menus::{multi_menu, row_menu};
use crate::ui::dialogs::{self, Dialog};
use super::paint::{paint_graph, paint_pills, paint_subject_and_labels};
use super::{RowAction, LABEL_LINE_H, ROW_H};
use crate::repo::RepoView;
use crate::settings::LabelPosition;
use crate::ui::{format_time, paint_avatar};

pub fn table(ui: &mut egui::Ui, view: &mut RepoView) {
    if let Some(e) = &view.log.error {
        ui.colored_label(crate::theme::sys::red(ui.visuals().dark_mode), e);
    }
    let mut actions = Vec::new();
    let n = view.log.data.as_ref().map_or(0, |d| d.commits.len());
    if n > 0 {
        actions.extend(keyboard_action(ui, view, n));
    }
    let Some(data) = view.log.data.as_ref() else {
        ui.centered_and_justified(|ui| {
            ui.spinner();
        });
        return;
    };
    if n == 0 {
        let msg = if view.log.inputs.is_empty() { "No commits yet" } else { "No commits match the filter" };
        ui.centered_and_justified(|ui| {
            ui.weak(msg);
        });
        return;
    }

    let columns = header(ui);

    let (label_pos, _) = label_prefs(ui.ctx());
    let two_line = matches!(label_pos, LabelPosition::Top | LabelPosition::Bottom);
    let labels_of = |hash: &String| {
        if label_pos == LabelPosition::Hidden {
            None
        } else {
            data.labels.get(hash).filter(|l| !l.is_empty())
        }
    };
    // Rows with ref labels get a second line for the pills, so heights vary.
    let offsets = row_offsets(&data.commits, |c| two_line && labels_of(&c.hash).is_some());
    let total_h = offsets[n];

    let mut scroll = egui::ScrollArea::vertical().id_salt("log_scroll").auto_shrink([false, false]);
    if let Some(t) = view.log.scroll_to.take() {
        let vis = &view.log.visible;
        if !(vis.start < t && t + 1 < vis.end) {
            let top = t.saturating_sub(vis.len().max(1) / 2);
            scroll = scroll.vertical_scroll_offset(offsets[top]);
        }
    }

    let ctx = RowCtx {
        head_hash: data.refs.head.hash().map(str::to_string),
        current: data.refs.head.branch_name().map(str::to_string),
        repo_state: view.state,
        sel_commits: view.selected_rows().iter().map(|&r| &data.commits[r]).collect(),
        columns,
        label_pos,
        dark: ui.visuals().dark_mode,
    };
    let multi = &view.log.multi;
    let mut visible = 0..0;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        scroll.show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let (area, _) = ui.allocate_exact_size(Vec2::new(width, total_h), Sense::hover());
            let first = offsets.partition_point(|&o| o <= viewport.min.y).saturating_sub(1).min(n - 1);
            let last = offsets.partition_point(|&o| o < viewport.max.y).min(n);
            visible = first..last;
            for i in first..last {
                let c = &data.commits[i];
                let rect = Rect::from_min_size(
                    area.min + Vec2::new(0.0, offsets[i]),
                    Vec2::new(width, offsets[i + 1] - offsets[i]),
                );
                let labels = labels_of(&c.hash).map(|l| l.as_slice()).unwrap_or(&[]);
                let is_sel = multi.contains(&c.hash);
                let graph = data.graph.row(i);
                row(ui, &ctx, &RowData { i, c, graph: &graph, rect, labels, is_sel }, &mut actions);
            }
        });
    });
    view.log.visible = visible;

    for a in actions {
        match a {
            RowAction::Select(i) => view.select_row(i),
            RowAction::Toggle(i) => view.toggle_row(i),
            RowAction::Extend(i) => view.extend_to_row(i),
            RowAction::SelectAll => view.select_all_rows(),
            RowAction::Op(op) => view.run(op),
            RowAction::Dialog(d) => view.dialog = Some(d),
            RowAction::Copy(s) => ui.ctx().copy_text(s),
            RowAction::EditMessage(hash) => {
                match git_core::ops::rebase_commits(&view.jobs.git, &hash) {
                    Ok(list) => {
                        let message = list.into_iter().find(|c| c.hash == hash).map(|c| c.message).unwrap_or_default();
                        view.dialog = Some(Dialog::EditMessage {
                            label: format!("{} {}", &hash[..hash.len().min(8)], message.lines().next().unwrap_or("")),
                            hash,
                            message,
                        });
                    }
                    Err(e) => view.notify(false, format!("Edit commit message: {e}")),
                }
            }
            RowAction::InteractiveRebase(hash) => match git_core::ops::rebase_commits(&view.jobs.git, &hash) {
                Ok(list) => view.dialog = Some(dialogs::rebase_dialog(&hash, list)),
                Err(e) => view.notify(false, format!("Interactive rebase: {e}")),
            },
            RowAction::PushUpTo(hash) => match dialogs::push_dialog(view) {
                Some(Dialog::Push { remote, remotes, local, remote_branch, force, tags, has_upstream, .. }) => {
                    let ahead = view.jobs.git
                        .read(&["rev-list", "--count", &format!("{remote}/{remote_branch}..{hash}")])
                        .ok()
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(0);
                    let revision = Some(hash);
                    view.dialog = Some(Dialog::Push {
                        revision, remote, remotes, local, remote_branch, set_upstream: false, force, tags, ahead, has_upstream,
                    });
                }
                _ => view.notify(false, "Push: no remote configured".into()),
            },
        }
    }
}

/// Per-frame state shared by every row.
struct RowCtx<'a> {
    head_hash: Option<String>,
    current: Option<String>,
    repo_state: git_core::repo_state::RepoState,
    /// Commits the context menu acts on when more than one is selected.
    sel_commits: Vec<&'a Commit>,
    columns: Columns,
    label_pos: LabelPosition,
    dark: bool,
}

/// Top offset of every row plus the total height as the last entry.
fn row_offsets(commits: &[Commit], tall: impl Fn(&Commit) -> bool) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(commits.len() + 1);
    let mut acc = 0.0f32;
    for c in commits {
        offsets.push(acc);
        acc += if tall(c) { ROW_H + LABEL_LINE_H } else { ROW_H };
    }
    offsets.push(acc);
    offsets
}

/// Arrow / page / home / end / select-all navigation when no text field has focus.
fn keyboard_action(ui: &egui::Ui, view: &mut RepoView, n: usize) -> Option<RowAction> {
    if ui.ctx().memory(|m| m.focused().is_some()) || view.dialog.is_some() || !view.log.keyboard {
        return None;
    }
    let page = view.log.visible.len().max(1);
    let (down, up, pgdn, pgup, home, end, shift, all) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::PageDown),
            i.key_pressed(egui::Key::PageUp),
            i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::End),
            i.modifiers.shift,
            i.modifiers.command && i.key_pressed(egui::Key::A),
        )
    });
    if all {
        return Some(RowAction::SelectAll);
    }
    let target = match view.log.selected {
        _ if home => 0,
        _ if end => n - 1,
        Some(s) if down => (s + 1).min(n - 1),
        Some(s) if up => s.saturating_sub(1),
        Some(s) if pgdn => (s + page).min(n - 1),
        Some(s) if pgup => s.saturating_sub(page),
        None if down || up || pgdn || pgup => 0,
        _ => return None,
    };
    view.log.scroll_to = Some(target);
    Some(if shift { RowAction::Extend(target) } else { RowAction::Select(target) })
}

/// One commit row to paint: its data, graph segment, geometry and selection state.
struct RowData<'a> {
    i: usize,
    c: &'a Commit,
    graph: &'a RowPrint,
    rect: Rect,
    labels: &'a [RefLabel],
    is_sel: bool,
}

/// Paints one commit row and records its click / context-menu actions.
fn row(ui: &mut egui::Ui, ctx: &RowCtx, r: &RowData, actions: &mut Vec<RowAction>) {
    let resp = ui.interact(r.rect, ui.scope_id().with(("log_row", r.i)), Sense::click());
    paint_background(ui, ctx, r, resp.hovered());
    paint_cells(ui, ctx, r);
    handle_interaction(ui, ctx, r, resp, actions);
}

/// Selection / hover background, inset so it reads as a rounded pill.
fn paint_background(ui: &egui::Ui, ctx: &RowCtx, r: &RowData, hovered: bool) {
    let painter = ui.painter_at(r.rect);
    let bg = r.rect.shrink2(Vec2::new(2.0, 1.0));
    if r.is_sel {
        // Soft orange tint with an orange outline.
        let o = crate::theme::sys::orange(ctx.dark);
        painter.rect_filled(bg, 6.0, o.gamma_multiply(if ctx.dark { 0.30 } else { 0.28 }));
        painter.rect_stroke(bg, 6.0, Stroke::new(1.0, o.gamma_multiply(0.45)), egui::StrokeKind::Inside);
    } else if hovered {
        painter.rect_filled(bg, 6.0, ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.5));
    }
}

/// Paints the graph, subject, labels and metadata cells.
fn paint_cells(ui: &egui::Ui, ctx: &RowCtx, r: &RowData) {
    let (c, rect, labels) = (r.c, r.rect, r.labels);
    let (dark, label_pos) = (ctx.dark, ctx.label_pos);
    let label_style = label_prefs(ui.ctx()).1;
    let painter = ui.painter_at(rect);
    let row_h = rect.height();
    let cy = rect.top() + if label_pos == LabelPosition::Top { row_h - ROW_H } else { 0.0 } + ROW_H / 2.0;
    let is_merge = c.parents.len() > 1;
    let weak_color = ui.visuals().weak_text_color();
    let text_color = if is_merge && !r.is_sel { weak_color } else { ui.visuals().text_color() };

    let Columns { order, fixed, visible: vis } = ctx.columns;
    let (lefts, ws) = col_layout(order, fixed, rect.width());
    // The leftmost column is inset so its content clears the rounded selection background.
    let cell = |k: usize| {
        let inset = if order[0] == k { 8.0 } else { 0.0 };
        Rect::from_min_size(Pos2::new(rect.left() + lefts[k] + inset, rect.top()), Vec2::new(ws[k] - inset, row_h))
    };

    let sc = cell(0);
    let is_head = ctx.head_hash.as_deref() == Some(c.hash.as_str());
    let graph_w = paint_graph(&painter, Rect::from_min_max(sc.min, rect.right_bottom()), cy, r.graph, is_head, is_merge, dark);
    let subject_left = sc.left() + graph_w + 4.0;
    let subject_rect = Rect::from_min_max(
        Pos2::new(subject_left, rect.top()),
        Pos2::new((sc.right() - 8.0).max(subject_left), rect.bottom()),
    );
    paint_subject_and_labels(
        &ui.painter_at(subject_rect), subject_rect.left() + 2.0, rect.top(), cy, &c.subject, text_color, !is_merge,
        labels, label_pos, label_style, dark,
    );
    if vis[1] && !labels.is_empty() {
        let mut col = cell(1);
        col.max.x -= 4.0;
        paint_pills(&ui.painter_at(col), col.left(), cy, labels, label_style, dark, text_color, weak_color);
    }
    if vis[2] {
        let ac = cell(2);
        let ap = ui.painter_at(Rect::from_min_max(ac.min, Pos2::new(ac.right() - 6.0, ac.bottom())));
        paint_avatar(&ap, Rect::from_center_size(Pos2::new(ac.left() + 8.0, cy), Vec2::splat(16.0)), &c.author_name, dark);
        ap.text(Pos2::new(ac.left() + 22.0, cy), Align2::LEFT_CENTER, &c.author_name, FontId::proportional(13.0), text_color);
    }
    let text_cell = |k: usize, text: &str, font: FontId| {
        if vis[k] {
            painter.text(Pos2::new(cell(k).left(), cy), Align2::LEFT_CENTER, text, font, text_color);
        }
    };
    text_cell(3, &c.hash[..c.hash.len().min(7)], FontId::monospace(12.5));
    text_cell(4, &format_time(c.commit_time), FontId::proportional(13.0));
}

/// Click selection, hover tooltip and the context menu.
fn handle_interaction(ui: &egui::Ui, ctx: &RowCtx, r: &RowData, resp: egui::Response, actions: &mut Vec<RowAction>) {
    let (i, c) = (r.i, r.c);
    if resp.clicked() {
        let m = ui.input(|i| i.modifiers);
        actions.push(if m.shift {
            RowAction::Extend(i)
        } else if m.command || m.ctrl {
            RowAction::Toggle(i)
        } else {
            RowAction::Select(i)
        });
    } else if resp.secondary_clicked() && !r.is_sel {
        // Right-click inside the selection keeps it, so the menu acts on all of it.
        actions.push(RowAction::Select(i));
    }
    resp.on_hover_text_at_pointer(format!(
        "{}\n{} <{}>\n{}",
        c.short_hash(),
        c.author_name,
        c.author_email,
        crate::ui::format_full_time(c.author_time)
    ))
    .context_menu(|ui| {
        if r.is_sel && ctx.sel_commits.len() > 1 {
            multi_menu(ui, &ctx.sel_commits, ctx.current.as_deref(), ctx.repo_state, actions);
        } else {
            row_menu(ui, c, ctx.head_hash.as_deref() == Some(c.hash.as_str()), ctx.current.as_deref(), ctx.repo_state, actions);
        }
    });
}
