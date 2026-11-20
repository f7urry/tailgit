//! Commit tool window: staged / unstaged / untracked / conflicted files and the commit message.

use eframe::egui::{self, RichText};
use git_core::status::StatusEntry;

use crate::ops::Op;
use crate::repo::RepoView;
use crate::ui::dialogs::{self, Dialog};
use crate::ui::{file_label, status_kind};

enum Action {
    Op(Op),
    Dialog(Dialog),
    Diff(StatusEntry, bool),
    Resolve,
    Amend(bool),
}

pub fn show(ui: &mut egui::Ui, view: &mut RepoView) {
    let mut actions: Vec<Action> = Vec::new();
    let conflicts: Vec<StatusEntry> = view.conflicts().cloned().collect();
    let staged: Vec<StatusEntry> = view.status.iter().filter(|e| e.is_staged()).cloned().collect();
    let unstaged: Vec<StatusEntry> = view.status.iter().filter(|e| e.is_unstaged()).cloned().collect();
    let untracked: Vec<StatusEntry> = view.status.iter().filter(|e| e.is_untracked()).cloned().collect();
    let selection = view.commit_selection.clone();
    let busy = view.busy.is_some();

    if let Some(e) = &view.status_error {
        ui.colored_label(crate::theme::sys::red(ui.visuals().dark_mode), e);
    }

    egui::Panel::bottom("commit_message")
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6)))
        .resizable(true)
        .default_size(190.0)
        .min_size(130.0)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let mut amend = view.amend;
                if ui.checkbox(&mut amend, "Amend").changed() {
                    actions.push(Action::Amend(amend));
                }
                ui.weak(format!("{} staged", staged.len()));
            });
            let h = (ui.available_height() - 36.0).max(40.0);
            egui::ScrollArea::vertical().id_salt("msg_scroll").max_height(h).show(ui, |ui| {
                ui.add_sized(
                    [ui.available_width(), h],
                    egui::TextEdit::multiline(&mut view.commit_message)
                        .hint_text("Commit message")
                        .font(egui::TextStyle::Monospace),
                );
            });
            ui.horizontal(|ui| {
                let can_commit = !busy
                    && conflicts.is_empty()
                    && !view.commit_message.trim().is_empty()
                    && (!staged.is_empty() || view.amend);
                let msg = view.commit_message.clone();
                if ui
                    .add_enabled(can_commit, egui::Button::new(if view.amend { "Amend Commit" } else { "Commit" }))
                    .clicked()
                {
                    actions.push(Action::Op(Op::Commit {
                        message: msg.clone(),
                        amend: view.amend,
                        push_after: false,
                    }));
                }
                if ui
                    .add_enabled(can_commit, egui::Button::new("Commit and Push…"))
                    .clicked()
                {
                    actions.push(Action::Op(Op::Commit {
                        message: msg,
                        amend: view.amend,
                        push_after: true,
                    }));
                }
                if !conflicts.is_empty() {
                    ui.colored_label(crate::theme::sys::orange(ui.visuals().dark_mode), "Resolve conflicts first");
                }
            });
        });

    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Stage All")).clicked() {
                actions.push(Action::Op(Op::StageAll));
            }
            let staged_paths: Vec<String> = staged.iter().map(|e| e.path.clone()).collect();
            if ui
                .add_enabled(!busy && !staged_paths.is_empty(), egui::Button::new("Unstage All"))
                .clicked()
            {
                actions.push(Action::Op(Op::Unstage(staged_paths)));
            }
            // Target: the clicked file if any, otherwise every checked (staged) file.
            let clicked: Option<StatusEntry> = selection
                .as_ref()
                .and_then(|(_, path)| view.status.iter().find(|e| &e.path == path).cloned());
            let targets: Vec<StatusEntry> = match clicked {
                Some(e) => vec![e],
                None => staged.clone(),
            };
            let hover = match targets.as_slice() {
                [] => "Nothing to roll back".to_string(),
                [e] if e.is_untracked() => format!("Delete untracked '{}' from disk", e.path),
                [e] => format!("Discard all changes to '{}' and restore from HEAD", e.path),
                many => format!("Discard all changes to {} checked files and restore from HEAD", many.len()),
            };
            if ui
                .add_enabled(!busy && !targets.is_empty(), egui::Button::new("Rollback"))
                .on_hover_text(hover)
                .clicked()
            {
                actions.push(Action::Dialog(rollback_dialog(&targets)));
            }
            if ui.button("\u{f2f1}").on_hover_text("Refresh").clicked() {
                view.jobs.load_status();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("status_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !conflicts.is_empty() {
                    egui::CollapsingHeader::new(
                        RichText::new(format!("Merge Conflicts ({})", conflicts.len())).color(crate::theme::sys::orange(ui.visuals().dark_mode)),
                    )
                    .id_salt("st_conflicts")
                    .default_open(true)
                    .show(ui, |ui| {
                        if ui.button("Resolve…").clicked() {
                            actions.push(Action::Resolve);
                        }
                        for e in &conflicts {
                            let sel = selection.as_ref() == Some(&(false, e.path.clone()));
                            ui.horizontal(|ui| {
                                if file_label(ui, git_core::log::ChangeKind::Unmerged, &e.path, sel).clicked() {
                                    actions.push(Action::Diff(e.clone(), false));
                                }
                                ui.weak(e.conflict_description());
                            });
                        }
                    });
                }
                section(ui, "Staged", "st_staged", &staged, true, &selection, &mut actions);
                section(ui, "Changes", "st_unstaged", &unstaged, false, &selection, &mut actions);
                section(ui, "Unversioned Files", "st_untracked", &untracked, false, &selection, &mut actions);
                if view.status.is_empty() {
                    ui.weak("Nothing to commit, working tree clean");
                }
            });
    });

    for a in actions {
        match a {
            Action::Op(op) => view.run(op),
            Action::Dialog(d) => view.dialog = Some(d),
            Action::Diff(e, staged) => view.open_status_diff(&e, staged),
            Action::Resolve => view.open_conflicts(),
            Action::Amend(on) => {
                view.amend = on;
                if on && view.commit_message.trim().is_empty() {
                    view.jobs.load_last_message();
                }
            }
        }
    }
}

