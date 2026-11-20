//! Title bar + toolbar: two button pills (git actions, views/open/settings).

use std::path::PathBuf;

use eframe::egui;

use super::widgets::{bar_button, group_frame};
use crate::ops::Op;
use crate::repo::{RepoView, Tab};
use crate::settings::Settings;
use crate::ui::{self, dialogs::Dialog};
use git_core::ops::PullMode;

pub(super) fn show(
    ui: &mut egui::Ui,
    view: &mut RepoView,
    settings: &mut Settings,
    show_sidebar: bool,
    show_settings: &mut bool,
    open_request: &mut Option<PathBuf>,
) {
    let dark = ui.visuals().dark_mode;
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
            ui.add_space(if show_sidebar { 4.0 } else { 84.0 });
            ui.spacing_mut().item_spacing.x = 8.0;
            let dark = ui.visuals().dark_mode;

            let changes = view.change_count();
            let stashes = view.stashes.len();
            let before = view.tab;
            let cur = view.tab;
            let target = {
                let sel = view.log.inputs.branch.as_deref().filter(|b| *b != "HEAD");
                view.log.data.as_ref().and_then(|d| {
                    sel.and_then(|n| d.refs.find_local(n)).or_else(|| d.refs.current_branch()).map(|b| (b.name.clone(), b.is_current))
                })
            };
            let (fetch_r, pull_r, push_r, stash_r, branch_r, rename_r, drop_r) = group_frame(ui, dark, |ui| {
                let pull_label = if behind > 0 { format!("Pull {behind}") } else { "Pull".to_string() };
                let push_label = if ahead > 0 { format!("Push {ahead}") } else { "Push".to_string() };
                (
                    bar_button(ui, "\u{f381}", "Fetch", false, !busy),
                    bar_button(ui, "\u{f0ab}", &pull_label, false, !busy),
                    bar_button(ui, "\u{f0aa}", &push_label, false, !busy),
                    bar_button(ui, "\u{f187}", "Stash", false, !busy),
                    bar_button(ui, "\u{f067}", "Branch", false, !busy),
                    bar_button(ui, "\u{f044}", "Rename", false, !busy && target.is_some())
                        .on_hover_text("Rename the branch selected in the log filter (default: current)"),
                    bar_button(ui, "\u{f1f8}", "Drop", false, !busy && target.as_ref().is_some_and(|t| !t.1))
                        .on_hover_text("Delete the branch selected in the log filter (the current branch can't be deleted)"),
                )
            });
            if branch_r.clicked() {
                let head = view.log.data.as_ref().and_then(|d| d.refs.head.branch_name().map(str::to_string)).unwrap_or_else(|| "HEAD".into());
                view.dialog = Some(Dialog::NewBranch {
                    start: "HEAD".into(),
                    start_label: head,
                    name: String::new(),
                    checkout: true,
                });
            }
            if let Some((name, _)) = &target {
                if rename_r.clicked() {
                    view.dialog = Some(Dialog::RenameBranch { old: name.clone(), name: name.clone() });
                }
                if drop_r.clicked() {
                    view.dialog = Some(Dialog::Confirm {
                        title: "Delete Branch".into(),
                        text: format!("Delete local branch '{name}'? Unmerged commits are refused unless you force-delete."),
                        op: Op::DeleteBranch { name: name.clone(), force: false },
                        danger: true,
                        ok: "OK".into(),
                    });
                }
            }
            if fetch_r.clicked() {
                view.run(Op::Fetch);
            }
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
                ui.add_space(6.0);
                let (set_r, open_r, st, lc, ac) = group_frame(ui, dark, |ui| {
                    let lc_label = if changes > 0 { format!("Changes {changes}") } else { "Changes".to_string() };
                    let st_label = if stashes > 0 { format!("Stashes {stashes}") } else { "Stashes".to_string() };
                    (
                        // This pill lives in a right-to-left row, so buttons are added in reverse.
                        bar_button(ui, "\u{f013}", "", false, true).on_hover_text("Settings (\u{2318},)"),
                        bar_button(ui, "\u{f07c}", "Open", false, true),
                        bar_button(ui, "\u{f187}", &st_label, cur == Tab::Stash, true),
                        bar_button(ui, "\u{f15c}", &lc_label, cur == Tab::Commit, true),
                        bar_button(ui, "\u{f1da}", "Commits", cur == Tab::Log, true),
                    )
                });
                if set_r.clicked() {
                    *show_settings = true;
                }
                egui::Popup::menu(&open_r).show(|ui| {
                    ui.set_width(240.0);
                    if ui.button("\u{f07c}  Open Folder\u{2026}").clicked() {
                        *open_request = rfd::FileDialog::new().pick_folder();
                        ui.close();
                    }
                    ui.separator();
                    if settings.recent.is_empty() {
                        ui.weak("No recent folders");
                    }
                    let mut remove: Option<String> = None;
                    for r in settings.recent.iter().take(5) {
                        let name = std::path::Path::new(r)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| r.clone());
                        let row_h = ui.spacing().interact_size.y;
                        let x_w = 24.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let name_w = ui.available_width() - x_w - 2.0;
                            let b = ui.add(
                                egui::Button::new((name, egui::Atom::grow()))
                                    .frame(false)
                                    .min_size(egui::vec2(name_w, row_h))
                                    .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                            if b.on_hover_text(r).clicked() {
                                *open_request = Some(PathBuf::from(r));
                                ui.close();
                            }
                            if ui
                                .add_sized([x_w, row_h], egui::Button::new("\u{f00d}").frame(false))
                                .on_hover_text("Remove from recent")
                                .clicked()
                            {
                                remove = Some(r.clone());
                            }
                        });
                    }
                    if let Some(r) = remove {
                        settings.remove_recent(&r);
                    }
                    if !settings.recent.is_empty() {
                        ui.separator();
                        if ui.button("Clear All").clicked() {
                            settings.clear_recent();
                            ui.close();
                        }
                    }
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

                if let Some(b) = &view.busy {
                    ui.spinner().on_hover_text(format!("{b}\n{}", view.progress));
                }
            });
        });
        ui.add_space(7.0);
    });
}
