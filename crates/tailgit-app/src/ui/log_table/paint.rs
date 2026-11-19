//! Painter helpers: commit graph, subject text and ref pills.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use log_graph::{color_rgb, LineEnd, RowPrint};

use super::{LABEL_LINE_H, LANE_W, ROW_H};
use crate::settings::{LabelPosition, LabelStyle};

/// Subject text; a conventional-commit prefix (`fix:`, `feat(x):`) is drawn bold.
fn paint_subject(p: &egui::Painter, pos: Pos2, subject: &str, color: Color32, bold_prefix: bool) -> f32 {
    let prefix_len = subject
        .find(": ")
        .filter(|&i| i <= 24 && subject[..i].chars().all(|c| c.is_alphanumeric() || "()-_!/".contains(c)))
        .map(|i| i + 1);
    let font = FontId::proportional(13.5);
    let mut job = egui::text::LayoutJob::default();
    match prefix_len {
        Some(n) if bold_prefix => {
            // Faux bold: the system font is loaded as a single weight.
            job.append(&subject[..n], 0.0, egui::TextFormat::simple(font.clone(), color));
            job.append(&subject[n..], 0.0, egui::TextFormat::simple(font, color));
            let g = p.layout_job(job);
            let w = g.size().x + 0.5;
            let top = Pos2::new(pos.x, pos.y - g.size().y / 2.0);
            // Overdraw the prefix with a 0.5px offset to embolden it.
            let prefix = p.layout_no_wrap(subject[..n].to_string(), FontId::proportional(13.5), color);
            p.galley(top, g, color);
            p.galley(top + Vec2::new(0.5, 0.0), prefix, color);
            w
        }
        _ => p.text(pos, Align2::LEFT_CENTER, subject, font, color).width(),
    }
}

fn lane_x(rect: Rect, lane: usize) -> f32 {
    rect.left() + 4.0 + LANE_W * (lane as f32 + 0.5)
}

/// Paints one graph row; returns the width used.
pub(super) fn paint_graph(painter: &egui::Painter, rect: Rect, mid: f32, rp: &RowPrint, is_head: bool, is_merge: bool, dark: bool) -> f32 {
    let color = |id: i32| {
        let [r, g, b] = color_rgb(id);
        let c = Color32::from_rgb(r, g, b);
        if dark {
            c.gamma_multiply(1.15)
        } else {
            c
        }
    };
    let mut max_lane = rp.lanes.max(1) - 1;
    for l in &rp.lines {
        max_lane = max_lane.max(l.from_lane).max(l.to_lane);
        let x0 = lane_x(rect, l.from_lane);
        let stroke = Stroke::new(1.5, color(l.color));
        let edge_y = if l.up { rect.top() } else { rect.bottom() };
        match l.end {
            LineEnd::Normal => {
                let x1 = (x0 + lane_x(rect, l.to_lane)) / 2.0;
                painter.line_segment([Pos2::new(x0, mid), Pos2::new(x1, edge_y)], stroke);
            }
            LineEnd::Arrow => {
                // Long edge continues off-row: arrow pointing along the edge.
                let tip_y = if l.up { rect.top() + 3.0 } else { rect.bottom() - 3.0 };
                painter.line_segment([Pos2::new(x0, mid), Pos2::new(x0, tip_y)], stroke);
                let dy = if l.up { 4.0 } else { -4.0 };
                painter.line_segment([Pos2::new(x0, tip_y), Pos2::new(x0 - 3.0, tip_y + dy)], stroke);
                painter.line_segment([Pos2::new(x0, tip_y), Pos2::new(x0 + 3.0, tip_y + dy)], stroke);
            }
            LineEnd::Dangling => {
                let y = mid + (rect.bottom() - mid) * 0.6;
                painter.line_segment([Pos2::new(x0, mid), Pos2::new(x0, y)], Stroke::new(1.2, color(l.color).gamma_multiply(0.6)));
            }
        }
    }
    let center = Pos2::new(lane_x(rect, rp.node_lane), mid);
    let nc = color(rp.node_color);
    let outline = Color32::from_rgb(
        (nc.r() as f32 * 0.45) as u8,
        (nc.g() as f32 * 0.45) as u8,
        (nc.b() as f32 * 0.45) as u8,
    );
    if is_merge {
        // Merge commit: slightly larger node with a chevron.
        painter.circle_filled(center, 6.0, nc);
        painter.circle_stroke(center, 6.0, Stroke::new(1.5, outline));
        let s = Stroke::new(1.4, Color32::WHITE);
        painter.line_segment([center + Vec2::new(-2.5, -0.8), center + Vec2::new(0.0, 1.8)], s);
        painter.line_segment([center + Vec2::new(0.0, 1.8), center + Vec2::new(2.5, -0.8)], s);
    } else {
        painter.circle_filled(center, 5.0, nc);
        painter.circle_stroke(center, 5.0, Stroke::new(1.5, outline));
    }
    if is_head {
        painter.circle_stroke(center, 8.0, Stroke::new(1.2, nc));
    }
    8.0 + LANE_W * (max_lane as f32 + 1.0)
}

