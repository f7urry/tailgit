//! Column header: drag a title to reorder, drag a divider to resize.

use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use super::columns::{col_layout, col_order, log_columns, set_log_columns, col_visibility, label_prefs, ColWidths, COL_NAMES};
use super::{MIN_COL_W, MIN_SUBJECT_W};
use crate::settings::LabelPosition;

const HEADER_H: f32 = 20.0;

/// Final column layout for the rows, after any reorder / resize this frame.
pub(super) struct Columns {
    pub order: [usize; 5],
    /// Fixed widths per column id (0 for Subject and hidden columns).
    pub fixed: [f32; 5],
    pub visible: [bool; 5],
}

pub(super) fn header(ui: &mut egui::Ui) -> Columns {
    let (hrect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEADER_H), Sense::hover());
    let label_col = label_prefs(ui.ctx()).0 == LabelPosition::Column;
    let mut cols = load_widths(ui);
    let max_meta = (hrect.width() - MIN_SUBJECT_W - cols.with_branches(label_col)).max(MIN_COL_W * 3.0);
    shrink_to_fit(&mut cols, max_meta);

    let vis = col_visibility(ui.ctx());
    let visible = [true, label_col, vis[0], vis[1], vis[2]];
    let fixed_of = |c: &ColWidths| {
        let on = |k: usize, w: f32| if visible[k] { w } else { 0.0 };
        [0.0, on(1, c.branches), on(2, c.author), on(3, c.hash), on(4, c.date)]
    };

    let order = reorder_cells(ui, hrect, col_order(ui.ctx()), fixed_of(&cols), visible);
    let mut state = log_columns(ui.ctx());
    state.order = order;
    set_log_columns(ui.ctx(), state);
    ui.painter_at(hrect).line_segment(
        [hrect.left_bottom(), hrect.right_bottom()],
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );

    let new_cols = resize_handles(ui, hrect, order, fixed_of(&cols), visible, cols);
    cols.branches = new_cols.branches;
    if new_cols.meta() <= max_meta || new_cols.meta() < cols.meta() {
        cols = new_cols;
    }
    let mut state = log_columns(ui.ctx());
    state.widths = [cols.branches, cols.author, cols.hash, cols.date];
    set_log_columns(ui.ctx(), state);

    Columns { order, fixed: fixed_of(&cols), visible }
}

fn load_widths(ui: &egui::Ui) -> ColWidths {
    let [branches, author, hash, date] = log_columns(ui.ctx()).widths;
    ColWidths { branches, author, hash, date }
}

/// Keep the subject usable when the window shrinks.
fn shrink_to_fit(cols: &mut ColWidths, max_meta: f32) {
    while cols.meta() > max_meta {
        let f = max_meta / cols.meta();
        cols.author = (cols.author * f).max(MIN_COL_W);
        cols.hash = (cols.hash * f).max(MIN_COL_W);
        cols.date = (cols.date * f).max(MIN_COL_W);
        if f >= 1.0 || cols.meta() <= MIN_COL_W * 3.0 + 0.5 {
            break;
        }
    }
}

/// Paints header titles; dragging one reorders the columns. Returns the new order.
fn reorder_cells(ui: &egui::Ui, hrect: Rect, mut order: [usize; 5], fixed: [f32; 5], visible: [bool; 5]) -> [usize; 5] {
    let (left, w) = col_layout(order, fixed, hrect.width());
    let hp = ui.painter_at(hrect);
    let weak = ui.visuals().weak_text_color();
    let font = FontId::proportional(13.0);
    for k in (0..5).filter(|&k| visible[k]) {
        let x = hrect.left() + left[k];
        let cell = Rect::from_min_max(Pos2::new(x, hrect.top()), Pos2::new(x + w[k], hrect.bottom()));
        let r = ui.interact(cell, ui.scope_id().with(("log_col_move", k)), Sense::drag());
        if r.hovered() && !r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            hp.rect_filled(
                cell.shrink2(Vec2::new(0.0, 1.0)),
                4.0,
                ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.6),
            );
            if let Some(px) = r.interact_pointer_pos().map(|p| p.x) {
                let mut keyed: Vec<(f32, usize)> = order
                    .iter()
                    .copied()
                    .filter(|&j| visible[j])
                    .map(|j| (if j == k { px } else { hrect.left() + left[j] + w[j] / 2.0 }, j))
                    .collect();
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut new_order: Vec<usize> = keyed.into_iter().map(|(_, j)| j).collect();
                new_order.extend(order.iter().copied().filter(|&j| !visible[j]));
                order = [new_order[0], new_order[1], new_order[2], new_order[3], new_order[4]];
            }
        }
        let tx = x + if k == 0 { 6.0 } else { 0.0 };
        ui.painter_at(Rect::from_min_max(Pos2::new(x, hrect.top()), Pos2::new(x + w[k] - 4.0, hrect.bottom())))
            .text(Pos2::new(tx, hrect.center().y), Align2::LEFT_CENTER, COL_NAMES[k], font.clone(), weak);
    }
    order
}

/// Resize handles for Branches / Author / Hash / Date. A column right of Subject is resized from its
/// left edge (drag left to widen); one left of Subject from its right edge (drag right).
fn resize_handles(
    ui: &egui::Ui,
    hrect: Rect,
    order: [usize; 5],
    fixed: [f32; 5],
    visible: [bool; 5],
    mut cols: ColWidths,
) -> ColWidths {
    let (left, w) = col_layout(order, fixed, hrect.width());
    let hp = ui.painter_at(hrect);
    let pos_of = |k: usize| order.iter().position(|&j| j == k).unwrap_or(0);
    for k in (1..5).filter(|&k| visible[k]) {
        let right_of_subject = pos_of(k) > pos_of(0);
        let x = hrect.left() + left[k];
        let line_x = if right_of_subject { x - 4.0 } else { x + w[k] - 2.0 };
        let hit = Rect::from_center_size(Pos2::new(line_x, hrect.center().y), Vec2::new(8.0, HEADER_H));
        let r = ui.interact(hit, ui.scope_id().with(("log_col_handle", k)), Sense::drag());
        let active = r.hovered() || r.dragged();
        if active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let color = if active {
            ui.visuals().widgets.active.bg_stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        hp.line_segment(
            [Pos2::new(line_x, hrect.top() + 4.0), Pos2::new(line_x, hrect.bottom() - 4.0)],
            Stroke::new(1.0, color),
        );
        if r.dragged() {
            let width = match k {
                1 => &mut cols.branches,
                2 => &mut cols.author,
                3 => &mut cols.hash,
                _ => &mut cols.date,
            };
            let dx = r.drag_delta().x;
            *width = (*width + if right_of_subject { -dx } else { dx }).max(MIN_COL_W);
        }
    }
    cols
}
