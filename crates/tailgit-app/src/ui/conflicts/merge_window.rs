//! The merge window, laid out like IntelliJ's three-side merge dialog:
//!
//! ```text
//! toolbar:  ↑ ↓  ⤓   Apply non-conflicting changes: ≫ Left  ≫≪ All  ≪ Right   ✨ ↶        5 changes, 1 conflict ⚙
//! header:   🔒 Local Changes   Show Details │ Result  path/… │ 🔒 Changes from …   Show Details
//! panes:    left text │ ✕ ≫ nums │ nums  result (editable) │ nums ≪ ✕ │ right text │ stripe
//! buttons:  [Accept Left] [Accept Right]                             [Save and Close] [Apply Changes]
//! ```
//!
//! It opens in its own native window. Rows have a fixed height, text is not wrapped
//! (the three panes scroll horizontally together) and changes are joined to the result
//! by smooth bands drawn across the gutters.

use std::collections::HashSet;
use std::ops::Range;

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Margin, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui,
    UiBuilder,
};
use git_core::merge3::split_lines;
use git_core::merge_context::{self, Span};
use git_core::merge_model::{self, ApplyFrom, MergeChange, MergeModel, Side};

use super::syntax::{self, Lang, Palette};
use super::{accept_op, ask, elided, rich, Confirm, MergeEditor};
use crate::ops::Op;
use crate::repo::RepoView;
use crate::settings::Settings;

const ROW_H: f32 = 22.0;
const MONO: f32 = 13.0;
const STRIPE_W: f32 = 14.0;
const HBAR_H: f32 = 12.0;

// ---------------------------------------------------------------- entry point

pub fn show(ctx: &egui::Context, view: &mut RepoView, settings: &mut Settings) {
    let busy = view.busy.is_some();
    let root = view.root.trim_end_matches('/').to_string();
    let Some(ed) = view.merge_editor.as_mut() else { return };
    let mut finish: Option<Option<Op>> = None; // Some(None) = cancel, Some(Some(op)) = run op
    let title = format!("Merge Revisions for {}/{}", root, ed.file.path);
    let size = ctx
        .input(|i| i.viewport().monitor_size)
        .map(|m| [(m.x * 0.92).clamp(900.0, 2000.0), (m.y * 0.86).clamp(520.0, 1100.0)])
        .unwrap_or([1400.0, 800.0]);
    let builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size([780.0, 420.0]);

    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("tailgit_merge_window"), builder, |ui, _class| {
        body(ui, ed, settings, busy, &mut finish);
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if ed.modified() {
                ed.confirm = Some(Confirm::Cancel);
            } else if finish.is_none() {
                finish = Some(None);
            }
        }
    });

    match finish {
        Some(Some(op)) => view.run(op),
        Some(None) => {
            // Cancelling one file stops a multi-file Merge….
            view.merge_editor = None;
            if let Some(d) = &mut view.conflicts_dialog {
                d.queue.clear();
            }
            view.jobs.load_conflict_info();
        }
        None => {}
    }
}

// ---------------------------------------------------------------- geometry & painting helpers

/// Horizontal layout of the table, shared by the header and every row.
#[derive(Clone, Copy)]
struct Geo {
    x0: f32,
    right: f32,
    lt: (f32, f32), // left text
    lg: (f32, f32), // left gutter: ✕ ≫ and the left line numbers
    rn: (f32, f32), // result line numbers (+ the resolve wand)
    rt: (f32, f32), // result text
    rg: (f32, f32), // right gutter: right line numbers, ≪ ✕
    rx: (f32, f32), // right text
}

impl Geo {
    fn new(left: f32, right: f32, split: [f32; 2]) -> Geo {
        let x0 = left + 10.0;
        let right = right - STRIPE_W;
        let (lg, rn, rg) = (92.0, 72.0, 114.0);
        let texts = ((right - x0) - (lg + rn + rg)).max(150.0);
        let (tl, tm) = (texts * split[0], texts * split[1]);
        let tr = texts - tl - tm;
        let mut x = x0;
        let mut next = |w: f32| {
            let r = (x, x + w);
            x += w;
            r
        };
        let lt = next(tl);
        let lg = next(lg);
        let rn = next(rn);
        let rt = next(tm);
        let rg = next(rg);
        let rx = next(tr);
        Geo { x0, right: rx.1, lt, lg, rn, rt, rg, rx }
    }
}

struct Cx {
    lang: Lang,
    pal: Palette,
    font: FontId,
}

fn visible_rows(vis: &Rect, y: f32, n: usize) -> Range<usize> {
    let a = ((vis.top() - y) / ROW_H).floor().max(0.0) as usize;
    let b = (((vis.bottom() - y) / ROW_H).ceil().max(0.0) as usize).min(n);
    a.min(n)..b
}

fn paint_lines(painter: &Painter, vis: &Rect, cx: &Cx, lines: &[String], x: f32, y: f32) {
    for k in visible_rows(vis, y, lines.len()) {
        let body = lines[k].trim_end_matches(['\n', '\r']);
        if body.is_empty() {
            continue;
        }
        let job = syntax::job(&body.replace('\t', "    "), cx.lang, &cx.pal, &cx.font, ROW_H);
        let g = painter.layout_job(job);
        painter.galley(pos2(x, y + k as f32 * ROW_H), g, cx.pal.text);
    }
}

