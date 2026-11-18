//! Branch tree panel (port of git4idea's branches dashboard):
//! HEAD, Local / Remote (per remote) / Tags, folders split on `/`,
//! current branch and favorites first, ahead/behind counters.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use git_core::ops::ResetMode;
use git_core::refs::{Branch, RefKind};

use crate::ops::Op;
use crate::repo::RepoView;
use crate::settings::Settings;
use crate::ui::dialogs::{self, Dialog};

#[derive(Clone)]
struct Item {
    /// Short ref name used by git, e.g. `feat/x`, `origin/feat/x`, `v1.0`.
    name: String,
    /// Path inside its group (remote branches drop the remote prefix).
    path: String,
    kind: RefKind,
    current: bool,
    favorite: bool,
    ahead: u32,
    behind: u32,
    gone: bool,
    upstream: Option<String>,
    target: String,
    remote: Option<String>,
}

#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    leaves: Vec<(String, Item)>,
}

impl Folder {
    fn insert(&mut self, item: Item) {
        let segs: Vec<String> = item.path.split('/').map(str::to_string).collect();
        let mut node = self;
        for s in &segs[..segs.len() - 1] {
            node = node.folders.entry(s.clone()).or_default();
        }
        node.leaves.push((segs[segs.len() - 1].clone(), item));
    }

    fn sort(&mut self) {
        self.leaves.sort_by(|(a, ia), (b, ib)| {
            ib.current
                .cmp(&ia.current)
                .then(ib.favorite.cmp(&ia.favorite))
                .then(a.cmp(b))
        });
        for f in self.folders.values_mut() {
            f.sort();
        }
    }

    fn has_favorite(&self) -> bool {
        self.leaves.iter().any(|(_, i)| i.favorite || i.current) || self.folders.values().any(Folder::has_favorite)
    }
}

enum Action {
    Filter(Option<String>),
    Op(Op),
    Dialog(Dialog),
    WorktreeDiff(String),
    ToggleFavorite(String),
    Copy(String),
}

pub fn show(ui: &mut egui::Ui, view: &mut RepoView, settings: &mut Settings) {
    let Some(data) = view.log.data.as_ref() else {
        ui.weak("Loading branches…");
        return;
    };
    let refs = &data.refs;
    let root = view.root.clone();
    let make = |b: &Branch, kind: RefKind, remote: Option<&str>| Item {
        name: b.name.clone(),
        path: match remote {
            Some(r) => b.name[r.len() + 1..].to_string(),
            None => b.name.clone(),
        },
        kind,
        current: b.is_current,
        favorite: settings.is_favorite(&root, &b.name),
        ahead: b.ahead,
        behind: b.behind,
        gone: b.upstream_gone,
        upstream: b.upstream.clone(),
        target: b.target.clone(),
        remote: remote.map(str::to_string),
    };
    let local: Vec<Item> = refs.local.iter().map(|b| make(b, RefKind::LocalBranch, None)).collect();
    let mut remote_groups: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for b in &refs.remote {
        let (r, _) = b.split_remote(&refs.remotes).unwrap_or(("", ""));
        if r.is_empty() {
            continue;
        }
        remote_groups
            .entry(r.to_string())
            .or_default()
            .push(make(b, RefKind::RemoteBranch, Some(r)));
    }
    let tags: Vec<Item> = refs
        .tags
        .iter()
        .map(|t| Item {
            name: t.name.clone(),
            path: t.name.clone(),
            kind: RefKind::Tag,
            current: false,
            favorite: settings.is_favorite(&root, &t.name),
            ahead: 0,
            behind: 0,
            gone: false,
            upstream: None,
            target: t.target.clone(),
            remote: None,
        })
        .collect();
    let current = refs.head.branch_name().map(str::to_string);
    let remote_names: Vec<String> = refs
        .remote
        .iter()
        .map(|b| b.name.clone())
        .collect();
    let remotes = refs.remotes.clone();
    let selected = view.log.inputs.branch.clone();

    let mut actions: Vec<Action> = Vec::new();
    let search_box = ui.add(
        egui::TextEdit::singleline(&mut view.branch_search)
            .hint_text("Branch or tag")
            .margin(egui::Margin { left: 30, right: 10, top: 7, bottom: 7 })
            .desired_width(f32::INFINITY),
    );
    ui.painter().text(
        search_box.rect.left_center() + egui::vec2(15.0, 0.0),
        egui::Align2::CENTER_CENTER,
        "\u{f002}",
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().weak_text_color(),
    );
    let search = view.branch_search.trim().to_lowercase();
    let ctx = Ctx {
        current: current.as_deref(),
        selected: selected.as_deref(),
        remotes: &remotes,
        remote_names: &remote_names,
    };

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let head_label = match &refs.head {
                git_core::refs::Head::Detached { hash } => format!("HEAD (detached at {})", &hash[..8.min(hash.len())]),
                _ => "HEAD (Current Branch)".to_string(),
            };
            let head_sel = selected.as_deref() == Some("HEAD");
            if ui.selectable_label(head_sel, head_label).clicked() {
                actions.push(Action::Filter(if head_sel { None } else { Some("HEAD".into()) }));
            }
            group(ui, "Local", "grp_local", local, &search, &ctx, &mut actions);
            egui::CollapsingHeader::new("Remote")
                .id_salt("grp_remote")
                .default_open(true)
                .show(ui, |ui| {
                    for (remote, items) in remote_groups {
                        group(ui, &remote, &format!("grp_remote_{remote}"), items, &search, &ctx, &mut actions);
                    }
                });
            group(ui, "Tags", "grp_tags", tags, &search, &ctx, &mut actions);
        });

    for a in actions {
        match a {
            Action::Filter(f) => {
                view.log.inputs.branch = f;
                view.apply_filter();
            }
            Action::Op(op) => view.run(op),
            Action::Dialog(d) => view.dialog = Some(d),
            Action::WorktreeDiff(rev) => {
                view.jobs.load_worktree_diff(rev.clone());
                view.dialog = Some(Dialog::WorktreeDiff { rev, files: None });
            }
            Action::ToggleFavorite(n) => settings.toggle_favorite(&root, &n),
            Action::Copy(s) => ui.ctx().copy_text(s),
        }
    }
}

