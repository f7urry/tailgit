//! Modal dialogs: confirmations and small forms.

use eframe::egui::{self, RichText};
use git_core::content::Source;
use git_core::log::{ChangeKind, FileChange};
use git_core::ops::{PushOptions, ResetMode};

use crate::ops::Op;
use crate::jobs::DiffRequest;
use crate::repo::RepoView;

pub enum Dialog {
    Confirm {
        title: String,
        text: String,
        op: Op,
        danger: bool,
        /// Label of the accept button.
        ok: String,
    },
    NewBranch {
        start: String,
        start_label: String,
        name: String,
        checkout: bool,
    },
    RenameBranch {
        old: String,
        name: String,
    },
    NewTag {
        target: String,
        target_label: String,
        name: String,
        message: String,
    },
    Push {
        remote: String,
        remotes: Vec<String>,
        local: String,
        remote_branch: String,
        set_upstream: bool,
        force: bool,
        tags: bool,
        ahead: u32,
        has_upstream: bool,
    },
    Stash {
        message: String,
        untracked: bool,
        keep_index: bool,
    },
    StashBranch {
        selector: String,
        name: String,
    },
    Reset {
        target: String,
        label: String,
        mode: ResetMode,
    },
    Merge {
        branch: String,
        no_ff: bool,
        squash: bool,
    },
    SetUpstream {
        branch: String,
        upstream: String,
        candidates: Vec<String>,
    },
    /// Files that differ between `rev` and the working tree; `None` while loading.
    WorktreeDiff {
        rev: String,
        files: Option<Result<Vec<FileChange>, String>>,
    },
}

pub fn confirm(title: &str, text: impl Into<String>, op: Op, danger: bool) -> Option<Dialog> {
    confirm_with(title, text, op, danger, "OK")
}

/// A confirmation whose accept button reads `ok`.
pub fn confirm_with(title: &str, text: impl Into<String>, op: Op, danger: bool, ok: &str) -> Option<Dialog> {
    Some(Dialog::Confirm {
        title: title.to_string(),
        text: text.into(),
        op,
        danger,
        ok: ok.to_string(),
    })
}

/// Prepares the push dialog for the current branch.
pub fn push_dialog(view: &RepoView) -> Option<Dialog> {
    let data = view.log.data.as_ref()?;
    let branch = data.refs.current_branch()?;
    push_dialog_for(&branch.name, branch.upstream.as_deref(), branch.ahead, &data.refs.remotes)
}

/// Build the push dialog for an arbitrary local branch.
pub fn push_dialog_for(name: &str, upstream: Option<&str>, ahead: u32, remotes: &[String]) -> Option<Dialog> {
    if remotes.is_empty() {
        return None;
    }
    let (remote, remote_branch) = match upstream {
        Some(up) => {
            let r = remotes
                .iter()
                .filter(|r| up.starts_with(&format!("{r}/")))
                .max_by_key(|r| r.len())
                .cloned()
                .unwrap_or_else(|| remotes[0].clone());
            let b = up.get(r.len() + 1..).unwrap_or(name).to_string();
            (r, b)
        }
        None => (
            remotes
                .iter()
                .find(|r| *r == "origin")
                .cloned()
                .unwrap_or_else(|| remotes[0].clone()),
            name.to_string(),
        ),
    };
    Some(Dialog::Push {
        remote,
        remotes: remotes.to_vec(),
        local: name.to_string(),
        remote_branch,
        set_upstream: upstream.is_none(),
        force: false,
        tags: false,
        ahead,
        has_upstream: upstream.is_some(),
    })
}

enum Outcome {
    Keep,
    Close,
    Run(Op),
    OpenDiff(DiffRequest),
}

pub fn show(ctx: &egui::Context, view: &mut RepoView) {
    let Some(mut dialog) = view.dialog.take() else {
        return;
    };
    let mut outcome = Outcome::Keep;
    let modal = egui::Modal::new(egui::Id::unique("tailgit_dialog")).show(ctx, |ui| {
        ui.set_min_width(380.0);
        ui.set_max_width(560.0);
        outcome = body(ui, &mut dialog);
    });
    if modal.should_close() && matches!(outcome, Outcome::Keep) {
        outcome = Outcome::Close;
    }
    match outcome {
        Outcome::Keep => view.dialog = Some(dialog),
        Outcome::Close => {}
        Outcome::Run(op) => view.run(op),
        Outcome::OpenDiff(request) => view.open_diff(request),
    }
}

