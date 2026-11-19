//! The commit log: filter bar and a virtualized, painter-drawn table with the commit graph.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use git_core::ops::ResetMode;
use git_core::repo_state::RepoState;
use log_graph::{color_rgb, LineEnd, RowPrint};

use crate::ops::Op;
use crate::repo::RepoView;
use crate::ui::dialogs::{self, Dialog};
use crate::ui::{format_time, paint_avatar};

const ROW_H: f32 = 26.0;
const LANE_W: f32 = 14.0;
const AUTHOR_W: f32 = 180.0;
const HASH_W: f32 = 84.0;
const DATE_W: f32 = 170.0;
const MIN_COL_W: f32 = 48.0;
const MIN_SUBJECT_W: f32 = 160.0;

/// User-resizable widths of the columns right of the subject.
#[derive(Clone, Copy)]
struct ColWidths {
    author: f32,
    hash: f32,
    date: f32,
}

impl ColWidths {
    fn meta(&self) -> f32 {
        self.author + self.hash + self.date
    }
}

/// Roomy single-line input used inside the filter popover.
fn text_input<'a>(value: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(value)
        .hint_text(hint.to_string())
        .desired_width(240.0)
        .margin(egui::Margin::symmetric(10, 7))
}

/// Calendar picker for an optional `YYYY-MM-DD` filter value (empty string = unset).
fn date_field(ui: &mut egui::Ui, id: &str, value: &mut String) {
    ui.horizontal(|ui| {
        match value.trim().parse::<jiff::civil::Date>() {
            Ok(mut d) => {
                let before = d;
                ui.add(egui_extras::DatePickerButton::new(&mut d).id_salt(id).show_icon(true));
                if d != before {
                    *value = d.to_string();
                }
                if ui.small_button("\u{f00d}").on_hover_text("Clear").clicked() {
                    value.clear();
                }
            }
            Err(_) => {
                if ui.button("\u{f073}  Pick a date").clicked() {
                    *value = jiff::Zoned::now().date().to_string();
                }
            }
        }
    });
}