struct Ctx<'a> {
    current: Option<&'a str>,
    selected: Option<&'a str>,
    remotes: &'a [String],
    remote_names: &'a [String],
}

fn group(
    ui: &mut egui::Ui,
    title: &str,
    id: &str,
    items: Vec<Item>,
    search: &str,
    ctx: &Ctx,
    actions: &mut Vec<Action>,
) {
    let header = egui::CollapsingHeader::new(format!("{title}  ({})", items.len()))
        .id_salt(id)
        .default_open(title != "Tags");
    if !search.is_empty() {
        let mut matches: Vec<Item> = items
            .into_iter()
            .filter(|i| i.name.to_lowercase().contains(search))
            .collect();
        if matches.is_empty() {
            return;
        }
        matches.sort_by(|a, b| a.name.cmp(&b.name));
        header.open(Some(true)).show(ui, |ui| {
            for item in &matches {
                leaf(ui, &item.path, item, ctx, actions);
            }
        });
        return;
    }
    let mut root = Folder::default();
    for i in items {
        root.insert(i);
    }
    root.sort();
    header.show(ui, |ui| folder(ui, &root, id, ctx, actions));
}

fn folder(ui: &mut egui::Ui, f: &Folder, id: &str, ctx: &Ctx, actions: &mut Vec<Action>) {
    for (name, sub) in &f.folders {
        let sub_id = format!("{id}/{name}");
        egui::CollapsingHeader::new(icon_job(ui, "\u{f07b}", icon_gray(ui), name))
            .id_salt(&sub_id)
            .default_open(sub.has_favorite())
            .show(ui, |ui| folder(ui, sub, &sub_id, ctx, actions));
    }
    for (label, item) in &f.leaves {
        leaf(ui, label, item, ctx, actions);
    }
}

/// Muted gray used for folder and branch icons, as in IntelliJ's branch popup.
fn icon_gray(ui: &egui::Ui) -> egui::Color32 {
    crate::theme::sys::gray(ui.visuals().dark_mode)
}

/// Header text with a colored icon glyph followed by the plain label.
fn icon_job(ui: &egui::Ui, icon: &str, color: egui::Color32, label: &str) -> egui::WidgetText {
    use egui::text::{LayoutJob, TextFormat};
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut job = LayoutJob::default();
    job.append(icon, 0.0, TextFormat::simple(font.clone(), color));
    job.append(label, 6.0, TextFormat::simple(font, ui.visuals().text_color()));
    job.into()
}

