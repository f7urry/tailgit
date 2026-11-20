//! Repository-state banner, bottom status bar and operation output.

use eframe::egui::{self, RichText};
use git_core::repo_state::RepoState;

use crate::ops::Op;
use crate::repo::RepoView;
use crate::ui;

/// Merge / rebase / cherry-pick in progress banner.
pub(super) fn state_banner(ui: &mut egui::Ui, view: &mut RepoView, dark: bool) {
    let isl = |o| crate::theme::island(dark, o);
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
}

/// Bottom bar (output toggle, notices, undo, pane toggles, repo path) plus the output panel.
pub(super) fn status_bar(ui: &mut egui::Ui, view: &mut RepoView, dark: bool, show_sidebar: &mut bool, show_details: &mut bool) {
    let isl = |o| crate::theme::island(dark, o);
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
                ui.add_space(4.0);
                let tip = if *show_details { "Hide details pane" } else { "Show details pane" };
                if ui::pane_toggle(ui, true, *show_details, tip).clicked() {
                    *show_details = !*show_details;
                }
                let tip = if *show_sidebar { "Hide sidebar" } else { "Show sidebar" };
                if ui::pane_toggle(ui, false, *show_sidebar, tip).clicked() {
                    *show_sidebar = !*show_sidebar;
                }
                if ui::status_icon(ui, "\u{f07c}", "Open in Finder").clicked() {
                    ui::open_file_manager(&view.root);
                }
                if ui::status_icon(ui, "\u{f120}", "Open in Terminal").clicked() {
                    ui::open_terminal(&view.root);
                }
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(&view.root).weak()).truncate());
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


}