pub fn filter_bar(ui: &mut egui::Ui, view: &mut RepoView) {
    let mut apply = false;
    let mut load_more = false;
    let enter = |ui: &egui::Ui, r: &egui::Response| {
        r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
    };
    ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);

    let log = &mut view.log;
    let inputs = &mut log.inputs;
    let data = &log.data;
    let loading = log.loading;
    ui.horizontal_wrapped(|ui| {
        // Search box with its modifiers tucked right next to it.
        let r = ui.add(
            egui::TextEdit::singleline(&mut inputs.text)
                .hint_text("\u{f002} Text or hash")
                .desired_width(220.0),
        );
        apply |= enter(ui, &r);
        ui.spacing_mut().item_spacing.x = 2.0;
        apply |= ui.toggle_value(&mut inputs.regex, ".*").on_hover_text("Regex").changed();
        apply |= ui.toggle_value(&mut inputs.match_case, "Cc").on_hover_text("Match case").changed();
        ui.spacing_mut().item_spacing.x = 8.0;

        // Branch picker.
        let branch_text = inputs.branch.clone().unwrap_or_else(|| "All".into());
        let mut choice: Option<Option<String>> = None;
        egui::ComboBox::from_id_salt("branch_filter")
            .selected_text(format!("\u{f126} {branch_text}"))
            .width(170.0)
            .height(400.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(inputs.branch.is_none(), "All branches").clicked() {
                    choice = Some(None);
                }
                if ui.selectable_label(inputs.branch.as_deref() == Some("HEAD"), "HEAD").clicked() {
                    choice = Some(Some("HEAD".into()));
                }
                if let Some(d) = data {
                    for b in d.refs.local.iter().chain(d.refs.remote.iter()) {
                        if ui
                            .selectable_label(inputs.branch.as_deref() == Some(b.name.as_str()), &b.name)
                            .clicked()
                        {
                            choice = Some(Some(b.name.clone()));
                        }
                    }
                }
            });
        if let Some(c) = choice {
            inputs.branch = c;
            apply = true;
        }

        // Secondary filters live in a popover to keep the bar calm.
        let active = [&inputs.user, &inputs.after, &inputs.before, &inputs.paths]
            .iter()
            .filter(|v| !v.trim().is_empty())
            .count()
            + inputs.no_merges as usize;
        let label = if active > 0 { format!("\u{f0b0} Filters \u{b7} {active}") } else { "\u{f0b0} Filters".to_string() };
        let btn = ui.add(egui::Button::new(label).selected(active > 0));
        egui::Popup::menu(&btn)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.set_min_width(320.0);
                    ui.spacing_mut().item_spacing = Vec2::new(10.0, 14.0);
                    egui::Grid::new("filter_grid")
                        .num_columns(2)
                        .striped(false)
                        .spacing([16.0, 14.0])
                        .show(ui, |ui| {
                            ui.label("Author");
                            let r = ui.add(text_input(&mut inputs.user, "name, name2"));
                            apply |= enter(ui, &r);
                            ui.end_row();

                            ui.label("After");
                            date_field(ui, "filter_after", &mut inputs.after);
                            ui.end_row();

                            ui.label("Before");
                            date_field(ui, "filter_before", &mut inputs.before);
                            ui.end_row();

                            ui.label("Paths");
                            let r = ui.add(text_input(&mut inputs.paths, "src, README.md"));
                            apply |= enter(ui, &r);
                            ui.end_row();
                        });
                    ui.checkbox(&mut inputs.no_merges, "Hide merge commits")
                        .on_hover_text("Exclude merge commits (MR / PR merges)");
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if ui.button("Apply").clicked() {
                            apply = true;
                            ui.close();
                        }
                        if ui.button("Reset").clicked() {
                            let (b, t, r, c) =
                                (inputs.branch.clone(), inputs.text.clone(), inputs.regex, inputs.match_case);
                            *inputs = Default::default();
                            inputs.branch = b;
                            inputs.text = t;
                            inputs.regex = r;
                            inputs.match_case = c;
                            apply = true;
                        }
                    });
                });
            });

        if !inputs.is_empty() && ui.button("\u{f00d} Clear").clicked() {
            *inputs = Default::default();
            apply = true;
        }
        if loading {
            ui.spinner();
        }
        if let Some(d) = data {
            ui.weak(format!("{} commits", d.commits.len()));
            if d.truncated && ui.small_button("Load more").clicked() {
                load_more = true;
            }
        }
    });
    if load_more {
        view.load_more();
    }
    if apply {
        view.apply_filter();
    }
}

enum RowAction {
    Select(usize),
    Toggle(usize),
    Extend(usize),
    SelectAll,
    Op(Op),
    Dialog(Dialog),
    Copy(String),
}

