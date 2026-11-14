//! Top-level window: repository picker, toolbar, tabs and panels.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use git_core::ops::PullMode;
use git_core::repo_state::RepoState;
use git_core::Git;

use crate::ops::Op;
use crate::repo::{RepoView, Tab};
use crate::settings::Settings;
use crate::ui::{self, dialogs::Dialog, PaneLayout};

pub struct TailApp {
    settings: Settings,
    repo: Option<RepoView>,
    path_input: String,
    error: Option<String>,
    was_focused: bool,
    show_sidebar: bool,
    show_details: bool,
    layout: PaneLayout,
    show_settings: bool,
    applied_font: String,
    applied_size: f32,
    applied_weight: f32,
}


impl TailApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> TailApp {
        let settings = Settings::load();
        let settings_font = settings.ui_font.clone();
        let settings_size = settings.ui_font_size;
        let settings_weight = settings.ui_font_weight;
        let mut app = TailApp {
            path_input: String::new(),
            settings,
            repo: None,
            error: None,
            was_focused: true,
            show_sidebar: true,
            show_details: true,
            layout: PaneLayout::All,
            show_settings: false,
            applied_font: settings_font,
            applied_size: settings_size,
            applied_weight: settings_weight,
        };
        crate::ui::settings_window::publish(&cc.egui_ctx, &app.settings);
        let start = initial.or_else(|| app.settings.recent.first().map(PathBuf::from));
        if let Some(p) = start {
            app.open(p, &cc.egui_ctx);
        }
        app
    }

    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        match Git::open(&path) {
            Ok(git) => {
                let root = git.root().display().to_string();
                self.settings.add_recent(&root);
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("TailGIT — {root}")));
                self.repo = Some(RepoView::new(git, ctx));
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        use egui::{Align2, CornerRadius, FontId, Frame, Margin, Rect, Sense, Stroke, UiBuilder, Vec2};
        let mut open: Option<PathBuf> = None;
        let dark = ui.visuals().dark_mode;
        let accent = crate::theme::sys::blue(dark);
        egui::CentralPanel::default().show(ui, |ui| {
            // Whole background drags the (title-bar-less) window.
            let bg = ui.interact(ui.max_rect(), egui::Id::unique("welcome_drag"), Sense::drag());
            if bg.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            let size = Vec2::new(760.0, 440.0).min(ui.available_size() - Vec2::splat(32.0));
            let card = Rect::from_center_size(ui.max_rect().center(), size);
            let v = ui.visuals().clone();
            ui.scope_builder(UiBuilder::new().max_rect(card), |ui| {
                Frame::new()
                    .fill(v.window_fill)
                    .stroke(v.window_stroke)
                    .corner_radius(CornerRadius::same(12))
                    .shadow(v.window_shadow)
                    .show(ui, |ui| {
                        ui.set_min_size(card.size());
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            // ── Left: identity + primary actions ──
                            let left_w = 340.0;
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, card.height()),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.set_width(left_w);
                                    ui.add_space(44.0);
                                    // App icon.
                                    let tex = crate::brand::texture(ui.ctx());
                                    let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
                                    ui.painter().image(
                                        tex.id(),
                                        r,
                                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                        egui::Color32::WHITE,
                                    );
                                    ui.add_space(18.0);
                                    ui.label(RichText::new("TailGIT").size(26.0).strong());
                                    ui.add_space(2.0);
                                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).small().weak());
                                    ui.add_space(32.0);

                                    let primary = egui::Button::new(RichText::new("Open Repository…").size(13.0).color(egui::Color32::WHITE))
                                        .min_size(Vec2::new(240.0, 28.0))
                                        .fill(accent)
                                        .stroke(Stroke::NONE);
                                    if ui.add(primary)
                                        .clicked()
                                    {
                                        open = rfd::FileDialog::new().pick_folder();
                                    }
                                    ui.add_space(10.0);
                                    let field = ui.add(
                                        egui::TextEdit::singleline(&mut self.path_input)
                                            .hint_text("Or enter a path and press Return")
                                            .desired_width(240.0)
                                            .margin(Margin::symmetric(8, 5)),
                                    );
                                    if field.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                        && !self.path_input.trim().is_empty()
                                    {
                                        open = Some(PathBuf::from(self.path_input.trim()));
                                    }
                                    if let Some(e) = &self.error {
                                        ui.add_space(8.0);
                                        ui.add_sized(
                                            [240.0, 0.0],
                                            egui::Label::new(RichText::new(e).small().color(crate::theme::sys::red(dark))).wrap(),
                                        );
                                    }
                                },
                            );

                            // ── Right: recent repositories (sidebar material) ──
                            let right = Rect::from_min_max(
                                egui::pos2(card.left() + left_w, card.top()),
                                card.right_bottom(),
                            );
                            let side = if dark { egui::Color32::from_white_alpha(6) } else { egui::Color32::from_black_alpha(10) };
                            ui.painter().rect_filled(
                                right,
                                CornerRadius { nw: 0, sw: 0, ne: 12, se: 12 },
                                side,
                            );
                            ui.painter().vline(right.left(), right.y_range(), Stroke::new(1.0, v.window_stroke.color));
                            ui.scope_builder(UiBuilder::new().max_rect(right.shrink2(Vec2::new(12.0, 16.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                                ui.label(RichText::new("  Recent Repositories").small().strong().weak());
                                ui.add_space(6.0);
                                if self.settings.recent.is_empty() {
                                    ui.add_space(120.0);
                                    ui.vertical_centered(|ui| ui.weak("No recent repositories"));
                                    return;
                                }
                                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                    for r in self.settings.recent.clone() {
                                        let path = std::path::Path::new(&r);
                                        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(r.clone());
                                        let home = std::env::var("HOME").unwrap_or_default();
                                        let shown = if !home.is_empty() && r.starts_with(&home) { format!("~{}", &r[home.len()..]) } else { r.clone() };
                                        let w = ui.available_width();
                                        let (row, resp) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::click());
                                        let p = ui.painter();
                                        if resp.is_pointer_button_down_on() {
                                            p.rect_filled(row, 8.0, accent);
                                        } else if resp.hovered() {
                                            p.rect_filled(row, 8.0, ui.visuals().widgets.hovered.bg_fill);
                                        }
                                        let pressed = resp.is_pointer_button_down_on();
                                        let icon = row.left_center() + Vec2::new(20.0, 0.0);
                                        p.text(icon, Align2::CENTER_CENTER, "\u{f07b}", FontId::proportional(18.0), if pressed { egui::Color32::WHITE } else { accent });
                                        let tx = row.left() + 40.0;
                                        let fg = if pressed { egui::Color32::WHITE } else { ui.visuals().strong_text_color() };
                                        let weak = if pressed { egui::Color32::from_white_alpha(200) } else { ui.visuals().weak_text_color() };
                                        let max_w = (right.right() - 22.0).min(row.right()) - tx;
                                        let line = |text: &str, size: f32, color: egui::Color32| {
                                            let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(size), color);
                                            job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
                                            ui.fonts_mut(|f| f.layout_job(job))
                                        };
                                        let g1 = line(&name, 13.0, fg);
                                        let g2 = line(&shown, 11.0, weak);
                                        p.galley(egui::pos2(tx, row.top() + 13.0 - g1.size().y / 2.0), g1, fg);
                                        p.galley(egui::pos2(tx, row.top() + 30.0 - g2.size().y / 2.0), g2, weak);
                                        if resp.on_hover_text(&r).clicked() {
                                            open = Some(PathBuf::from(r));
                                        }
                                    }
                                });
                            });
                        });
                    });
            });
        });
        if let Some(p) = open {
            self.open(p, ui.ctx());
        }
    }
}