/// Draws up to four pills starting at `x`, vertically centred on `cy`, then "+n"; returns the end x.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_pills(
    p: &egui::Painter,
    mut x: f32,
    cy: f32,
    labels: &[git_core::refs::RefLabel],
    style: LabelStyle,
    dark: bool,
    text: Color32,
    weak: Color32,
) -> f32 {
    for (k, label) in labels.iter().enumerate() {
        if k == 4 {
            let g = p.layout_no_wrap(format!("+{}", labels.len() - 4), FontId::proportional(11.5), weak);
            let w = g.size().x;
            p.galley(Pos2::new(x, cy - g.size().y / 2.0), g, weak);
            return x + w;
        }
        x += paint_pill(p, Pos2::new(x, cy), label, dark, style, text) + 6.0;
    }
    x
}

/// Lays out the subject and its ref pills for the chosen placement (`Column` and `Hidden` draw no pills here).
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_subject_and_labels(
    p: &egui::Painter,
    x: f32,
    row_top: f32,
    cy: f32,
    subject: &str,
    text: Color32,
    bold: bool,
    labels: &[git_core::refs::RefLabel],
    pos: LabelPosition,
    style: LabelStyle,
    dark: bool,
) {
    let weak = p.ctx().global_style().visuals.weak_text_color();
    let mut sx = x;
    let mut pills: Option<(f32, f32)> = None; // (x, cy)
    match pos {
        LabelPosition::Top => pills = Some((x, row_top + LABEL_LINE_H / 2.0 + 2.0)),
        LabelPosition::Bottom => pills = Some((x, row_top + ROW_H - 3.0 + LABEL_LINE_H / 2.0)),
        LabelPosition::Left if !labels.is_empty() => {
            sx = paint_pills(p, x, cy, labels, style, dark, text, weak) + 4.0;
        }
        _ => {}
    }
    let w = paint_subject(p, Pos2::new(sx, cy), subject, text, bold);
    match (pos, pills) {
        (LabelPosition::Right, _) if !labels.is_empty() => {
            paint_pills(p, sx + w + 10.0, cy, labels, style, dark, text, weak);
        }
        (_, Some((px, py))) if !labels.is_empty() => {
            paint_pills(p, px, py, labels, style, dark, text, weak);
        }
        _ => {}
    }
}

/// A ref pill centred vertically on `pos` (left edge at `pos.x`); returns its width.
fn paint_pill(p: &egui::Painter, pos: Pos2, label: &git_core::refs::RefLabel, dark: bool, style: LabelStyle, text_color: Color32) -> f32 {
    use crate::theme::sys;
    use git_core::refs::RefKind;
    let palette = [
        sys::blue(dark),
        sys::green(dark),
        sys::purple(dark),
        sys::teal(dark),
        sys::indigo(dark),
        sys::red(dark),
        sys::orange(dark),
    ];
    // Local `feat/x` and remote `origin/feat/x` share a colour.
    let key = match label.kind {
        RefKind::RemoteBranch => label.name.split_once('/').map_or(label.name.as_str(), |(_, rest)| rest),
        _ => label.name.as_str(),
    };
    let h = key.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let accent = sys::orange(dark);
    let base = match style {
        LabelStyle::Accent => accent,
        _ if label.kind == RefKind::Tag => sys::yellow(dark),
        _ => palette[(h % palette.len() as u64) as usize],
    };
    let remote = label.kind == RefKind::RemoteBranch;
    let text = if label.is_current {
        format!("\u{f00c} {}", label.name)
    } else if label.kind == RefKind::Tag {
        format!("\u{f02b} {}", label.name)
    } else {
        label.name.clone()
    };
    let (bg, fg, stroke) = match style {
        LabelStyle::Border => {
            let c = if label.is_current { accent } else { base };
            (Color32::TRANSPARENT, text_color, c)
        }
        _ if label.is_current => (accent, Color32::WHITE, accent),
        _ if remote => (base.gamma_multiply(0.14), base, base.gamma_multiply(0.8)),
        _ if label.kind == RefKind::Tag && style == LabelStyle::Colorful => (base, Color32::BLACK, base),
        _ => (base, Color32::WHITE, base),
    };
    let g = p.layout_no_wrap(text, FontId::proportional(12.0), fg);
    let size = g.size() + Vec2::new(14.0, 3.0);
    let r = Rect::from_min_size(Pos2::new(pos.x, pos.y - size.y / 2.0), size);
    p.rect_filled(r, 5.0, bg);
    p.rect_stroke(r, 5.0, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    p.galley(r.min + Vec2::new(7.0, 1.5), g, fg);
    size.x
}