pub fn table(ui: &mut egui::Ui, view: &mut RepoView) {
    if let Some(e) = &view.log.error {
        ui.colored_label(crate::theme::sys::red(ui.visuals().dark_mode), e);
    }
    let Some(data) = view.log.data.as_ref() else {
        ui.centered_and_justified(|ui| {
            ui.spinner();
        });
        return;
    };
    let n = data.commits.len();
    if n == 0 {
        ui.centered_and_justified(|ui| {
            ui.weak(if view.log.inputs.is_empty() {
                "No commits yet"
            } else {
                "No commits match the filter"
            });
        });
        return;
    }
    let dark = ui.visuals().dark_mode;
    let head_hash = data.refs.head.hash().map(str::to_string);
    let current = data.refs.head.branch_name().map(str::to_string);
    let repo_state = view.state;
    let mut actions: Vec<RowAction> = Vec::new();

    // Keyboard navigation when no text field has focus.
    let no_focus = ui.ctx().memory(|m| m.focused().is_none());
    if no_focus && view.dialog.is_none() {
        let page = view.log.visible.len().max(1);
        let sel = view.log.selected;
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
        let target = match sel {
            _ if home => Some(0),
            _ if end => Some(n - 1),
            Some(s) if down => Some((s + 1).min(n - 1)),
            Some(s) if up => Some(s.saturating_sub(1)),
            Some(s) if pgdn => Some((s + page).min(n - 1)),
            Some(s) if pgup => Some(s.saturating_sub(page)),
            None if down || up || pgdn || pgup => Some(0),
            _ => None,
        };
        if all {
            actions.push(RowAction::SelectAll);
        } else if let Some(t) = target {
            actions.push(if shift { RowAction::Extend(t) } else { RowAction::Select(t) });
            view.log.scroll_to = Some(t);
        }
    }

    // Header.
    let header_h = 20.0;
    let (hrect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), header_h), Sense::hover());
    let hp = ui.painter_at(hrect);
    let cols_id = ui.scope_id().with("log_col_widths");
    let mut cols: ColWidths = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<[f32; 3]>(cols_id))
        .map(|[author, hash, date]| ColWidths { author, hash, date })
        .unwrap_or(ColWidths { author: AUTHOR_W, hash: HASH_W, date: DATE_W });
    // Keep the subject usable when the window shrinks.
    let max_meta = (hrect.width() - MIN_SUBJECT_W).max(MIN_COL_W * 3.0);
    while cols.meta() > max_meta {
        let f = max_meta / cols.meta();
        cols.author = (cols.author * f).max(MIN_COL_W);
        cols.hash = (cols.hash * f).max(MIN_COL_W);
        cols.date = (cols.date * f).max(MIN_COL_W);
        if f >= 1.0 || cols.meta() <= MIN_COL_W * 3.0 + 0.5 {
            break;
        }
    }
    let (hash_w, date_w) = (cols.hash, cols.date);
    let meta_w = cols.meta();
    let weak = ui.visuals().weak_text_color();
    let font = FontId::proportional(13.0);
    hp.text(Pos2::new(hrect.left() + 6.0, hrect.center().y), Align2::LEFT_CENTER, "Subject", font.clone(), weak);
    hp.text(
        Pos2::new(hrect.right() - meta_w, hrect.center().y),
        Align2::LEFT_CENTER,
        "Author",
        font.clone(),
        weak,
    );
    hp.text(Pos2::new(hrect.right() - date_w - hash_w, hrect.center().y), Align2::LEFT_CENTER, "Hash", font.clone(), weak);
    hp.text(Pos2::new(hrect.right() - date_w, hrect.center().y), Align2::LEFT_CENTER, "Date", font.clone(), weak);
    hp.line_segment(
        [hrect.left_bottom(), hrect.right_bottom()],
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
    // Drag handles on the left edge of each meta column; dragging left widens the column.
    let handles = [(0usize, hrect.right() - meta_w), (1, hrect.right() - date_w - hash_w), (2, hrect.right() - date_w)];
    let mut new_cols = cols;
    for (k, x) in handles {
        let hit = Rect::from_center_size(Pos2::new(x - 4.0, hrect.center().y), Vec2::new(8.0, header_h));
        let r = ui.interact(hit, ui.scope_id().with(("log_col_handle", k)), Sense::drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let color = if r.hovered() || r.dragged() {
            ui.visuals().widgets.active.bg_stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        hp.line_segment(
            [Pos2::new(x - 4.0, hrect.top() + 4.0), Pos2::new(x - 4.0, hrect.bottom() - 4.0)],
            Stroke::new(1.0, color),
        );
        if r.dragged() {
            let w = match k {
                0 => &mut new_cols.author,
                1 => &mut new_cols.hash,
                _ => &mut new_cols.date,
            };
            *w = (*w - r.drag_delta().x).max(MIN_COL_W);
        }
    }
    if new_cols.meta() <= max_meta || new_cols.meta() < cols.meta() {
        cols = new_cols;
    }
    ui.ctx().data_mut(|d| d.insert_persisted(cols_id, [cols.author, cols.hash, cols.date]));

    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("log_scroll")
        .auto_shrink([false, false]);
    if let Some(t) = view.log.scroll_to.take() {
        let vis = &view.log.visible;
        if !(vis.start < t && t + 1 < vis.end) {
            let rows_visible = vis.len().max(1);
            let top = t.saturating_sub(rows_visible / 2);
            scroll = scroll.vertical_scroll_offset(top as f32 * ROW_H);
        }
    }
    let multi = &view.log.multi;
    let sel_rows = view.selected_rows();
    // Commits the context menu acts on when more than one is selected.
    let sel_commits: Vec<&git_core::log::Commit> = sel_rows.iter().map(|&r| &data.commits[r]).collect();
    let mut visible = 0..0;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        scroll.show_rows(ui, ROW_H, n, |ui, range| {
            visible = range.clone();
            let width = ui.available_width();
            for i in range {
                let c = &data.commits[i];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
                let painter = ui.painter_at(rect);
                let is_sel = multi.contains(&c.hash);
                let is_merge = c.parents.len() > 1;
                if is_sel {
                    // Fork-style selection: full-width rounded orange bar.
                    painter.rect_filled(rect.shrink2(Vec2::new(2.0, 1.0)), 6.0, crate::theme::sys::orange(dark));
                } else if resp.hovered() {
                    painter.rect_filled(rect.shrink2(Vec2::new(2.0, 1.0)), 6.0, ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.5));
                }
                let text_color = if is_sel {
                    Color32::WHITE
                } else if is_merge {
                    ui.visuals().weak_text_color()
                } else {
                    ui.visuals().text_color()
                };
                let weak_color = if is_sel { text_color } else { ui.visuals().weak_text_color() };

                // Graph.
                let rp = data.graph.row(i);
                let is_head = head_hash.as_deref() == Some(c.hash.as_str());
                let graph_w = paint_graph(&painter, rect, &rp, is_head, is_merge, dark);

                // Subject area with ref badges.
                let subject_rect = Rect::from_min_max(
                    Pos2::new(rect.left() + graph_w + 4.0, rect.top()),
                    Pos2::new(rect.right() - meta_w - 8.0, rect.bottom()),
                );
                let sp = ui.painter_at(subject_rect);
                let sub_w = paint_subject(&sp, Pos2::new(subject_rect.left() + 2.0, rect.center().y), &c.subject, text_color, is_sel || !is_merge);
                // Ref badges sit to the right of the subject.
                let mut x = subject_rect.left() + 2.0 + sub_w + 10.0;
                if let Some(labels) = data.labels.get(&c.hash) {
                    let accent = crate::theme::sys::orange(dark);
                    for (k, label) in labels.iter().enumerate() {
                        if k == 3 {
                            let g = sp.layout_no_wrap(format!("+{}", labels.len() - 3), FontId::proportional(11.0), weak_color);
                            sp.galley(Pos2::new(x, rect.center().y - g.size().y / 2.0), g.clone(), weak_color);
                            break;
                        }
                        // Current branch: solid accent pill; everything else: outlined pill.
                        let (bg, fg, outline) = if label.is_current && !is_sel {
                            (accent, Color32::WHITE, accent)
                        } else if is_sel {
                            (Color32::WHITE, Color32::BLACK, Color32::WHITE)
                        } else {
                            (ui.visuals().extreme_bg_color, ui.visuals().strong_text_color(), accent)
                        };
                        let g = sp.layout_no_wrap(label.name.clone(), FontId::proportional(12.5), fg);
                        let size = g.size() + Vec2::new(16.0, 4.0);
                        let r = Rect::from_min_size(Pos2::new(x, rect.center().y - size.y / 2.0), size);
                        sp.rect_filled(r, size.y / 2.0, bg);
                        sp.rect_stroke(r, size.y / 2.0, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
                        sp.galley(r.min + Vec2::new(8.0, 2.0), g, fg);
                        x += size.x + 6.0;
                    }
                }
                let ax = rect.right() - meta_w;
                let ap = ui.painter_at(Rect::from_min_max(
                    Pos2::new(ax, rect.top()),
                    Pos2::new(rect.right() - date_w - hash_w - 6.0, rect.bottom()),
                ));
                paint_avatar(&ap, Rect::from_center_size(Pos2::new(ax + 8.0, rect.center().y), Vec2::splat(16.0)), &c.author_name, dark);
                ap.text(
                    Pos2::new(ax + 22.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &c.author_name,
                    FontId::proportional(13.0),
                    text_color,
                );
                painter.text(
                    Pos2::new(rect.right() - date_w - hash_w, rect.center().y),
                    Align2::LEFT_CENTER,
                    &c.hash[..c.hash.len().min(7)],
                    FontId::monospace(12.5),
                    text_color,
                );
                painter.text(
                    Pos2::new(rect.right() - date_w, rect.center().y),
                    Align2::LEFT_CENTER,
                    format_time(c.commit_time),
                    FontId::proportional(13.0),
                    text_color,
                );

                if resp.clicked() {
                    let m = ui.input(|i| i.modifiers);
                    actions.push(if m.shift {
                        RowAction::Extend(i)
                    } else if m.command || m.ctrl {
                        RowAction::Toggle(i)
                    } else {
                        RowAction::Select(i)
                    });
                } else if resp.secondary_clicked() && !is_sel {
                    // Right-click inside the selection keeps it, so the menu acts on all of it.
                    actions.push(RowAction::Select(i));
                }
                let resp = resp.on_hover_text_at_pointer(format!(
                    "{}\n{} <{}>\n{}",
                    c.short_hash(),
                    c.author_name,
                    c.author_email,
                    crate::ui::format_full_time(c.author_time)
                ));
                resp.context_menu(|ui| {
                    if is_sel && sel_commits.len() > 1 {
                        multi_menu(ui, &sel_commits, current.as_deref(), repo_state, &mut actions);
                    } else {
                        row_menu(ui, c, current.as_deref(), repo_state, &mut actions);
                    }
                });
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
        }
    }
}

fn row_menu(ui: &mut egui::Ui, c: &git_core::log::Commit, current: Option<&str>, repo_state: RepoState, actions: &mut Vec<RowAction>) {
    ui.spacing_mut().button_padding = egui::vec2(14.0, 6.0);
    let short = c.short_hash().to_string();
    let mut act = |ui: &mut egui::Ui, label: &str, a: RowAction| {
        if ui.button(label).clicked() {
            actions.push(a);
            ui.close();
        }
    };
    act(ui, "Copy Revision Number", RowAction::Copy(c.hash.clone()));
    act(ui, "Copy Subject", RowAction::Copy(c.subject.clone()));
    ui.separator();
    act(
        ui,
        "Checkout Revision",
        RowAction::Dialog(
            dialogs::confirm(
                "Checkout Revision",
                format!("Check out {short} in detached HEAD state?"),
                Op::CheckoutDetached(c.hash.clone()),
                false,
            )
            .expect("dialog"),
        ),
    );
    act(
        ui,
        "New Branch…",
        RowAction::Dialog(Dialog::NewBranch {
            start: c.hash.clone(),
            start_label: format!("{short} {}", c.subject),
            name: String::new(),
            checkout: true,
        }),
    );
    act(
        ui,
        "New Tag…",
        RowAction::Dialog(Dialog::NewTag {
            target: c.hash.clone(),
            target_label: format!("{short} {}", c.subject),
            name: String::new(),
            message: String::new(),
        }),
    );
    ui.separator();
    act(ui, "Cherry-Pick", RowAction::Op(Op::CherryPick(vec![c.hash.clone()])));
    act(
        ui,
        "Revert Commit",
        RowAction::Dialog(
            dialogs::confirm(
                "Revert Commit",
                format!("Create a commit that reverts {short} \"{}\"?", c.subject),
                Op::Revert(vec![c.hash.clone()]),
                false,
            )
            .expect("dialog"),
        ),
    );
    let drop_block = if c.parents.len() != 1 {
        Some(format!("Selected commit has {} parents", c.parents.len()))
    } else if repo_state != RepoState::Normal {
        let op = match repo_state {
            RepoState::Rebasing => "rebase",
            RepoState::Merging => "merge",
            RepoState::CherryPicking => "cherry-pick",
            _ => "revert",
        };
        Some(format!("Cannot drop commit during {op}"))
    } else {
        None
    };
    let drop_text = match current {
        Some(cur) => format!("Do you want to drop 1 commit from the '{cur}' branch?"),
        None => "The repository is in the detached HEAD state. Do you want to drop 1 commit?".to_string(),
    };
    let mut drop_action = None;
    let mut resp = ui.add_enabled(drop_block.is_none(), egui::Button::new("Drop Commit"));
    if let Some(reason) = &drop_block {
        resp = resp.on_disabled_hover_text(reason);
    }
    if resp.clicked() {
        drop_action = Some(RowAction::Dialog(Dialog::Confirm {
            title: "Drop Commit".into(),
            text: format!("{drop_text}\n\n{short} \"{}\"\n\nThis rewrites history; later commits get new hashes.", c.subject),
            op: Op::DropCommits(vec![c.hash.clone()]),
            danger: true,
            ok: "Drop".into(),
        }));
        ui.close();
    }
    if let Some(cur) = current {
        act(
            ui,
            &format!("Reset Current Branch ('{cur}') to Here…"),
            RowAction::Dialog(Dialog::Reset {
                target: c.hash.clone(),
                label: format!("{short} {}", c.subject),
                mode: ResetMode::Mixed,
            }),
        );
        act(
            ui,
            &format!("Rebase '{cur}' onto Here"),
            RowAction::Dialog(
                dialogs::confirm(
                    "Rebase",
                    format!("Rebase '{cur}' onto {short}?"),
                    Op::Rebase { onto: c.hash.clone() },
                    false,
                )
                .expect("dialog"),
            ),
        );
        act(
            ui,
            &format!("Merge into '{cur}'…"),
            RowAction::Dialog(Dialog::Merge {
                branch: c.hash.clone(),
                no_ff: false,
                squash: false,
            }),
        );
    }
    if let Some(a) = drop_action {
        actions.push(a);
    }
}

/// Why history-rewriting ops are unavailable in the current repo state.
fn busy_reason(repo_state: RepoState, what: &str) -> Option<String> {
    let op = match repo_state {
        RepoState::Normal => return None,
        RepoState::Rebasing => "rebase",
        RepoState::Merging => "merge",
        RepoState::CherryPicking => "cherry-pick",
        _ => "revert",
    };
    Some(format!("Cannot {what} during {op}"))
}

/// Context menu for several selected commits (`commits` is in log order, newest first).
fn multi_menu(
    ui: &mut egui::Ui,
    commits: &[&git_core::log::Commit],
    current: Option<&str>,
    repo_state: RepoState,
    actions: &mut Vec<RowAction>,
) {
    ui.spacing_mut().button_padding = egui::vec2(14.0, 6.0);
    let n = commits.len();
    let list = |max: usize| {
        let mut s: String = commits
            .iter()
            .take(max)
            .map(|c| format!("{} \"{}\"", c.short_hash(), c.subject))
            .collect::<Vec<_>>()
            .join("\n");
        if n > max {
            s.push_str(&format!("\n… and {} more", n - max));
        }
        s
    };
    let mut act = |ui: &mut egui::Ui, label: &str, enabled: Option<String>, a: RowAction| {
        let mut r = ui.add_enabled(enabled.is_none(), egui::Button::new(label));
        if let Some(reason) = &enabled {
            r = r.on_disabled_hover_text(reason);
        }
        if r.clicked() {
            actions.push(a);
            ui.close();
        }
    };
    ui.weak(format!("{n} commits selected"));
    ui.separator();
    act(
        ui,
        "Copy Revision Numbers",
        None,
        RowAction::Copy(commits.iter().map(|c| c.hash.as_str()).collect::<Vec<_>>().join("\n")),
    );
    act(
        ui,
        "Copy Subjects",
        None,
        RowAction::Copy(commits.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>().join("\n")),
    );
    ui.separator();
    let busy = busy_reason(repo_state, "start another operation");
    // Cherry-pick applies oldest first; revert undoes newest first.
    let oldest_first: Vec<String> = commits.iter().rev().map(|c| c.hash.clone()).collect();
    let newest_first: Vec<String> = commits.iter().map(|c| c.hash.clone()).collect();
    act(
        ui,
        &format!("Cherry-Pick {n} Commits"),
        busy.clone(),
        RowAction::Dialog(
            dialogs::confirm(
                "Cherry-Pick",
                format!("Cherry-pick {n} commits onto the current HEAD (oldest first)?\n\n{}", list(10)),
                Op::CherryPick(oldest_first),
                false,
            )
            .expect("dialog"),
        ),
    );
    act(
        ui,
        &format!("Revert {n} Commits"),
        busy,
        RowAction::Dialog(
            dialogs::confirm(
                "Revert Commits",
                format!("Create {n} commits that revert these (newest first)?\n\n{}", list(10)),
                Op::Revert(newest_first.clone()),
                false,
            )
            .expect("dialog"),
        ),
    );
    let drop_block = commits
        .iter()
        .find(|c| c.parents.len() != 1)
        .map(|c| format!("{} has {} parents; only commits with one parent can be dropped", c.short_hash(), c.parents.len()))
        .or_else(|| busy_reason(repo_state, "drop commits"));
    let drop_text = match current {
        Some(cur) => format!("Do you want to drop {n} commits from the '{cur}' branch?"),
        None => format!("The repository is in the detached HEAD state. Do you want to drop {n} commits?"),
    };
    act(
        ui,
        &format!("Drop {n} Commits"),
        drop_block,
        RowAction::Dialog(Dialog::Confirm {
            title: "Drop Commits".into(),
            text: format!("{drop_text}\n\n{}\n\nThis rewrites history; later commits get new hashes.", list(10)),
            op: Op::DropCommits(newest_first),
            danger: true,
            ok: format!("Drop {n}"),
        }),
    );
}

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
fn paint_graph(painter: &egui::Painter, rect: Rect, rp: &RowPrint, is_head: bool, is_merge: bool, dark: bool) -> f32 {
    let color = |id: i32| {
        let [r, g, b] = color_rgb(id);
        let c = Color32::from_rgb(r, g, b);
        if dark {
            c.gamma_multiply(1.15)
        } else {
            c
        }
    };
    let mid = rect.center().y;
    let mut max_lane = rp.lanes.max(1) - 1;
    for l in &rp.lines {
        max_lane = max_lane.max(l.from_lane).max(l.to_lane);
        let x0 = lane_x(rect, l.from_lane);
        let stroke = Stroke::new(1.6, color(l.color));
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
    if is_merge {
        // Merge commit: hollow ring with a small chevron, like Fork.
        let bg = if dark { Color32::from_gray(30) } else { Color32::WHITE };
        painter.circle_filled(center, 6.5, bg);
        painter.circle_stroke(center, 6.0, Stroke::new(1.6, nc));
        let s = Stroke::new(1.4, nc);
        painter.line_segment([center + Vec2::new(-2.5, -0.8), center + Vec2::new(0.0, 1.8)], s);
        painter.line_segment([center + Vec2::new(0.0, 1.8), center + Vec2::new(2.5, -0.8)], s);
        if is_head {
            painter.circle_stroke(center, 8.0, Stroke::new(1.2, nc));
        }
    } else if is_head {
        painter.circle_filled(center, 4.5, nc);
        painter.circle_stroke(center, 4.5, Stroke::new(1.5, if dark { Color32::WHITE } else { Color32::BLACK }));
    } else {
        painter.circle_filled(center, 3.8, nc);
    }
    8.0 + LANE_W * (max_lane as f32 + 1.0)
}