fn leaf(ui: &mut egui::Ui, label: &str, item: &Item, ctx: &Ctx, actions: &mut Vec<Action>) {
    let selected = ctx.selected == Some(item.name.as_str());
    let icon = match (item.current, item.favorite, item.kind) {
        (true, _, _) => "\u{f058}",
        (_, true, _) => "\u{f005}",
        (_, _, RefKind::Tag) => "\u{f02b}",
        _ => "\u{f126}",
    };
    // IntelliJ-style: neutral icons for folders and branches, orange for tags,
    // yellow for favorites; the label itself keeps the default text color.
    let icon_color = match (item.favorite, item.kind) {
        (true, _) if !item.current => crate::theme::sys::yellow(ui.visuals().dark_mode),
        (_, RefKind::Tag) => crate::theme::sys::orange(ui.visuals().dark_mode),
        _ => icon_gray(ui),
    };
    let mut text = RichText::new(label);
    if item.current {
        text = text.strong();
    }
    let r = ui
        .horizontal(|ui| {
            ui.label(RichText::new(icon).color(icon_color));
            let r = ui.selectable_label(selected, text);
            if item.ahead > 0 {
                ui.label(RichText::new(format!("↑{}", item.ahead)).small().color(crate::theme::sys::green(ui.visuals().dark_mode)));
            }
            if item.behind > 0 {
                ui.label(RichText::new(format!("↓{}", item.behind)).small().color(crate::theme::sys::blue(ui.visuals().dark_mode)));
            }
            if item.gone {
                ui.label(RichText::new("gone").small().weak());
            }
            r
        })
        .inner;
    let r = r.on_hover_text(match &item.upstream {
        Some(u) => format!("{}\ntracks {u}", item.name),
        None => item.name.clone(),
    });
    if r.double_clicked() {
        actions.push(checkout_action(item, ctx));
    } else if r.clicked() {
        actions.push(Action::Filter(if selected { None } else { Some(item.name.clone()) }));
    }
    r.context_menu(|ui| menu(ui, item, ctx, actions));
}

fn checkout_action(item: &Item, _ctx: &Ctx) -> Action {
    match item.kind {
        RefKind::LocalBranch => Action::Op(Op::Checkout(item.name.clone())),
        RefKind::RemoteBranch => Action::Op(Op::CheckoutTracking {
            local: item.path.clone(),
            remote: item.name.clone(),
        }),
        _ => Action::Op(Op::CheckoutDetached(item.name.clone())),
    }
}