fn paint_numbers(painter: &Painter, vis: &Rect, pal: &Palette, n: usize, first: usize, right_x: f32, y: f32) {
    let font = FontId::monospace(12.0);
    for k in visible_rows(vis, y, n) {
        painter.text(
            pos2(right_x, y + k as f32 * ROW_H + ROW_H / 2.0),
            Align2::RIGHT_CENTER,
            (first + k + 1).to_string(),
            font.clone(),
            pal.lineno,
        );
    }
}

/// Smooth connector between two columns; `from`/`to` are `(top, bottom)` at `x0`/`x1`.
fn band(painter: &Painter, x0: f32, x1: f32, from: (f32, f32), to: (f32, f32), fill: Option<Color32>, stroke: Stroke) {
    if x1 <= x0 {
        return;
    }
    const N: usize = 24;
    let mut top = Vec::with_capacity(N + 1);
    let mut bot = Vec::with_capacity(N + 1);
    for k in 0..=N {
        let u = k as f32 / N as f32;
        let s = u * u * (3.0 - 2.0 * u);
        let x = x0 + (x1 - x0) * u;
        top.push(pos2(x, from.0 + (to.0 - from.0) * s));
        bot.push(pos2(x, from.1 + (to.1 - from.1) * s));
    }
    if let Some(color) = fill {
        let mut mesh = egui::Mesh::default();
        for k in 0..=N {
            mesh.colored_vertex(top[k], color);
            mesh.colored_vertex(bot[k], color);
            if k > 0 {
                let p = 2 * (k as u32 - 1);
                mesh.add_triangle(p, p + 1, p + 2);
                mesh.add_triangle(p + 1, p + 3, p + 2);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.add(egui::Shape::line(top, stroke));
    if (from.1 - from.0).abs() > 0.5 || (to.1 - to.0).abs() > 0.5 {
        painter.add(egui::Shape::line(bot, stroke));
    }
}

/// A small flat icon button centred on `center` (gutter buttons).
fn icon_btn(ui: &mut Ui, pal: &Palette, center: Pos2, glyph: &str, size: f32) -> egui::Response {
    let rect = Rect::from_center_size(center, vec2(size, size));
    let id = ui.scope_id().with(("mw_icon", glyph, center.x as i32, center.y as i32));
    let resp = ui.interact(rect, id, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, pal.hover);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(12.0), pal.icon);
    resp
}

/// Flat toolbar button: optional icon glyph followed by an optional label.
fn tool_btn(ui: &mut Ui, pal: &Palette, icon: &str, label: &str, enabled: bool, on: bool) -> egui::Response {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    let icon_col = if enabled { pal.icon } else { pal.icon_dim };
    let text_col = if enabled { pal.text } else { pal.icon_dim };
    if !icon.is_empty() {
        job.append(icon, 0.0, TextFormat::simple(FontId::proportional(13.0), icon_col));
    }
    if !label.is_empty() {
        let lead = if icon.is_empty() { 0.0 } else { 7.0 };
        job.append(label, lead, TextFormat::simple(FontId::proportional(13.0), text_col));
    }
    let g = ui.painter().layout_job(job);
    let size = vec2((g.size().x + 14.0).max(28.0), 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if (enabled && resp.hovered()) || on {
        ui.painter().rect_filled(rect, 6.0, pal.hover);
    }
    ui.painter().galley(rect.center() - g.size() / 2.0, g, text_col);
    resp
}

/// `microservices/…source/File.java`: shortens the middle of a path to fit `max_w`.
fn middle_elide(ui: &Ui, text: &str, font: &FontId, max_w: f32) -> String {
    let width = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x;
    if width(text) <= max_w {
        return text.to_owned();
    }
    let head_end = text.find('/').map_or(0, |p| p + 1);
    let head = &text[..head_end];
    let mut tail_start = head_end;
    loop {
        let tail = &text[tail_start..];
        let s = format!("{head}…{tail}");
        if width(&s) <= max_w || tail.is_empty() {
            return s;
        }
        tail_start += tail.chars().next().map_or(1, char::len_utf8);
    }
}

fn line_width(l: &str) -> usize {
    l.trim_end_matches(['\n', '\r']).chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

// ---------------------------------------------------------------- the window body

/// What the user did in the change list this frame.
pub(super) enum Act {
    Accept(usize, Side, bool),
    Ignore(usize, Side, bool),
    IgnoreAll(usize),
    Resolve(usize),
    EditChange(usize, String),
    EditGap(Range<usize>, String),
    Expand(usize),
}

fn body(ui: &mut Ui, ed: &mut MergeEditor, settings: &mut Settings, busy: bool, finish: &mut Option<Option<Op>>) {
    let dark = ui.visuals().dark_mode;
    let pal = syntax::palette(dark);
    let path = ed.file.path.clone();
    let modifier = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
    let frame = |m: Margin| egui::Frame::new().fill(pal.bg).inner_margin(m);
    let body_font = FontId::proportional(13.0);

    // ---- toolbar
    let mut nav: Option<bool> = None;
    let mut revert = false;
    let mut applied = false;
    let mut collapse = ed.collapse;
    egui::Panel::top("mw_toolbar")
        .frame(frame(Margin { left: 12, right: 12, top: 8, bottom: 2 }))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                let Some(m) = ed.model.as_mut() else {
                    ui.label(RichText::new("Binary files can only be resolved by accepting one side as a whole.").color(pal.text));
                    return;
                };
                if tool_btn(ui, &pal, "\u{f062}", "", true, false).on_hover_text("Previous Difference").clicked() {
                    nav = Some(false);
                }
                if tool_btn(ui, &pal, "\u{f063}", "", true, false).on_hover_text("Next Difference").clicked() {
                    nav = Some(true);
                }
                ui.add_space(8.0);
                if tool_btn(ui, &pal, "\u{f066}", "", true, collapse)
                    .on_hover_text("Collapse Unchanged Fragments")
                    .clicked()
                {
                    collapse = !collapse;
                }
                ui.add_space(14.0);
                ui.label(RichText::new("Apply non-conflicting changes:").color(pal.text));
                ui.add_space(4.0);
                for (icon, label, from, tip) in [
                    ("\u{f101}", "Left", ApplyFrom::Left, "Apply Non-Conflicting Changes from the Left Side"),
                    ("\u{f101}\u{f100}", "All", ApplyFrom::All, "Apply All Non-Conflicting Changes"),
                    ("\u{f100}", "Right", ApplyFrom::Right, "Apply Non-Conflicting Changes from the Right Side"),
                ] {
                    let can = m.can_apply_non_conflicting(from);
                    if tool_btn(ui, &pal, icon, label, can, false).on_hover_text(tip).clicked() {
                        m.apply_non_conflicting(from);
                        applied = true;
                    }
                }
                ui.add_space(8.0);
                if tool_btn(ui, &pal, "\u{f0d0}", "", m.can_resolve_simple(), false)
                    .on_hover_text("Resolve simple conflicts: changes that do not overlap at word level or differ only in whitespace")
                    .clicked()
                {
                    m.resolve_simple();
                    applied = true;
                }
                let can_revert = m.modified || m.changes.iter().any(|c| c.is_resolved());
                if tool_btn(ui, &pal, "\u{f0e2}", "", can_revert, false)
                    .on_hover_text("Revert Conflict Resolution")
                    .clicked()
                {
                    revert = true;
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let gear = tool_btn(ui, &pal, "\u{f013}", "", true, false).on_hover_text("Settings");
                    egui::Popup::from_toggle_button_response(&gear)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.set_min_width(280.0);
                            if ui
                                .checkbox(&mut settings.merge_auto_apply, "Automatically apply non-conflicting changes")
                                .changed()
                            {
                                settings.save();
                            }
                            ui.checkbox(&mut collapse, "Collapse unchanged fragments");
                        });
                    ui.add_space(6.0);
                    let (ch, co) = m.unresolved();
                    let text = merge_model::status_text(ch, co);
                    let color = if ch == 0 && co == 0 { crate::theme::sys::green(dark) } else { pal.text };
                    ui.label(RichText::new(text).color(color));
                });
            });
        });
    if collapse != ed.collapse {
        ed.collapse = collapse;
        ed.expanded.clear();
    }
    if revert {
        ed.confirm = Some(Confirm::Revert);
    }
    if let Some(forward) = nav {
        ed.scroll_to = ed.next_unresolved(ed.current, forward, false);
    }
    if applied {
        // Like IntelliJ, scroll to the first unresolved change afterwards.
        ed.scroll_to = ed.next_unresolved(None, true, false);
    }

    // ---- column headers
    egui::Panel::top("mw_header").frame(frame(Margin::ZERO)).show_separator_line(false).show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        let g = Geo::new(rect.left(), rect.right(), ed.view_split());
        let painter = ui.painter().clone();
        let cy = rect.center().y;
        painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, pal.sep));

        let side = |ui: &mut Ui, icon_x: f32, text_x: f32, max_right: f32, spans: &[Span], link_right: f32, id: &str| {
            painter.text(pos2(icon_x, cy), Align2::CENTER_CENTER, "\u{f023}", FontId::proportional(11.0), pal.icon);
            let full = merge_context::to_plain(spans);
            let link = painter.layout_no_wrap("Show Details".into(), body_font.clone(), pal.link);
            let lrect = Rect::from_min_size(pos2(link_right - link.size().x, cy - link.size().y / 2.0), link.size());
            let gal = elided(ui, &full, body_font.clone(), pal.text, (max_right - link.size().x - 16.0 - text_x).max(40.0), None);
            let truncated = gal.elided;
            let trect = Rect::from_min_size(pos2(text_x, cy - gal.size().y / 2.0), gal.size());
            painter.galley(trect.min, gal, pal.text);
            if truncated {
                ui.interact(trect, ui.scope_id().with((id, "t")), Sense::hover()).on_hover_text(full);
            }
            let resp = ui.interact(lrect.expand(3.0), ui.scope_id().with((id, "l")), Sense::click());
            painter.galley(lrect.min, link, pal.link);
            if resp.hovered() {
                painter.hline(lrect.x_range(), lrect.bottom() + 1.0, Stroke::new(1.0, pal.link));
            }
            resp
        };
        let left = side(ui, g.lt.0 + 13.0, g.lt.0 + 21.0, g.lg.1, &ed.context.left_title, g.lg.1 - 12.0, "left");
        let right = side(ui, g.rg.0 + 38.0, g.rg.0 + 47.0, g.rx.1, &ed.context.right_title, g.rx.1 + 4.0, "right");
        for (resp, spans) in [(&left, &ed.context.left_title), (&right, &ed.context.right_title)] {
            egui::Popup::from_toggle_button_response(resp)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_max_width(460.0);
                    ui.label(RichText::new(merge_context::to_plain(spans)).strong());
                    ui.add_space(4.0);
                    rich(ui, &ed.context.description);
                });
        }
        // Result title and path.
        painter.text(pos2(g.rn.0 + 26.0, cy), Align2::LEFT_CENTER, "Result", body_font.clone(), pal.text);
        let shown = middle_elide(ui, &path, &body_font, (g.rt.1 - g.rt.0 - 12.0).max(80.0));
        let tg = painter.layout_no_wrap(shown, body_font.clone(), pal.lineno);
        painter.galley(pos2(g.rt.0, cy - tg.size().y / 2.0), tg, pal.lineno);
    });

    // ---- bottom buttons
    egui::Panel::bottom("mw_buttons").frame(frame(Margin { left: 14, right: 14, top: 10, bottom: 12 })).show_separator_line(false).show(ui, |ui| {
        ui.spacing_mut().button_padding = vec2(14.0, 6.0);
        ui.spacing_mut().item_spacing.x = 10.0;
        let secondary = |ui: &mut Ui, label: &str, enabled: bool| {
            ui.add_enabled(
                enabled,
                egui::Button::new(RichText::new(label).color(pal.text))
                    .fill(pal.bg)
                    .stroke(Stroke::new(1.0, pal.btn_border))
                    .corner_radius(7.0)
                    .min_size(vec2(0.0, 30.0)),
            )
        };
        ui.horizontal(|ui| {
            let cx = ui.max_rect().center().x;
            crate::ui::layout_switcher(ui, cx, &mut ed.layout, "merge");
            for (label, side) in [("Accept Left", Side::Left), ("Accept Right", Side::Right)] {
                if secondary(ui, label, !busy).clicked() {
                    if ed.modified() {
                        ed.confirm = Some(Confirm::Accept(side));
                    } else {
                        *finish = Some(Some(accept_op(&path, side)));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let can = ed.model.is_some() && !busy;
                let apply = ui.add_enabled(
                    can,
                    egui::Button::new(RichText::new("Apply Changes").color(Color32::WHITE))
                        .fill(pal.primary)
                        .stroke(Stroke::NONE)
                        .corner_radius(7.0)
                        .min_size(vec2(0.0, 30.0)),
                );
                if apply.clicked() {
                    if let Some(m) = &ed.model {
                        let (ch, co) = m.unresolved();
                        if ch == 0 && co == 0 {
                            *finish = Some(Some(Op::WriteResolved { path: path.clone(), content: m.result_text().into_bytes() }));
                        } else {
                            ed.confirm = Some(Confirm::Apply);
                        }
                    }
                }
                if secondary(ui, "Save and Close", can).on_hover_text("Save the result without marking the file resolved").clicked() {
                    if let Some(m) = &ed.model {
                        *finish = Some(Some(Op::SaveResult { path: path.clone(), content: m.result_text().into_bytes() }));
                    }
                }
            });
        });
    });

    // ---- the three panes
    egui::CentralPanel::default().frame(frame(Margin::ZERO)).show(ui, |ui| panes(ui, ed, &pal, modifier));

    // ---- confirmations (exact IntelliJ wording)
    if let Some(c) = ed.confirm {
        let (title, text, yes, no) = match c {
            Confirm::Apply => {
                let (ch, co) = ed.model.as_ref().map(|m| m.unresolved()).unwrap_or((0, 0));
                (
                    "Apply Changes".to_string(),
                    merge_model::unprocessed_message(ch, co),
                    "Apply Changes and Mark Resolved".to_string(),
                    "Continue Merge",
                )
            }
            Confirm::Accept(side) => {
                let name = if side == Side::Left { "Left" } else { "Right" };
                (
                    format!("Accept {name}"),
                    format!(
                        "There are unsaved changes in the result file. Discard changes and accept {} anyway?",
                        name.to_lowercase()
                    ),
                    format!("Discard Changes and Accept {name}"),
                    "Continue Merge",
                )
            }
            Confirm::Cancel => (
                "Cancel Merge".to_string(),
                "There are unsaved changes in the result file. Discard changes and cancel merge anyway?".to_string(),
                "Discard Changes and Cancel Merge".to_string(),
                "Continue Merge",
            ),
            Confirm::Revert => (
                "Confirm Revert".to_string(),
                "All changes will be reverted. The file will return to its original conflicted state.".to_string(),
                "Revert".to_string(),
                "Cancel",
            ),
        };
        match ask(ui.ctx(), &title, &title, &text, &yes, no) {
            Some(true) => {
                ed.confirm = None;
                match c {
                    Confirm::Apply => {
                        if let Some(m) = &ed.model {
                            *finish = Some(Some(Op::WriteResolved { path: path.clone(), content: m.result_text().into_bytes() }));
                        }
                    }
                    Confirm::Accept(side) => *finish = Some(Some(accept_op(&path, side))),
                    Confirm::Cancel => *finish = Some(None),
                    Confirm::Revert => {
                        if let Some(m) = ed.model.as_mut() {
                            m.revert();
                        }
                    }
                }
            }
            Some(false) => ed.confirm = None,
            None => {}
        }
    }
}

