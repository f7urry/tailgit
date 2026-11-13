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
    layout: PaneLayout,
    show_settings: bool,
    applied_font: crate::settings::UiFont,
}


impl TailApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> TailApp {
        let settings = Settings::load();
        let settings_font = settings.ui_font;
        let mut app = TailApp {
            path_input: String::new(),
            settings,
            repo: None,
            error: None,
            was_focused: true,
            show_sidebar: true,
            layout: PaneLayout::All,
            show_settings: false,
            applied_font: settings_font,
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
        let mut close = false;

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
                if self.settings.ui_font != self.applied_font {
                    self.applied_font = self.settings.ui_font;
                    crate::theme::install_fonts(&ctx, self.applied_font);
                }
            }
        }
        if self.repo.is_none() {
            self.welcome(ui);
            return;
        }
        let mut lay = self.layout;
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
        let show_sidebar = self.show_sidebar;
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
            ui.horizontal(|ui| {
                ui.add_space(if show_sidebar { 4.0 } else { 78.0 });
                ui.spacing_mut().item_spacing.x = 2.0;
                if tool_button(ui, "\u{f2f1}", "Refresh", 0, true).clicked() {
                    view.refresh_all();
                }
                if tool_button(ui, "\u{f381}", "Fetch", 0, !busy).clicked() {
                    view.run(Op::Fetch);
                }
                let pull = tool_button(ui, "\u{f0ab}", "Pull", behind, !busy);
                egui::Popup::menu(&pull).show(|ui| {
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
                if tool_button(ui, "\u{f0aa}", "Push", ahead, !busy).clicked() {
                    match ui::dialogs::push_dialog(view) {
                        Some(d) => view.dialog = Some(d),
                        None => view.notify(false, "Nothing to push: no current branch or no remotes".into()),
                    }
                }
                if tool_button(ui, "\u{f187}", "Stash", 0, !busy).clicked() {
                    view.dialog = Some(Dialog::Stash {
                        message: String::new(),
                        untracked: false,
                        keep_index: false,
                    });
                }
                ui.add_space(12.0);

                // Centered repository / branch pill.
                let right_w = 90.0;
                let pill_w = (ui.available_width() - right_w - 12.0).clamp(160.0, 520.0);
                let dark = ui.visuals().dark_mode;
                egui::Frame::new()
                    .fill(if dark { egui::Color32::from_white_alpha(10) } else { egui::Color32::from_black_alpha(10) })
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(10, 3))
                    .show(ui, |ui| {
                        ui.set_width(pill_w - 20.0);
                        ui.vertical_centered(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.menu_button(RichText::new(&view.name).size(14.0).strong(), |ui| {
                                if ui.button("Open Repository…").clicked() {
                                    open_request = rfd::FileDialog::new().pick_folder();
                                    ui.close();
                                }
                                ui.separator();
                                for r in settings.recent.clone() {
                                    if r != view.root && ui.button(&r).clicked() {
                                        open_request = Some(PathBuf::from(r));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                if ui.button("Close Repository").clicked() {
                                    close = true;
                                    ui.close();
                                }
                            });
                            branch_menu(ui, view);
                        });
                    });

                ui.add_space(6.0);
                if tool_button(ui, "\u{f126}", "Branch", 0, !busy).clicked() {
                    let head = view.log.data.as_ref().and_then(|d| d.refs.head.branch_name().map(str::to_string)).unwrap_or_else(|| "HEAD".into());
                    view.dialog = Some(Dialog::NewBranch {
                        start: "HEAD".into(),
                        start_label: head,
                        name: String::new(),
                        checkout: true,
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    if ui.add(egui::Button::new("\u{f013}").frame(false)).on_hover_text("Settings (\u{2318},)").clicked() {
                        self.show_settings = true;
                    }
                    if let Some(b) = &view.busy {
                        ui.spinner().on_hover_text(format!("{b}\n{}", view.progress));
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
                // Centered pane-layout switcher.
                let cx = ui.max_rect().center().x;
                crate::ui::layout_switcher(ui, cx, &mut lay, "main");
                if let Some((ok, text, _)) = &view.notice {
                    let color = if *ok { crate::theme::sys::green(ui.visuals().dark_mode) } else { crate::theme::sys::red(ui.visuals().dark_mode) };
                    ui.label(RichText::new(text).color(color));
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
                if lay == PaneLayout::All {
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
        self.layout = lay;
        view.auto_apply_non_conflicting = settings.merge_auto_apply;
        ui::conflicts::show(&ctx, view, settings);
        ui::dialogs::show(&ctx, view);

        if view.needs_repaint_soon() || view.busy.is_some() || view.log.loading {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        if close {
            self.repo = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Title("TailGIT".into()));
        }
        if let Some(p) = open_request {
            self.open(p, &ctx);
        }
    }
}

/// Toolbar button: large glyph over a small caption, with an optional count badge.
fn tool_button(ui: &mut egui::Ui, icon: &str, label: &str, badge: u32, enabled: bool) -> egui::Response {
    use egui::{Align2, FontId, Sense, Vec2};
    let size = Vec2::new(58.0, 44.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let v = ui.visuals();
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.rect_filled(rect, 6.0, v.widgets.hovered.weak_bg_fill.gamma_multiply(0.6));
    }
    let color = if enabled { v.text_color() } else { v.weak_text_color().gamma_multiply(0.6) };
    p.text(rect.center_top() + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), color);
    p.text(rect.center_bottom() - Vec2::new(0.0, 9.0), Align2::CENTER_CENTER, label, FontId::proportional(11.5), if enabled { v.weak_text_color() } else { color });
    if badge > 0 {
        let c = rect.center_top() + Vec2::new(12.0, 6.0);
        let text = badge.to_string();
        let r = 7.0f32.max(3.5 * text.len() as f32 + 2.0);
        p.rect_filled(egui::Rect::from_center_size(c, Vec2::new(r * 2.0, 14.0)), 7.0, egui::Color32::from_gray(if v.dark_mode { 110 } else { 120 }));
        p.text(c, Align2::CENTER_CENTER, text, FontId::proportional(10.0), egui::Color32::WHITE);
    }
    resp
}

/// Sidebar header + navigation items + branch tree.
fn sidebar(ui: &mut egui::Ui, view: &mut RepoView, settings: &mut Settings) {
    use egui::{Align2, FontId, Sense, Vec2};
    ui.add_space(8.0);
    ui.label(RichText::new(&view.name).size(15.0).strong());
    ui.add_space(4.0);
    let changes = view.change_count();
    let stashes = view.stashes.len();
    let before = view.tab;
    let dark = ui.visuals().dark_mode;
    for (tab, icon, label) in [
        (Tab::Commit, "\u{f15c}", if changes > 0 { format!("Local Changes ({changes})") } else { "Local Changes".to_string() }),
        (Tab::Log, "\u{f1da}", "All Commits".to_string()),
        (Tab::Stash, "\u{f187}", if stashes > 0 { format!("Stashes ({stashes})") } else { "Stashes".to_string() }),
    ] {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
        let p = ui.painter();
        let sel = view.tab == tab;
        if sel {
            p.rect_filled(rect, 6.0, if dark { egui::Color32::from_white_alpha(18) } else { egui::Color32::from_black_alpha(18) });
        } else if resp.hovered() {
            p.rect_filled(rect, 6.0, if dark { egui::Color32::from_white_alpha(8) } else { egui::Color32::from_black_alpha(8) });
        }
        let fg = ui.visuals().text_color();
        p.text(rect.left_center() + Vec2::new(14.0, 0.0), Align2::CENTER_CENTER, icon, FontId::proportional(14.0), fg);
        p.text(rect.left_center() + Vec2::new(30.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(14.0), fg);
        if resp.clicked() {
            view.tab = tab;
        }
    }
    if before != view.tab {
        view.diff = None;
        view.commit_selection = None;
        if view.tab == Tab::Commit {
            view.jobs.load_status();
        }
    }
    ui.add_space(4.0);
    ui.separator();
    ui::branch_tree::show(ui, view, settings);
}

fn branch_menu(ui: &mut egui::Ui, view: &mut RepoView) {
    let Some(data) = view.log.data.as_ref() else {
        ui.spinner();
        return;
    };
    let head = match &data.refs.head {
        git_core::refs::Head::Branch { name, .. } | git_core::refs::Head::Unborn { name } => name.clone(),
        git_core::refs::Head::Detached { hash } => format!("detached {}", &hash[..8.min(hash.len())]),
    };
    let current = data.refs.current_branch().cloned();
    let locals: Vec<String> = data.refs.local.iter().map(|b| b.name.clone()).collect();
    let mut op = None;
    let mut dialog = None;
    let label = match &current {
        Some(b) if b.ahead > 0 || b.behind > 0 => format!("\u{f126} {head}  ↑{} ↓{}  ⌄", b.ahead, b.behind),
        _ => format!("\u{f126} {head}  ⌄"),
    };
    ui.menu_button(RichText::new(label).size(12.0).weak(), |ui| {
        if ui.button("New Branch…").clicked() {
            dialog = Some(Dialog::NewBranch {
                start: "HEAD".into(),
                start_label: head.clone(),
                name: String::new(),
                checkout: true,
            });
            ui.close();
        }
        ui.separator();
        ui.weak("Checkout");
        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
            for b in &locals {
                let is_current = current.as_ref().is_some_and(|c| &c.name == b);
                if ui.add_enabled(!is_current, egui::Button::new(b)).clicked() {
                    op = Some(Op::Checkout(b.clone()));
                    ui.close();
                }
            }
        });
    });
    if let Some(d) = dialog {
        view.dialog = Some(d);
    }
    if let Some(o) = op {
        view.run(o);
    }
}
