//! Top-level window: owns app state and composes the toolbar, panels and dialogs.

mod content;
mod statusbar;
mod toolbar;
mod welcome;
mod widgets;

use std::path::PathBuf;

use eframe::egui;
use git_core::Git;

use crate::repo::RepoView;
use crate::settings::Settings;
use crate::ui::{self, PaneLayout};

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
        let settings_sidebar = settings.show_sidebar;
        let settings_details = settings.show_details;
        let settings_font = settings.ui_font.clone();
        let settings_size = settings.ui_font_size;
        let settings_weight = settings.ui_font_weight;
        let mut app = TailApp {
            path_input: String::new(),
            settings,
            repo: None,
            error: None,
            was_focused: true,
            show_sidebar: settings_sidebar,
            show_details: settings_details,
            layout: PaneLayout::All,
            show_settings: false,
            applied_font: settings_font,
            applied_size: settings_size,
            applied_weight: settings_weight,
        };
        crate::ui::settings_window::publish(&cc.egui_ctx, &app.settings);
        app.settings.log_columns = app.settings.log_columns.clone().sanitized();
        ui::log_table::set_log_columns(&cc.egui_ctx, app.settings.log_columns.clone());
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
        if view.split.is_some() && !settings.branch_click_split {
            view.close_split();
            view.log.inputs.branch = None;
            view.apply_filter();
        }

        let focused = ui.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.was_focused {
            view.jobs.load_status();
        }

        let dark = ui.visuals().dark_mode;
        ui.painter().rect_filled(ui.ctx().content_rect(), 0.0, crate::theme::gap_color(dark));
        let mut show_sidebar = self.show_sidebar;
        let mut show_details = self.show_details;

        content::sidebar_panel(ui, view, settings, lay, show_sidebar, dark);
        toolbar::show(ui, view, settings, show_sidebar, &mut self.show_settings, &mut open_request);
        statusbar::state_banner(ui, view, dark);
        statusbar::status_bar(ui, view, dark, &mut show_sidebar, &mut show_details);
        content::tab_panels(ui, view, settings, lay, show_details, dark);

        // Save column order / widths / visibility once the drag or click is finished.
        let cols = ui::log_table::log_columns(&ctx);
        if cols != settings.log_columns && !ctx.input(|i| i.pointer.any_down()) {
            settings.log_columns = cols;
            settings.save();
        }
        self.show_sidebar = show_sidebar;
        self.show_details = show_details;
        if settings.show_sidebar != show_sidebar || settings.show_details != show_details {
            settings.show_sidebar = show_sidebar;
            settings.show_details = show_details;
            settings.save();
        }
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