// ---------------------------------------------------------------- panes

struct Env<'a> {
    m: &'a MergeModel,
    cx: &'a Cx,
    geo: Geo,
    hscroll: f32,
    edit_w: f32,
    modifier: bool,
    collapse: bool,
    expanded: &'a HashSet<usize>,
    current: Option<usize>,
    scroll_to: Option<usize>,
}

#[derive(Default)]
struct Out {
    acts: Vec<Act>,
    /// Stripe marks: (y in content, height, colour).
    marks: Vec<(f32, f32, Color32)>,
    /// New horizontal offset requested by the result editor (keeping the caret visible).
    hscroll: Option<f32>,
}

fn panes(ui: &mut Ui, ed: &mut MergeEditor, pal: &Palette, modifier: bool) {
    if ed.model.is_none() {
        ui.add_space(12.0);
        ui.label(
            RichText::new(match (&ed.file.yours, &ed.file.theirs) {
                (None, _) | (_, None) => "One side deleted this file.",
                _ => "The versions are binary and cannot be merged line by line.",
            })
            .color(pal.lineno),
        );
        return;
    }
    let area = ui.available_rect_before_wrap();
    let geo = Geo::new(area.left(), area.right(), ed.view_split());
    let scroll_rect = Rect::from_min_max(area.min, pos2(area.max.x - STRIPE_W, area.max.y - HBAR_H));
    let stripe = Rect::from_min_max(pos2(area.max.x - STRIPE_W, area.min.y), pos2(area.max.x, area.max.y - HBAR_H));
    let hbar_y = area.max.y - HBAR_H / 2.0;

    let cx = Cx { lang: Lang::from_path(&ed.file.path), pal: *pal, font: FontId::monospace(MONO) };
    let cw = ui.painter().layout_no_wrap("M".into(), cx.font.clone(), Color32::WHITE).size().x;
    let (content_w, pane_w) = {
        let m = ed.model.as_ref().expect("model");
        let widest = |v: &[String]| v.iter().map(|l| line_width(l)).max().unwrap_or(0) as f32 * cw + 48.0;
        (
            [widest(&m.left), widest(&m.result), widest(&m.right)],
            [geo.lt.1 - geo.lt.0, geo.rt.1 - geo.rt.0, geo.rx.1 - geo.rx.0],
        )
    };
    let max_h = (0..3).map(|k| (content_w[k] - pane_w[k]).max(0.0)).fold(0.0, f32::max);

    // Shared horizontal scrolling (trackpad / shift+wheel over the panes).
    if ui.rect_contains_pointer(scroll_rect) {
        let dx = ui.input(|i| i.smooth_scroll_delta.x);
        if dx != 0.0 {
            ed.hscroll -= dx;
        }
    }
    ed.hscroll = ed.hscroll.clamp(0.0, max_h);

    let scroll_to = ed.scroll_to.take();
    let mut sa = egui::ScrollArea::vertical()
        .id_salt("merge_rows")
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(v) = ed.vscroll_req.take() {
        sa = sa.vertical_scroll_offset(v);
    }
    let m = ed.model.as_ref().expect("model");
    let env = Env {
        m,
        cx: &cx,
        geo,
        hscroll: ed.hscroll,
        edit_w: content_w[1].max(pane_w[1]),
        modifier,
        collapse: ed.collapse,
        expanded: &ed.expanded,
        current: ed.current,
        scroll_to,
    };
    let scrolled = ui
        .scope_builder(UiBuilder::new().max_rect(scroll_rect), |ui| {
            sa.show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let mut out = Out::default();
                let origin = ui.cursor().top();
                changes_view(ui, &env, &mut out, origin);
                out
            })
        })
        .inner;
    let mut out = scrolled.inner;
    if let Some(h) = out.hscroll.take() {
        ed.hscroll = h.clamp(0.0, max_h);
    }
    if let Some(i) = scroll_to {
        ed.current = Some(i);
    }

    // ---- vertical stripe: change marks and the scroll thumb
    let content_h = scrolled.content_size.y.max(1.0);
    let view_h = scrolled.inner_rect.height();
    let offset = scrolled.state.clamped_offset().y;
    {
        let painter = ui.painter();
        for (y, h, color) in &out.marks {
            let ty = stripe.top() + y / content_h * stripe.height();
            let th = (h / content_h * stripe.height()).max(3.0);
            painter.rect_filled(
                Rect::from_min_size(pos2(stripe.left() + 1.0, ty), vec2(stripe.width() - 2.0, th)),
                1.0,
                *color,
            );
        }
        if content_h > view_h + 1.0 {
            let th = (view_h / content_h * stripe.height()).max(24.0);
            let ty = stripe.top() + offset / (content_h - view_h) * (stripe.height() - th);
            painter.rect_filled(
                Rect::from_min_size(pos2(stripe.left() + 3.0, ty), vec2(stripe.width() - 6.0, th)),
                4.0,
                pal.thumb,
            );
        }
    }
    let track = ui.interact(stripe, ui.scope_id().with("mw_stripe"), Sense::click_and_drag());
    if track.dragged() || track.clicked() {
        if let Some(p) = track.interact_pointer_pos() {
            let v = (p.y - stripe.top()) / stripe.height().max(1.0) * content_h - view_h / 2.0;
            ed.vscroll_req = Some(v.clamp(0.0, (content_h - view_h).max(0.0)));
        }
    }

    // ---- horizontal bars under each pane
    for (k, (x0, x1)) in [geo.lt, geo.rt, geo.rx].into_iter().enumerate() {
        let (pw, cwk) = (pane_w[k], content_w[k]);
        if cwk <= pw + 1.0 {
            continue;
        }
        let tw = (pw * pw / cwk).max(28.0);
        let range = (cwk - pw).max(1.0);
        let tx = x0 + (pw - tw) * (ed.hscroll / range).clamp(0.0, 1.0);
        let thumb = Rect::from_min_size(pos2(tx, hbar_y - 3.0), vec2(tw, 6.0));
        let resp = ui.interact(thumb.expand2(vec2(0.0, 4.0)), ui.scope_id().with(("mw_hbar", k)), Sense::drag());
        if resp.dragged() && pw > tw {
            ed.hscroll = (ed.hscroll + resp.drag_delta().x * range / (pw - tw)).clamp(0.0, max_h);
        }
        ui.painter().rect_filled(thumb, 3.0, pal.thumb);
        let _ = x1;
    }

    // ---- draggable borders between the code panes
    if ed.layout == crate::ui::PaneLayout::All {
        const MIN: f32 = 0.12;
        let texts = ((geo.rx.1 - geo.x0) - (92.0 + 72.0 + 114.0)).max(150.0);
        for (k, x) in [geo.lt.1, geo.rt.1].into_iter().enumerate() {
            let hit = Rect::from_x_y_ranges((x - 4.0)..=(x + 4.0), scroll_rect.y_range());
            let resp = ui
                .interact(hit, ui.scope_id().with(("mw_split", k)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let active = resp.hovered() || resp.dragged();
            let color = if active { pal.link } else { pal.sep };
            ui.painter().vline(x, scroll_rect.y_range(), Stroke::new(if active { 2.0 } else { 1.0 }, color));
            if resp.dragged() {
                let d = resp.drag_delta().x / texts;
                let [mut a, mut b] = ed.split;
                if k == 0 {
                    a += d;
                    b -= d;
                } else {
                    b += d;
                }
                a = a.max(MIN);
                b = b.max(MIN);
                if a + b > 1.0 - MIN {
                    if k == 0 {
                        a = (1.0 - MIN - b).max(MIN);
                    } else {
                        b = (1.0 - MIN - a).max(MIN);
                    }
                }
                ed.split = [a, b];
            }
        }
    }

    // ---- apply what the user did
    let m = ed.model.as_mut().expect("model");
    for a in out.acts {
        match a {
            Act::Accept(i, side, resolve) => {
                m.accept(i, side, resolve);
                ed.current = Some(i);
            }
            Act::Ignore(i, side, resolve) => {
                m.ignore(i, side, resolve);
                ed.current = Some(i);
            }
            Act::IgnoreAll(i) => m.ignore_all(i),
            Act::Resolve(i) => m.resolve(i, None),
            Act::EditChange(i, text) => m.edit_change(i, split_lines(&text)),
            Act::EditGap(range, text) => m.edit_gap(range, split_lines(&text)),
            Act::Expand(g) => {
                ed.expanded.insert(g);
            }
        }
    }
}

enum Edit {
    None,
    Gap(usize, Range<usize>),
    Change(usize),
}

struct Row<'a> {
    left: &'a [String],
    lfirst: usize,
    mid: &'a [String],
    mfirst: usize,
    right: &'a [String],
    rfirst: usize,
}

