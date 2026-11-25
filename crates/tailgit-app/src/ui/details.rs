//! Commit details panel: changed files on top, metadata below.

use eframe::egui::{self, RichText};
use git_core::log::CommitDetails;

use crate::repo::RepoView;
use crate::ui::{file_label, format_full_time, ref_badge};

pub fn show(ui: &mut egui::Ui, view: &mut RepoView) {
    if view.log.selected_hash.is_none() && view.split.as_ref().is_none_or(|s| s.selected_hash.is_none()) {
        ui.centered_and_justified(|ui| ui.weak("Select commit to view changes"));
        return;
    }
    let details = match &view.details {
        None => {
            ui.centered_and_justified(|ui| ui.spinner());
            return;
        }
        Some(Err(e)) => {
            ui.colored_label(crate::theme::sys::red(ui.visuals().dark_mode), e);
            return;
        }
        Some(Ok(d)) => d.clone(),
    };

    egui::Panel::bottom("details_meta")
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6)))
        .resizable(true)
        .default_size(ui.available_height() * 0.45)
        .min_size(80.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("meta_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| meta(ui, view, &details));
        });

    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.strong(format!("{} changed file(s)", details.changes.len()));
            if details.parents.len() > 1 {
                ui.weak("(vs first parent)");
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("changes_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let open_path = view.diff.as_ref().map(|d| d.request.new_path.clone());
                for change in &details.changes {
                    let sel = open_path.as_deref() == Some(change.path.as_str()) && view.diff.is_some();
                    let r = file_label(ui, change.kind, &change.path, sel);
                    let r = match &change.old_path {
                        Some(old) => r.on_hover_text(format!("{old} → {}", change.path)),
                        None => r,
                    };
                    if r.clicked() {
                        view.open_commit_file_diff(&details, change);
                    }
                }
            });
    });
}

fn meta(ui: &mut egui::Ui, view: &mut RepoView, d: &CommitDetails) {
    ui.add(egui::Label::new(RichText::new(&d.subject).strong().size(15.0)).wrap());
    let body = d.message.strip_prefix(&d.subject).unwrap_or(&d.message).trim();
    if !body.is_empty() {
        ui.add_space(4.0);
        let mut text = body.to_string();
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .desired_width(f32::INFINITY)
                .desired_rows(1)
                .frame(egui::Frame::NONE)
                .interactive(true),
        );
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .link(RichText::new(&d.hash[..d.hash.len().min(10)]).monospace())
            .on_hover_text("Copy full hash")
            .clicked()
        {
            ui.ctx().copy_text(d.hash.clone());
        }
        ui.label(format!("{} <{}>", d.author_name, d.author_email));
        ui.weak(format!("on {}", format_full_time(d.author_time)));
    });
    if d.committer_name != d.author_name || d.committer_email != d.author_email {
        ui.weak(format!(
            "committed by {} <{}> on {}",
            d.committer_name,
            d.committer_email,
            format_full_time(d.commit_time)
        ));
    }
    let mut goto: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        ui.weak(if d.parents.len() > 1 { "Parents:" } else { "Parent:" });
        if d.parents.is_empty() {
            ui.weak("none (root commit)");
        }
        for p in &d.parents {
            if ui.link(RichText::new(&p[..8.min(p.len())]).monospace()).clicked() {
                goto = Some(p.clone());
            }
        }
    });
    if let Some(labels) = view
        .log
        .data
        .iter()
        .chain(view.split.iter().flat_map(|s| s.data.iter()))
        .find_map(|data| data.labels.get(&d.hash))
        .cloned() {
        ui.horizontal_wrapped(|ui| {
            for l in &labels {
                ref_badge(ui, l);
            }
        });
    }
    ui.add_space(4.0);
    match &view.containing {
        Some((h, Some(Ok(branches)))) if h == &d.hash => {
            egui::CollapsingHeader::new(format!("In {} branch(es)", branches.len()))
                .id_salt("containing")
                .default_open(branches.len() <= 12)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for b in branches {
                            ui.label(RichText::new(b).small());
                        }
                    });
                });
        }
        Some((h, Some(Err(e)))) if h == &d.hash => {
            ui.weak(e);
        }
        Some((h, None)) if h == &d.hash => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Finding containing branches…");
            });
        }
        _ => {
            if ui.small_button("Show containing branches").clicked() {
                view.request_containing(d.hash.clone());
            }
        }
    }
    if let Some(p) = goto {
        view.select_hash(&p);
    }
}
