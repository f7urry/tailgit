//! Central layout: branch sidebar and the per-tab panels.

use eframe::egui;

use super::widgets::sidebar;
use crate::repo::{RepoView, Tab};
use crate::settings::Settings;
use crate::ui::{self, PaneLayout};

/// Three-dot grip drawn in the gap between two resizable panes.
fn grip(ui: &egui::Ui, center: egui::Pos2, vertical: bool, dark: bool) {
    let color = if dark { egui::Color32::from_gray(120) } else { egui::Color32::from_gray(150) };
    for k in -1..=1 {
        let off = k as f32 * 6.0;
        let pos = if vertical { center + egui::vec2(0.0, off) } else { center + egui::vec2(off, 0.0) };
        ui.painter().circle_filled(pos, 1.6, color);
    }
}

/// Full-height branch sidebar (Fork / Finder layout), under the window controls.
pub(super) fn sidebar_panel(ui: &mut egui::Ui, view: &mut RepoView, settings: &mut Settings, lay: PaneLayout, show_sidebar: bool, dark: bool) {
    let full_w = ui.ctx().content_rect().width();
    if (show_sidebar && lay == PaneLayout::All) || lay == PaneLayout::Left {
        let panel = egui::Panel::left("branches");
        let panel = if lay == PaneLayout::Left { panel.exact_size(full_w - 16.0).resizable(false) } else { panel.resizable(true) };
        let resp = panel
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
        if lay != PaneLayout::Left {
            let r = resp.response.rect;
            grip(ui, egui::pos2(r.right() - 1.5, r.center().y), true, dark);
        }
    }
}

/// The main area for the active tab (log, changes or stashes) and its details/diff panes.
pub(super) fn tab_panels(ui: &mut egui::Ui, view: &mut RepoView, settings: &mut Settings, lay: PaneLayout, show_details: bool, dark: bool) {
    let isl = |o| crate::theme::island(dark, o);
    match view.tab {
        _ if lay == PaneLayout::Left => {}
        Tab::Log if lay == PaneLayout::Right => {
            egui::CentralPanel::default().frame(isl((6, 6, 0, 3))).show(ui, |ui| ui::details::show(ui, view));
        }
        Tab::Log => {
            if lay == PaneLayout::All && show_details {
                let r = egui::Panel::right("details")
                    .frame(isl((3, 6, 0, 3)))
                    .show_separator_line(false)
                    .resizable(true)
                    .default_size(380.0)
                    .show(ui, |ui| ui::details::show(ui, view))
                    .response
                    .rect;
                grip(ui, egui::pos2(r.left(), r.center().y), true, dark);
            }
            if view.diff.is_some() {
                let mut keep = true;
                let r = egui::Panel::bottom("log_diff")
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
                    })
                    .response
                    .rect;
                grip(ui, egui::pos2(r.center().x, r.top() + 1.5), false, dark);
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
                let r = egui::Panel::left("tab_left")
                    .frame(isl((3, 3, 0, 3)))
                    .show_separator_line(false)
                    .resizable(true)
                    .default_size(440.0)
                    .show(ui, |ui| list(ui, view))
                    .response
                    .rect;
                grip(ui, egui::pos2(r.right(), r.center().y), true, dark);
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
}