/// Unchanged regions and changes, top to bottom.
fn changes_view(ui: &mut Ui, env: &Env, out: &mut Out, origin: f32) {
    let m = env.m;
    let pal = &env.cx.pal;
    let g = &env.geo;
    let (mut pl, mut pr, mut pres) = (0usize, 0usize, 0usize);
    let n = m.changes.len();
    for i in 0..=n {
        // Unchanged region before change i (or the tail).
        let (l_end, r_end, res_end) = match m.changes.get(i) {
            Some(c) => (c.left.start, c.right.start, c.result.start),
            None => (m.left.len(), m.right.len(), m.result.len()),
        };
        let gap_len = (l_end - pl).max(r_end - pr).max(res_end - pres);
        if gap_len > 0 {
            let (lg, mg, rg) = (&m.left[pl..l_end], &m.result[pres..res_end], &m.right[pr..r_end]);
            if env.collapse && gap_len > 8 && !env.expanded.contains(&i) {
                let head = 2usize;
                row(
                    ui,
                    env,
                    out,
                    origin,
                    Row {
                        left: &lg[..head.min(lg.len())],
                        lfirst: pl,
                        mid: &mg[..head.min(mg.len())],
                        mfirst: pres,
                        right: &rg[..head.min(rg.len())],
                        rfirst: pr,
                    },
                    None,
                    Edit::None,
                );
                let hidden = gap_len.saturating_sub(2 * head);
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                let painter = ui.painter();
                let band_rect = Rect::from_x_y_ranges(g.x0..=g.right, rect.y_range());
                painter.rect_filled(band_rect, 0.0, pal.fold);
                painter.hline(band_rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, pal.sep));
                painter.hline(band_rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, pal.sep));
                painter.text(
                    band_rect.center(),
                    Align2::CENTER_CENTER,
                    format!("\u{22ef}  {hidden} unchanged line{}  \u{22ef}", if hidden == 1 { "" } else { "s" }),
                    FontId::proportional(12.0),
                    pal.lineno,
                );
                if resp.on_hover_text("Expand unchanged lines").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    out.acts.push(Act::Expand(i));
                }
                let (tl, tm, tr) = (head.min(lg.len()), head.min(mg.len()), head.min(rg.len()));
                row(
                    ui,
                    env,
                    out,
                    origin,
                    Row {
                        left: &lg[lg.len() - tl..],
                        lfirst: pl + lg.len() - tl,
                        mid: &mg[mg.len() - tm..],
                        mfirst: pres + mg.len() - tm,
                        right: &rg[rg.len() - tr..],
                        rfirst: pr + rg.len() - tr,
                    },
                    None,
                    Edit::None,
                );
            } else {
                row(
                    ui,
                    env,
                    out,
                    origin,
                    Row { left: lg, lfirst: pl, mid: mg, mfirst: pres, right: rg, rfirst: pr },
                    None,
                    Edit::Gap(i, pres..res_end),
                );
            }
        }
        let Some(c) = m.changes.get(i) else { break };
        row(
            ui,
            env,
            out,
            origin,
            Row {
                left: &m.left[c.left.clone()],
                lfirst: c.left.start,
                mid: &m.result[c.result.clone()],
                mfirst: c.result.start,
                right: &m.right[c.right.clone()],
                rfirst: c.right.start,
            },
            Some((i, c)),
            Edit::Change(i),
        );
        pl = c.left.end;
        pr = c.right.end;
        pres = c.result.end;
    }
}

