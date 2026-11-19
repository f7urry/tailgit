//! Stash list with apply / pop / drop / branch, and the stash's changed files.

use eframe::egui::{self, RichText};

use crate::ops::Op;
use crate::repo::RepoView;
use crate::ui::dialogs::{self, Dialog};
use crate::ui::{file_label, format_time};

pub fn show(ui: &mut egui::Ui, view: &mut RepoView) {
    let busy = view.busy.is_some();
    let mut op: Option<Op> = None;
    let mut dialog: Option<Dialog> = None;
    let mut select: Option<String> = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(!busy, egui::Button::new("Stash Changes…")).clicked() {
            dialog = Some(Dialog::Stash {
                message: String::new(),
                untracked: false,
                keep_index: false,
            });
        }
        if ui.button("\u{f2f1}").on_hover_text("Refresh").clicked() {
            view.jobs.load_stashes();
        }
    });
    ui.separator();
    if view.stashes.is_empty() {
        ui.weak("No stashes");
    }
    if let Some(sel) = view.stash_selected.clone() {
        if let Some(s) = view.stashes.iter().find(|s| s.hash == sel).cloned() {
            let avail = ui.available_height();
            egui::Panel::bottom("stash_detail")
                .frame(egui::Frame::new())
                .resizable(true)
                .default_size(avail * 0.5)
                .size_range(80.0..=(avail - 80.0).max(80.0))
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.strong(&s.selector);
                        for (label, o) in [
                            ("Apply", Op::StashApply { selector: s.selector.clone(), index: false }),
                            ("Pop", Op::StashPop { selector: s.selector.clone(), index: false }),
                        ] {
                            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                                op = Some(o);
                            }
                        }
                        if ui.add_enabled(!busy, egui::Button::new("Drop…")).clicked() {
                            dialog = dialogs::confirm(
                                "Drop Stash",
                                format!("Drop {} \"{}\"?", s.selector, s.message),
                                Op::StashDrop(s.selector.clone()),
                                true,
                            );
                        }
                    });
                    match view.stash_details.clone() {
                        None => {
                            ui.spinner();
                        }
                        Some(Err(e)) => {
                            ui.colored_label(crate::theme::sys::red(ui.visuals().dark_mode), e);
                        }
                        Some(Ok(d)) => {
                            egui::ScrollArea::vertical()
                                .id_salt("stash_files")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let open = view.diff.as_ref().map(|x| x.request.new_path.clone());
                                    for c in &d.changes {
                                        if file_label(ui, c.kind, &c.path, open.as_deref() == Some(c.path.as_str())).clicked() {
                                            view.open_commit_file_diff(&d, c);
                                        }
                                    }
                                });
                        }
                    }
                });
        }
    }
    egui::ScrollArea::vertical()
        .id_salt("stash_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for s in &view.stashes {
                let sel = view.stash_selected.as_deref() == Some(s.hash.as_str());
                let r = ui.selectable_label(
                    sel,
                    RichText::new(format!("{}  {}   {}", s.selector, s.message, format_time(s.time))),
                );
                if r.clicked() {
                    select = Some(s.hash.clone());
                }
                r.context_menu(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(14.0, 6.0);
                    let sel = s.selector.clone();
                    for (label, o) in [
                        ("Apply", Op::StashApply { selector: sel.clone(), index: false }),
                        ("Apply (restore index)", Op::StashApply { selector: sel.clone(), index: true }),
                        ("Pop", Op::StashPop { selector: sel.clone(), index: false }),
                    ] {
                        if ui.button(label).clicked() {
                            op = Some(o);
                            ui.close();
                        }
                    }
                    if ui.button("Unstash as Branch…").clicked() {
                        dialog = Some(Dialog::StashBranch {
                            selector: sel.clone(),
                            name: String::new(),
                        });
                        ui.close();
                    }
                    if ui.button("Drop…").clicked() {
                        dialog = dialogs::confirm(
                            "Drop Stash",
                            format!("Drop {} \"{}\"? This cannot be undone from TailGIT.", s.selector, s.message),
                            Op::StashDrop(sel.clone()),
                            true,
                        );
                        ui.close();
                    }
                });
            }
        });
    if let Some(h) = select {
        view.stash_selected = Some(h.clone());
        view.stash_details = None;
        view.jobs.load_stash_details(h);
    }
    if let Some(d) = dialog {
        view.dialog = Some(d);
    }
    if let Some(o) = op {
        view.run(o);
    }
}