impl eframe::App for TailApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut open_request: Option<PathBuf> = None;

        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.show_settings = true;
        }
        if self.show_settings {
            let prev_appearance = self.settings.appearance;
            if ui::settings_window::show(&ctx, &mut self.show_settings, &mut self.settings) {
                ui::settings_window::publish(&ctx, &self.settings);
                if self.settings.appearance != prev_appearance {
                    crate::theme::apply_appearance(&ctx, self.settings.appearance);
                }
                crate::theme::apply_text_options(&ctx, self.settings.text_hinting, self.settings.text_subpixel);
                if self.settings.ui_font != self.applied_font || self.settings.ui_font_weight != self.applied_weight {
                    self.applied_font = self.settings.ui_font.clone();
                    self.applied_weight = self.settings.ui_font_weight;
                    crate::theme::install_fonts(&ctx, &self.applied_font, self.applied_weight);
                }
                if self.settings.ui_font_size != self.applied_size {
                    self.applied_size = self.settings.ui_font_size;
                    crate::theme::apply_font_size(&ctx, self.applied_size);
                }
            }
        }
        if self.repo.is_none() {
            self.welcome(ui);
            return;
        }
        let lay = self.layout;
        let settings = &mut self.settings;
        let view = self.repo.as_mut().expect("repo");
        if view.topo_order != settings.sort_topo {
            view.topo_order = settings.sort_topo;
            view.reload_log();
        }
        view.poll();

        let focused = ui.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.was_focused {
            view.jobs.load_status();
        }
        self.was_focused = focused;

        // Toolbar.
        // Unified title bar + toolbar (macOS): leaves room for the window controls
        // and lets empty space drag the window.
        let dark = ui.visuals().dark_mode;
        let isl = |o| crate::theme::island(dark, o);
        ui.painter().rect_filled(ui.ctx().content_rect(), 0.0, crate::theme::gap_color(dark));
        // Full-height sidebar (Fork / Finder layout): sits under the window
        // controls; the toolbar spans only the content area to its right.
        let mut show_sidebar = self.show_sidebar;
        let mut show_details = self.show_details;
        // Sidebar toggle, pinned next to the window controls in both states.
        egui::Area::new(egui::Id::NULL.with("sidebar_toggle"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(82.0, 9.0))
            .show(&ctx, |ui| {
                let tip = if show_sidebar { "Hide sidebar" } else { "Show sidebar" };
                if ui::pane_toggle(ui, false, show_sidebar, tip).clicked() {
                    show_sidebar = !show_sidebar;
                }
            });
        let full_w = ui.ctx().content_rect().width();
        if (show_sidebar && lay == PaneLayout::All) || lay == PaneLayout::Left {
            let panel = egui::Panel::left("branches");
            let panel = if lay == PaneLayout::Left { panel.exact_size(full_w - 16.0).resizable(false) } else { panel.resizable(true) };
            panel
                .frame(
                    egui::Frame::new()
                        .fill(crate::theme::gap_color(dark))
                        .inner_margin(egui::Margin { left: 10, right: 6, top: 44, bottom: 8 }),
                )
                .show_separator_line(false)
                .default_size(260.0)
                .show(ui, |ui| {
                    // Title-bar strip above the sidebar drags the window.
                    let strip = egui::Rect::from_min_size(ui.max_rect().min - egui::vec2(10.0, 44.0), egui::vec2(ui.max_rect().width() + 16.0, 40.0));
                    let drag = ui.interact(strip, egui::Id::unique("sidebar_drag"), egui::Sense::drag());
                    if drag.drag_started() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    sidebar(ui, view, settings)
                });
        }
        egui::Panel::top("toolbar").frame(crate::theme::chrome(dark)).show_separator_line(false).show(ui, |ui| {
            let bar = ui.max_rect();
            let drag = ui.interact(bar, egui::Id::unique("titlebar_drag"), egui::Sense::click_and_drag());
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            ui.add_space(6.0);
            let busy = view.busy.is_some();
            let (ahead, behind) = view
                .log
                .data
                .as_ref()
                .and_then(|d| d.refs.current_branch())
                .map(|b| (b.ahead, b.behind))
                .unwrap_or((0, 0));
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 38.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(if show_sidebar { 4.0 } else { 116.0 });
                ui.spacing_mut().item_spacing.x = 8.0;
                let dark = ui.visuals().dark_mode;

                if btn_group(ui, dark, |ui| [bar_button(ui, "\u{f07c}", "Open Folder", false, true)]).0.clicked() {
                    open_request = rfd::FileDialog::new().pick_folder();
                }
                if btn_group(ui, dark, |ui| [bar_button(ui, "\u{f381}", "Fetch", false, !busy)]).0.clicked() {
                    view.run(Op::Fetch);
                }
                let (pull_r, push_r, stash_r) = btn_group_3(ui, dark, |ui| {
                    let pull_label = if behind > 0 { format!("Pull {behind}") } else { "Pull".to_string() };
                    let push_label = if ahead > 0 { format!("Push {ahead}") } else { "Push".to_string() };
                    (
                        bar_button(ui, "\u{f0ab}", &pull_label, false, !busy),
                        bar_button(ui, "\u{f0aa}", &push_label, false, !busy),
                        bar_button(ui, "\u{f187}", "Stash", false, !busy),
                    )
                });
                egui::Popup::menu(&pull_r).show(|ui| {
                    for (label, mode) in [
                        ("Pull (merge)", PullMode::Merge),
                        ("Pull (rebase)", PullMode::Rebase),
                        ("Pull (fast-forward only)", PullMode::FastForwardOnly),
                    ] {
                        if ui.button(label).clicked() {
                            view.run(Op::Pull(mode));
                            ui.close();
                        }
                    }
                });
                if push_r.clicked() {
                    match ui::dialogs::push_dialog(view) {
                        Some(d) => view.dialog = Some(d),
                        None => view.notify(false, "Nothing to push: no current branch or no remotes".into()),
                    }
                }
                if stash_r.clicked() {
                    view.dialog = Some(Dialog::Stash {
                        message: String::new(),
                        untracked: false,
                        keep_index: false,
                    });
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    let tip = if show_details { "Hide details pane" } else { "Show details pane" };
                    if ui::pane_toggle(ui, true, show_details, tip).clicked() {
                        show_details = !show_details;
                    }
                    if ui.add(egui::Button::new("\u{f013}").frame(false)).on_hover_text("Settings (\u{2318},)").clicked() {
                        self.show_settings = true;
                    }
                    if let Some(b) = &view.busy {
                        ui.spinner().on_hover_text(format!("{b}\n{}", view.progress));
                    }
                    // Right group: views + new branch.
                    let changes = view.change_count();
                    let stashes = view.stashes.len();
                    let before = view.tab;
                    let cur = view.tab;
                    let (lc, ac, st, br) = btn_group_4(ui, dark, |ui| {
                        let lc_label = if changes > 0 { format!("Local Changes {changes}") } else { "Local Changes".to_string() };
                        let st_label = if stashes > 0 { format!("Stashes {stashes}") } else { "Stashes".to_string() };
                        (
                            bar_button(ui, "\u{f15c}", &lc_label, cur == Tab::Commit, true),
                            bar_button(ui, "\u{f1da}", "All Commits", cur == Tab::Log, true),
                            bar_button(ui, "\u{f187}", &st_label, cur == Tab::Stash, true),
                            bar_button(ui, "\u{f067}", "Branch", false, !busy),
                        )
                    });
                    if lc.clicked() {
                        view.tab = Tab::Commit;
                    }
                    if ac.clicked() {
                        view.tab = Tab::Log;
                    }
                    if st.clicked() {
                        view.tab = Tab::Stash;
                    }
                    if before != view.tab {
                        view.diff = None;
                        view.commit_selection = None;
                        if view.tab == Tab::Commit {
                            view.jobs.load_status();
                        }
                    }
                    if br.clicked() {
                        let head = view.log.data.as_ref().and_then(|d| d.refs.head.branch_name().map(str::to_string)).unwrap_or_else(|| "HEAD".into());
                        view.dialog = Some(Dialog::NewBranch {
                            start: "HEAD".into(),
                            start_label: head,
                            name: String::new(),
                            checkout: true,
                        });
                    }
                });
            });
            ui.add_space(7.0);
        });

        // Repository state banner (merge / rebase / cherry-pick in progress).
        let conflicts = view.conflict_count();
        if view.state != RepoState::Normal || conflicts > 0 {
            egui::Panel::top("state_banner").frame(isl((6, 6, 0, 6))).show_separator_line(false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let text = if conflicts > 0 {
                        format!("\u{f071} {} — {conflicts} conflicted file(s)", view.state.label())
                    } else {
                        format!("\u{f071} {} in progress — conflicts resolved", view.state.label())
                    };
                    ui.label(RichText::new(text).color(crate::theme::sys::orange(ui.visuals().dark_mode)).strong());
                    if conflicts > 0 && ui.button("Resolve…").clicked() {
                        view.open_conflicts();
                    }
                    let mut op = None;
                    if conflicts == 0 {
                        ui::conflicts::continue_buttons(ui, view.state, view.busy.is_some(), &mut op);
                    } else {
                        let busy = view.busy.is_some();
                        let abort = match view.state {
                            RepoState::Merging => Some(("Abort Merge", Op::MergeAbort)),
                            RepoState::Rebasing => Some(("Abort Rebase", Op::RebaseAbort)),
                            RepoState::CherryPicking => Some(("Abort Cherry-Pick", Op::CherryPickAbort)),
                            RepoState::Reverting => Some(("Abort Revert", Op::RevertAbort)),
                            RepoState::Normal => None,
                        };
                        if let Some((label, o)) = abort {
                            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                                op = Some(o);
                            }
                        }
                    }
                    if let Some(o) = op {
                        view.run(o);
                    }
                });
            });
        }

        // Status bar and operation output.
        egui::Panel::bottom("status_bar").frame(crate::theme::chrome(dark)).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.toggle_value(&mut view.show_output, "Output");
                if let Some((ok, text, _)) = &view.notice {
                    let color = if *ok { crate::theme::sys::green(ui.visuals().dark_mode) } else { crate::theme::sys::red(ui.visuals().dark_mode) };
                    ui.label(RichText::new(text).color(color));
                }
                if let Some(d) = view.last_drop.clone() {
                    if ui.add_enabled(view.busy.is_none(), egui::Link::new("Undo")).clicked() {
                        view.run(Op::UndoDrop { old_head: d.old_head, new_head: d.new_head });
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(&view.root);
                });
            });
        });
        if view.show_output {
            egui::Panel::bottom("output")
                .frame(isl((6, 6, 3, 3)))
                .show_separator_line(false)
                .resizable(true)
                .default_size(180.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("output_scroll")
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for e in &view.output {
                                let color = if e.ok { crate::theme::sys::green(ui.visuals().dark_mode) } else { crate::theme::sys::red(ui.visuals().dark_mode) };
                                ui.label(
                                    RichText::new(format!("{}  {}", e.time.format("%H:%M:%S"), e.title))
                                        .color(color)
                                        .strong(),
                                );
                                if !e.text.is_empty() {
                                    ui.label(RichText::new(&e.text).monospace().size(12.0));
                                }
                            }
                        });
                });
        }


        match view.tab {
            _ if lay == PaneLayout::Left => {}
            Tab::Log if lay == PaneLayout::Right => {
                egui::CentralPanel::default().frame(isl((6, 6, 0, 3))).show(ui, |ui| ui::details::show(ui, view));
            }
            Tab::Log => {
                if lay == PaneLayout::All && show_details {
                    egui::Panel::right("details")
                        .frame(isl((3, 6, 0, 3)))
                        .show_separator_line(false)
                        .resizable(true)
                        .default_size(380.0)
                        .show(ui, |ui| ui::details::show(ui, view));
                }
                if view.diff.is_some() {
                    let mut keep = true;
                    egui::Panel::bottom("log_diff")
                        .frame(isl((3, 3, 3, 3)))
                        .show_separator_line(false)
                        .resizable(true)
                        .default_size(320.0)
                        .show(ui, |ui| {
                            let mut sbs = settings.side_by_side;
                            let out = ui::diff_view::show(ui, view.diff.as_mut().expect("diff"), &mut sbs, true);
                            keep = out.keep;
                            if sbs != settings.side_by_side {
                                settings.side_by_side = sbs;
                                settings.save();
                            }
                        });
                    if !keep {
                        view.diff = None;
                    }
                }
                egui::CentralPanel::default().frame(isl((3, 3, 0, 3))).show(ui, |ui| {
                    ui::log_table::filter_bar(ui, view);
                    ui.add_space(4.0);
                    ui::log_table::table(ui, view);
                });
            }
            Tab::Commit | Tab::Stash => {
                let list = |ui: &mut egui::Ui, view: &mut RepoView| {
                    if view.tab == Tab::Commit {
                        ui::commit_panel::show(ui, view)
                    } else {
                        ui::stash_panel::show(ui, view)
                    }
                };
                if lay == PaneLayout::Center {
                    egui::CentralPanel::default().frame(isl((6, 6, 0, 3))).show(ui, |ui| list(ui, view));
                } else if lay == PaneLayout::All {
                    egui::Panel::left("tab_left")
                        .frame(isl((3, 3, 0, 3)))
                        .show_separator_line(false)
                        .resizable(true)
                        .default_size(440.0)
                        .show(ui, |ui| list(ui, view));
                }
                let mut action = None;
                if lay != PaneLayout::Center {
                egui::CentralPanel::default().frame(isl((if lay == PaneLayout::Right { 6 } else { 3 }, 6, 0, 3))).show(ui, |ui| match &mut view.diff {
                    Some(d) => {
                        let mut sbs = settings.side_by_side;
                        action = ui::diff_view::show(ui, d, &mut sbs, false).action;
                        if sbs != settings.side_by_side {
                            settings.side_by_side = sbs;
                            settings.save();
                        }
                    }
                    None => {
                        ui.centered_and_justified(|ui| ui.weak("Select a file to see its changes"));
                    }
                });
                }
                match action {
                    Some(ui::diff_view::DiffAction::Run(op)) => view.run(op),
                    Some(ui::diff_view::DiffAction::Confirm(d)) => view.dialog = Some(d),
                    None => {}
                }
            }
        }

        self.show_sidebar = show_sidebar;
        self.show_details = show_details;
        self.layout = lay;
        view.auto_apply_non_conflicting = settings.merge_auto_apply;
        ui::conflicts::show(&ctx, view, settings);
        ui::dialogs::show(&ctx, view);

        if view.needs_repaint_soon() || view.busy.is_some() || view.log.loading {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        if let Some(p) = open_request {
            self.open(p, &ctx);
        }
    }
}