/// One aligned row group: the left lines, the result lines (editable) and the right lines,
/// joined by bands when it is a change.
fn row(ui: &mut Ui, env: &Env, out: &mut Out, origin: f32, r: Row, change: Option<(usize, &MergeChange)>, edit: Edit) {
    let (g, cx) = (&env.geo, env.cx);
    let pal = &cx.pal;
    let (nl, nm, nr) = (r.left.len(), r.mid.len(), r.right.len());
    let h = nl.max(nm).max(nr) as f32 * ROW_H;
    if h <= 0.0 {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let y = rect.top();
    let resolved = change.is_some_and(|(_, c)| c.is_resolved());
    if let Some((i, c)) = change {
        if !resolved {
            let color = if c.is_conflict() { pal.conflict_mark } else { pal.side_border };
            out.marks.push((y - origin, h, color));
        }
        if env.scroll_to == Some(i) {
            ui.scroll_to_rect(rect, Some(Align::Center));
        }
    }
    let vis = ui.clip_rect();
    if !vis.intersects(rect) {
        return;
    }
    let painter = ui.painter().with_clip_rect(vis);
    let lb = Rect::from_min_max(pos2(g.lt.0, y), pos2(g.lt.1, y + nl as f32 * ROW_H));
    let mb = Rect::from_min_max(pos2(g.rn.0, y), pos2(g.rt.1, y + nm as f32 * ROW_H));
    let rb = Rect::from_min_max(pos2(g.rx.0, y), pos2(g.rx.1, y + nr as f32 * ROW_H));

    // Fills and bands.
    let (mut pend_l, mut pend_r) = (false, false);
    if let Some((i, c)) = change {
        let conflict = c.is_conflict();
        let (side_fill, side_border, mid_fill) = if conflict {
            (pal.conflict_side_fill, pal.conflict_border, pal.conflict_fill)
        } else {
            (pal.side_fill, pal.side_border, pal.result_fill)
        };
        let w = if env.current == Some(i) { 1.5 } else { 1.0 };
        pend_l = c.left_changed && !resolved && !c.is_resolved_side(Side::Left);
        pend_r = c.right_changed && !resolved && !c.is_resolved_side(Side::Right);
        if resolved {
            if c.left_changed || c.right_changed {
                painter.rect_filled(mb, 0.0, pal.applied_fill);
            }
        } else {
            painter.rect_filled(mb, 0.0, mid_fill);
            painter.hline(mb.x_range(), mb.top(), Stroke::new(w, side_border));
            if nm > 0 {
                painter.hline(mb.x_range(), mb.bottom(), Stroke::new(w, side_border));
            }
        }
        for (changed, pending, block, x0, x1, from_mid) in [
            (c.left_changed, pend_l, lb, g.lt.1, g.rn.0, false),
            (c.right_changed, pend_r, rb, g.rt.1, g.rx.0, true),
        ] {
            if !changed {
                continue;
            }
            let (fill, stroke) = if pending {
                (Some(side_fill), Stroke::new(w, side_border))
            } else {
                (None, Stroke::new(1.0, pal.applied_border))
            };
            if let Some(f) = fill {
                painter.rect_filled(block, 0.0, f);
            }
            painter.hline(block.x_range(), block.top(), stroke);
            if !block.is_negative() && block.height() > 0.5 {
                painter.hline(block.x_range(), block.bottom(), stroke);
            }
            let (a, b) = ((mb.top(), mb.bottom()), (block.top(), block.bottom()));
            if from_mid {
                band(&painter, x0, x1, a, b, fill, stroke);
            } else {
                band(&painter, x0, x1, b, a, fill, stroke);
            }
        }
    }

    // Line numbers.
    paint_numbers(&painter, &vis, pal, nl, r.lfirst, g.lg.1 - 16.0, y);
    paint_numbers(&painter, &vis, pal, nm, r.mfirst, g.rn.1 - 18.0, y);
    paint_numbers(&painter, &vis, pal, nr, r.rfirst, g.rg.0 + 60.0, y);

    // Left and right text (clipped to their panes, scrolled together).
    let clip = |a: f32, b: f32| vis.intersect(Rect::from_x_y_ranges(a..=b, vis.y_range()));
    paint_lines(&painter.with_clip_rect(clip(g.lt.0, g.lt.1)), &vis, cx, r.left, g.lt.0 + 14.0 - env.hscroll, y);
    paint_lines(&painter.with_clip_rect(clip(g.rx.0, g.rx.1)), &vis, cx, r.right, g.rx.0 + 2.0 - env.hscroll, y);

    // Result text: editable unless it is a collapsed fragment.
    let mid_x = g.rt.0 + 2.0 - env.hscroll;
    if matches!(edit, Edit::None) || nm == 0 {
        paint_lines(&painter.with_clip_rect(clip(g.rt.0, g.rt.1)), &vis, cx, r.mid, mid_x, y);
    } else {
        let trailing = r.mid.last().is_some_and(|l| l.ends_with('\n'));
        let mut text: String = r.mid.concat();
        if trailing {
            text.pop();
        }
        let before = text.clone();
        let salt = match &edit {
            Edit::Gap(i, _) => ("mw_gap", *i),
            _ => ("mw_chg", change.map_or(0, |(i, _)| i)),
        };
        let ed_rect = Rect::from_min_size(pos2(mid_x, y), vec2(env.edit_w, nm as f32 * ROW_H));
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            ui.painter().layout_job(syntax::job(buf.as_str(), cx.lang, pal, &cx.font, ROW_H))
        };
        let edit_out = ui
            .scope_builder(UiBuilder::new().max_rect(ed_rect), |ui| {
                ui.set_clip_rect(clip(g.rt.0, g.rt.1));
                egui::TextEdit::multiline(&mut text)
                    .layouter(&mut layouter)
                    .id_salt(salt)
                    .font(cx.font.clone())
                    .frame(egui::Frame::NONE)
                    .margin(Margin::ZERO)
                    .desired_width(env.edit_w)
                    .desired_rows(nm)
                    .show(ui)
            })
            .inner;
        if edit_out.response.changed() && text != before {
            // Keep the caret inside the pane.
            if let Some(range) = edit_out.cursor_range {
                let caret = edit_out.galley.pos_from_cursor(range.primary).min.x + 2.0;
                let pane_w = g.rt.1 - g.rt.0;
                if caret - env.hscroll > pane_w - 24.0 {
                    out.hscroll = Some(caret - pane_w + 48.0);
                } else if caret - env.hscroll < 8.0 {
                    out.hscroll = Some((caret - 48.0).max(0.0));
                }
            }
            let mut t = text.clone();
            if trailing && !t.is_empty() {
                t.push('\n');
            }
            out.acts.push(match edit {
                Edit::Gap(_, range) => Act::EditGap(range, t),
                Edit::Change(i) => Act::EditChange(i, t),
                Edit::None => unreachable!(),
            });
        }
        if let Some((i, c)) = change {
            edit_out.response.context_menu(|ui| {
                if ui.add_enabled(!resolved, egui::Button::new("Ignore")).clicked() {
                    out.acts.push(Act::IgnoreAll(i));
                    ui.close();
                }
                if ui.add_enabled(env.m.can_resolve(i, None), egui::Button::new("Resolve Automatically")).clicked() {
                    out.acts.push(Act::Resolve(i));
                    ui.close();
                }
                let _ = c;
            });
        }
    }

    // Gutter buttons: ✕ ≫ on the left, ≪ ✕ on the right, ✨ in the result gutter.
    if let Some((i, c)) = change {
        let cy = y + ROW_H / 2.0;
        let tip = |base: &str| {
            if c.is_conflict() {
                format!("{base}\nCtrl+click to resolve conflict")
            } else {
                base.to_string()
            }
        };
        let accept_glyph = |left: bool| {
            if c.one_side_applied {
                "\u{f063}"
            } else if left {
                "\u{f101}"
            } else {
                "\u{f100}"
            }
        };
        if pend_l {
            if icon_btn(ui, pal, pos2(g.lg.0 + 14.0, cy), "\u{f00d}", 20.0).on_hover_text(tip("Ignore")).clicked() {
                out.acts.push(Act::Ignore(i, Side::Left, env.modifier));
            }
            if icon_btn(ui, pal, pos2(g.lg.0 + 36.0, cy), accept_glyph(true), 20.0).on_hover_text(tip("Accept")).clicked() {
                out.acts.push(Act::Accept(i, Side::Left, env.modifier));
            }
        }
        if pend_r {
            if icon_btn(ui, pal, pos2(g.rg.0 + 14.0, cy), accept_glyph(false), 20.0).on_hover_text(tip("Accept")).clicked() {
                out.acts.push(Act::Accept(i, Side::Right, env.modifier));
            }
            if icon_btn(ui, pal, pos2(g.rg.0 + 36.0, cy), "\u{f00d}", 20.0).on_hover_text(tip("Ignore")).clicked() {
                out.acts.push(Act::Ignore(i, Side::Right, env.modifier));
            }
        }
        if c.is_conflict() && !resolved && env.m.can_resolve(i, None) {
            if icon_btn(ui, pal, pos2(g.rn.0 + 14.0, cy), "\u{f0d0}", 20.0).on_hover_text("Resolve automatically").clicked() {
                out.acts.push(Act::Resolve(i));
            }
        }
    }
}
