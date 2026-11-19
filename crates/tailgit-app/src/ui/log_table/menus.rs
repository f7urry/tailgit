//! Context menus for one or several selected commits.

use eframe::egui;
use git_core::ops::ResetMode;
use git_core::repo_state::RepoState;

use super::RowAction;
use crate::ops::Op;
use crate::ui::dialogs::{self, Dialog};

pub(super) fn row_menu(ui: &mut egui::Ui, c: &git_core::log::Commit, current: Option<&str>, repo_state: RepoState, actions: &mut Vec<RowAction>) {
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
    } else {
        busy_reason(repo_state, "drop commit")
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
pub(super) fn multi_menu(
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