/// Rounded dark container grouping toolbar buttons (Fork-style).
fn group_frame<R>(ui: &mut egui::Ui, _dark: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(3, 3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), add).inner
        })
        .inner
}

fn btn_group(ui: &mut egui::Ui, dark: bool, add: impl FnOnce(&mut egui::Ui) -> [egui::Response; 1]) -> (egui::Response,) {
    let [a] = group_frame(ui, dark, add);
    (a,)
}

fn btn_group_3(ui: &mut egui::Ui, dark: bool, add: impl FnOnce(&mut egui::Ui) -> (egui::Response, egui::Response, egui::Response)) -> (egui::Response, egui::Response, egui::Response) {
    group_frame(ui, dark, add)
}

fn btn_group_4(
    ui: &mut egui::Ui,
    dark: bool,
    add: impl FnOnce(&mut egui::Ui) -> (egui::Response, egui::Response, egui::Response, egui::Response),
) -> (egui::Response, egui::Response, egui::Response, egui::Response) {
    // Right-to-left parent: lay the group out left-to-right inside.
    group_frame(ui, dark, add)
}

/// Compact horizontal toolbar button: icon + label, highlighted when selected.
fn bar_button(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool, enabled: bool) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let fill = if selected {
        if dark { egui::Color32::from_white_alpha(24) } else { egui::Color32::from_black_alpha(24) }
    } else {
        egui::Color32::TRANSPARENT
    };
    let color = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color().gamma_multiply(0.6) };
    let text = RichText::new(format!("{icon} {label}")).size(12.5).color(color);
    ui.add_enabled(enabled, egui::Button::new(text).fill(fill).corner_radius(6.0).min_size(egui::vec2(0.0, 26.0)))
}

/// Sidebar header + branch tree.
fn sidebar(ui: &mut egui::Ui, view: &mut RepoView, settings: &mut Settings) {
    ui.add_space(8.0);
    ui.label(RichText::new(&view.name).size(15.0).strong());
    ui.add_space(4.0);
    ui.add_space(4.0);
    ui.separator();
    ui::branch_tree::show(ui, view, settings);
}