fn buttons(ui: &mut egui::Ui, ok: &str, enabled: bool, danger: bool) -> Option<bool> {
    let mut res = None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let mut text = RichText::new(ok);
        if danger {
            text = text.color(crate::theme::sys::red(ui.visuals().dark_mode));
        }
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.add_enabled(enabled, egui::Button::new(text)).clicked() || (enabled && enter && !danger) {
            res = Some(true);
        }
        if ui.button("Cancel").clicked() {
            res = Some(false);
        }
    });
    res
}

fn name_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    let r = ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    if !r.has_focus() && value.is_empty() {
        r.request_focus();
    }
}

fn body(ui: &mut egui::Ui, dialog: &mut Dialog) -> Outcome {
    let close_or = |r: Option<bool>, op: &dyn Fn() -> Op| match r {
        Some(true) => Outcome::Run(op()),
        Some(false) => Outcome::Close,
        None => Outcome::Keep,
    };
    match dialog {
        Dialog::Confirm {
            title,
            text,
            op,
            danger,
            ok,
        } => {
            ui.heading(title.as_str());
            ui.label(text.as_str());
            let op = op.clone();
            close_or(buttons(ui, ok, true, *danger), &|| op.clone())
        }
        Dialog::NewBranch {
            start,
            start_label,
            name,
            checkout,
        } => {
            ui.heading("New Branch");
            ui.label(format!("From {start_label}"));
            name_field(ui, "Branch name:", name);
            ui.checkbox(checkout, "Checkout branch");
            let r = buttons(ui, "Create", !name.trim().is_empty(), false);
            let (n, s, c) = (name.trim().to_string(), start.clone(), *checkout);
            close_or(r, &|| {
                if c {
                    Op::CheckoutNewBranch {
                        name: n.clone(),
                        start: s.clone(),
                    }
                } else {
                    Op::CreateBranch {
                        name: n.clone(),
                        start: s.clone(),
                    }
                }
            })
        }
        Dialog::RenameBranch { old, name } => {
            ui.heading(format!("Rename {old}"));
            name_field(ui, "New name:", name);
            let r = buttons(ui, "Rename", !name.trim().is_empty() && name.trim() != old, false);
            let (o, n) = (old.clone(), name.trim().to_string());
            close_or(r, &|| Op::RenameBranch {
                old: o.clone(),
                new: n.clone(),
            })
        }
        Dialog::NewTag {
            target,
            target_label,
            name,
            message,
        } => {
            ui.heading("New Tag");
            ui.label(format!("On {target_label}"));
            name_field(ui, "Tag name:", name);
            ui.label("Message (optional, creates an annotated tag):");
            ui.add(egui::TextEdit::multiline(message).desired_rows(3).desired_width(f32::INFINITY));
            let r = buttons(ui, "Create", !name.trim().is_empty(), false);
            let (t, n, m) = (target.clone(), name.trim().to_string(), message.clone());
            close_or(r, &|| Op::CreateTag {
                name: n.clone(),
                target: t.clone(),
                message: m.clone(),
            })
        }
        Dialog::Push {
            remote,
            remotes,
            local,
            remote_branch,
            set_upstream,
            force,
            tags,
            ahead,
            has_upstream,
        } => {
            ui.heading("Push Commits");
            if *has_upstream {
                ui.label(format!("{ahead} commit(s) to push from {local}"));
            } else {
                ui.label(format!("{local} has no upstream yet"));
            }
            egui::Grid::new("push_grid").num_columns(2).show(ui, |ui| {
                ui.label("Remote:");
                egui::ComboBox::from_id_salt("push_remote")
                    .selected_text(remote.as_str())
                    .show_ui(ui, |ui| {
                        for r in remotes.iter() {
                            ui.selectable_value(remote, r.clone(), r);
                        }
                    });
                ui.end_row();
                ui.label("Remote branch:");
                ui.text_edit_singleline(remote_branch);
                ui.end_row();
            });
            ui.checkbox(set_upstream, "Set upstream (track remote branch)");
            ui.checkbox(tags, "Push tags (--follow-tags)");
            ui.checkbox(force, "Force push (--force-with-lease)");
            let r = buttons(
                ui,
                if *force { "Force Push" } else { "Push" },
                !remote_branch.trim().is_empty(),
                *force,
            );
            let opts = PushOptions {
                remote: remote.clone(),
                local_branch: local.clone(),
                remote_branch: remote_branch.trim().to_string(),
                set_upstream: *set_upstream,
                force_with_lease: *force,
                tags: *tags,
                no_verify: false,
            };
            close_or(r, &|| Op::Push(opts.clone()))
        }
        Dialog::Stash {
            message,
            untracked,
            keep_index,
        } => {
            ui.heading("Stash Changes");
            name_field(ui, "Message:", message);
            ui.checkbox(untracked, "Include untracked files");
            ui.checkbox(keep_index, "Keep staged changes in the index");
            let r = buttons(ui, "Stash", true, false);
            let (m, u, k) = (message.clone(), *untracked, *keep_index);
            close_or(r, &|| Op::StashPush {
                message: m.clone(),
                include_untracked: u,
                keep_index: k,
            })
        }
        Dialog::StashBranch { selector, name } => {
            ui.heading(format!("Unstash {selector} as branch"));
            name_field(ui, "Branch name:", name);
            let r = buttons(ui, "Create", !name.trim().is_empty(), false);
            let (s, n) = (selector.clone(), name.trim().to_string());
            close_or(r, &|| Op::StashBranch {
                name: n.clone(),
                selector: s.clone(),
            })
        }
        Dialog::Reset { target, label, mode } => {
            ui.heading("Reset Current Branch");
            ui.label(format!("Reset HEAD to {label}"));
            ui.add_space(4.0);
            for (m, title, help) in [
                (ResetMode::Soft, "Soft", "Keep all changes, staged"),
                (ResetMode::Mixed, "Mixed", "Keep changes in the working tree, unstaged"),
                (ResetMode::Hard, "Hard", "Discard all local changes"),
                (ResetMode::Keep, "Keep", "Keep local changes not touched by the reset"),
            ] {
                ui.radio_value(mode, m, format!("{title} — {help}"));
            }
            let danger = *mode == ResetMode::Hard;
            let r = buttons(ui, "Reset", true, danger);
            let (t, m) = (target.clone(), *mode);
            close_or(r, &|| Op::Reset {
                target: t.clone(),
                mode: m,
            })
        }
        Dialog::Merge {
            branch,
            no_ff,
            squash,
        } => {
            ui.heading(format!("Merge {branch} into current branch"));
            ui.checkbox(no_ff, "Always create a merge commit (--no-ff)");
            ui.checkbox(squash, "Squash (--squash)");
            let r = buttons(ui, "Merge", true, false);
            let (b, n, s) = (branch.clone(), *no_ff, *squash);
            close_or(r, &|| Op::Merge {
                branch: b.clone(),
                no_ff: n,
                squash: s,
            })
        }
        Dialog::WorktreeDiff { rev, files } => {
            ui.heading(format!("Diff of '{rev}' with Working Tree"));
            let mut out = Outcome::Keep;
            match files {
                None => {
                    ui.weak("Loading…");
                }
                Some(Err(e)) => {
                    ui.colored_label(crate::theme::sys::red(ui.visuals().dark_mode), e.as_str());
                }
                Some(Ok(list)) if list.is_empty() => {
                    ui.weak("No differences.");
                }
                Some(Ok(list)) => {
                    ui.weak(format!("{} file(s) differ. Click a file to see its diff.", list.len()));
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for c in list.iter() {
                            if crate::ui::file_label(ui, c.kind, &c.path, false).clicked() {
                                let old = if c.kind == ChangeKind::Added {
                                    Source::Empty
                                } else {
                                    Source::Revision(rev.clone())
                                };
                                let new = if c.kind == ChangeKind::Deleted {
                                    Source::Empty
                                } else {
                                    Source::WorkTree
                                };
                                out = Outcome::OpenDiff(DiffRequest {
                                    title: c.path.clone(),
                                    old,
                                    old_path: c.old_path.clone().unwrap_or_else(|| c.path.clone()),
                                    new,
                                    new_path: c.path.clone(),
                                });
                            }
                        }
                    });
                }
            }
            ui.add_space(8.0);
            if ui.button("Close").clicked() {
                out = Outcome::Close;
            }
            out
        }
        Dialog::SetUpstream {
            branch,
            upstream,
            candidates,
        } => {
            ui.heading(format!("Set upstream of {branch}"));
            egui::ComboBox::from_id_salt("upstream_pick")
                .selected_text(upstream.as_str())
                .width(300.0)
                .show_ui(ui, |ui| {
                    for c in candidates.iter() {
                        ui.selectable_value(upstream, c.clone(), c);
                    }
                });
            let r = buttons(ui, "Set", !upstream.is_empty(), false);
            let (b, u) = (branch.clone(), upstream.clone());
            match r {
                Some(true) => Outcome::Run(Op::SetUpstream {
                    branch: b,
                    upstream: u,
                }),
                Some(false) => Outcome::Close,
                None => Outcome::Keep,
            }
        }
    }
}