fn section(
    ui: &mut egui::Ui,
    title: &str,
    id: &str,
    entries: &[StatusEntry],
    staged: bool,
    selection: &Option<(bool, String)>,
    actions: &mut Vec<Action>,
) {
    if entries.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("{title} ({})", entries.len()))
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            for e in entries {
                let code = if staged { e.index } else { e.worktree };
                let sel = selection.as_ref() == Some(&(staged, e.path.clone()));
                ui.horizontal(|ui| {
                    let mut checked = staged;
                    let hover = if staged { "Unstage" } else { "Stage" };
                    if ui.checkbox(&mut checked, "").on_hover_text(hover).changed() {
                        let paths = vec![e.path.clone()];
                        actions.push(Action::Op(if staged { Op::Unstage(paths) } else { Op::Stage(paths) }));
                    }
                    let r = file_label(ui, status_kind(code), &e.path, sel);
                    let r = match &e.orig_path {
                        Some(o) => r.on_hover_text(format!("{o} → {}", e.path)),
                        None => r,
                    };
                    if r.clicked() {
                        actions.push(Action::Diff(e.clone(), staged));
                    }
                    r.context_menu(|ui| entry_menu(ui, e, staged, actions));
                });
            }
        });
}

fn entry_menu(ui: &mut egui::Ui, e: &StatusEntry, staged: bool, actions: &mut Vec<Action>) {
    ui.spacing_mut().button_padding = egui::vec2(14.0, 6.0);
    let paths = vec![e.path.clone()];
    if ui.button("Show Diff").clicked() {
        actions.push(Action::Diff(e.clone(), staged));
        ui.close();
    }
    if staged {
        if ui.button("Unstage").clicked() {
            actions.push(Action::Op(Op::Unstage(paths.clone())));
            ui.close();
        }
    } else if ui.button("Stage").clicked() {
        actions.push(Action::Op(Op::Stage(paths.clone())));
        ui.close();
    }
    ui.separator();
    let label = if e.is_untracked() { "Delete File…" } else { "Rollback…" };
    if ui.button(label).clicked() {
        actions.push(Action::Dialog(rollback_dialog(std::slice::from_ref(e))));
        ui.close();
    }
    if ui.button("Copy Path").clicked() {
        ui.ctx().copy_text(e.path.clone());
        ui.close();
    }
}

/// Confirmation dialog for discarding files: deletes untracked files, rolls back tracked ones.
fn rollback_dialog(entries: &[StatusEntry]) -> Dialog {
    let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    if let [e] = entries {
        if e.is_untracked() {
            return dialogs::confirm(
                "Delete File",
                format!("Delete untracked '{}' from disk? It cannot be recovered with git.", e.path),
                Op::DeleteUntracked(paths),
                true,
            )
            .expect("dialog");
        }
    }
    let what = match entries {
        [e] => format!("'{}'", e.path),
        _ => format!("{} files", entries.len()),
    };
    dialogs::confirm(
        "Rollback Changes",
        format!("Discard all changes to {what} (staged and unstaged) and restore from HEAD?"),
        Op::Rollback(paths),
        true,
    )
    .expect("dialog")
}