fn menu(ui: &mut egui::Ui, item: &Item, ctx: &Ctx, actions: &mut Vec<Action>) {
    ui.spacing_mut().button_padding = egui::vec2(14.0, 6.0);
    let current = ctx.current.unwrap_or("HEAD");
    let name = &item.name;
    let mut act = |ui: &mut egui::Ui, label: String, a: Action| {
        if ui.button(label).clicked() {
            actions.push(a);
            ui.close();
        }
    };
    if !item.current {
        act(ui, "Checkout".into(), checkout_action(item, ctx));
    }
    act(
        ui,
        format!("New Branch from '{name}'…"),
        Action::Dialog(Dialog::NewBranch {
            start: name.clone(),
            start_label: name.clone(),
            name: String::new(),
            checkout: true,
        }),
    );
    if !item.current && ctx.current.is_some() {
        ui.separator();
        act(
            ui,
            format!("Rebase '{current}' onto '{name}'"),
            Action::Dialog(
                dialogs::confirm(
                    "Rebase",
                    format!("Rebase '{current}' onto '{name}'?"),
                    Op::Rebase { onto: name.clone() },
                    false,
                )
                .expect("dialog"),
            ),
        );
        act(
            ui,
            format!("Merge '{name}' into '{current}'…"),
            Action::Dialog(Dialog::Merge {
                branch: name.clone(),
                no_ff: false,
                squash: false,
            }),
        );
        act(
            ui,
            format!("Reset '{current}' to '{name}'…"),
            Action::Dialog(Dialog::Reset {
                target: name.clone(),
                label: name.clone(),
                mode: ResetMode::Mixed,
            }),
        );
    }
    act(
        ui,
        "Show Diff with Working Tree".into(),
        Action::WorktreeDiff(if item.kind == RefKind::Tag { name.clone() } else { item.target.clone() }),
    );
    ui.separator();
    match item.kind {
        RefKind::LocalBranch => {
            if item.current {
                act(ui, "Pull (merge)".into(), Action::Op(Op::Pull(git_core::ops::PullMode::Merge)));
                act(ui, "Pull (rebase)".into(), Action::Op(Op::Pull(git_core::ops::PullMode::Rebase)));
            }
            if let Some(up) = &item.upstream {
                if let Some(remote) = ctx.remotes.iter().filter(|r| up.starts_with(&format!("{r}/"))).max_by_key(|r| r.len()) {
                    act(
                        ui,
                        "Update".into(),
                        Action::Op(Op::UpdateBranch {
                            branch: name.clone(),
                            remote: remote.clone(),
                            remote_branch: up[remote.len() + 1..].to_string(),
                            current: item.current,
                        }),
                    );
                }
            }
            if let Some(d) = dialogs::push_dialog_for(name, item.upstream.as_deref(), item.ahead, ctx.remotes) {
                act(ui, "Push…".into(), Action::Dialog(d));
            }
            let candidates: Vec<String> = ctx.remote_names.to_vec();
            if !candidates.is_empty() {
                act(
                    ui,
                    "Set Upstream…".into(),
                    Action::Dialog(Dialog::SetUpstream {
                        branch: name.clone(),
                        upstream: item.upstream.clone().unwrap_or_else(|| {
                            candidates
                                .iter()
                                .find(|c| c.ends_with(&format!("/{name}")))
                                .cloned()
                                .unwrap_or_default()
                        }),
                        candidates,
                    }),
                );
            }
            if item.upstream.is_some() {
                act(ui, "Unset Upstream".into(), Action::Op(Op::UnsetUpstream { branch: name.clone() }));
            }
            act(
                ui,
                "Rename…".into(),
                Action::Dialog(Dialog::RenameBranch {
                    old: name.clone(),
                    name: name.clone(),
                }),
            );
            if !item.current {
                act(
                    ui,
                    "Delete".into(),
                    Action::Dialog(Dialog::Confirm {
                        title: "Delete Branch".into(),
                        text: format!("Delete local branch '{name}'? Unmerged commits are refused unless you force-delete."),
                        op: Op::DeleteBranch { name: name.clone(), force: false },
                        danger: true,
                        ok: "OK".into(),
                    }),
                );
                act(
                    ui,
                    "Force Delete".into(),
                    Action::Dialog(Dialog::Confirm {
                        title: "Force Delete Branch".into(),
                        text: format!("Force-delete '{name}' even if it has unmerged commits? This cannot be undone from TailGIT."),
                        op: Op::DeleteBranch { name: name.clone(), force: true },
                        danger: true,
                        ok: "OK".into(),
                    }),
                );
            }
        }
        RefKind::RemoteBranch => {
            if let Some(remote) = &item.remote {
                act(
                    ui,
                    "Delete Remote Branch".into(),
                    Action::Dialog(Dialog::Confirm {
                        title: "Delete Remote Branch".into(),
                        text: format!("Delete '{}' on remote '{remote}'? This affects everyone using that remote.", item.path),
                        op: Op::DeleteRemoteBranch {
                            remote: remote.clone(),
                            branch: item.path.clone(),
                        },
                        danger: true,
                        ok: "OK".into(),
                    }),
                );
            }
        }
        RefKind::Tag => {
            for r in ctx.remotes {
                act(
                    ui,
                    format!("Push Tag to '{r}'"),
                    Action::Op(Op::PushTag {
                        remote: r.clone(),
                        tag: name.clone(),
                    }),
                );
            }
            act(
                ui,
                "Delete Tag".into(),
                Action::Dialog(Dialog::Confirm {
                    title: "Delete Tag".into(),
                    text: format!("Delete local tag '{name}'?"),
                    op: Op::DeleteTag(name.clone()),
                    danger: true,
                        ok: "OK".into(),
                }),
            );
        }
        RefKind::Head => {}
    }
    ui.separator();
    act(
        ui,
        if item.favorite { "Remove from Favorites" } else { "Add to Favorites" }.into(),
        Action::ToggleFavorite(name.clone()),
    );
    act(ui, "Copy Name".into(), Action::Copy(name.clone()));
    act(ui, "Copy Commit Hash".into(), Action::Copy(item.target.clone()));
}
