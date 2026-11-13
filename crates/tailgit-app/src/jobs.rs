//! Background work: every git call runs off the UI thread and reports back as a [`Msg`].

use std::collections::HashMap;

use crossbeam_channel::Sender;
use eframe::egui;
use git_core::conflicts::{ConflictFile, UnmergedPath};
use git_core::merge_context::MergeContext;
use git_core::content::{self, Source, TextDiffResult};
use git_core::log::{Commit, CommitDetails, FileChange, LogFilter};
use git_core::refs::{self, RefLabel, Refs};
use git_core::repo_state::{self, RepoState};
use git_core::stash::Stash;
use git_core::status::StatusEntry;
use git_core::Git;
use log_graph::Graph;

use crate::ops::Op;

pub struct LogData {
    pub refs: Refs,
    pub labels: HashMap<String, Vec<RefLabel>>,
    pub commits: Vec<Commit>,
    pub graph: Graph,
    pub index: HashMap<String, usize>,
    pub truncated: bool,
}

pub struct DiffData {
    pub old_label: String,
    pub new_label: String,
    pub diff: TextDiffResult,
    /// Raw contents the diff was computed from (used to validate partial staging).
    pub old_bytes: Option<Vec<u8>>,
    pub new_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRequest {
    pub title: String,
    pub old: Source,
    pub old_path: String,
    pub new: Source,
    pub new_path: String,
}

pub enum Msg {
    Log {
        generation: u64,
        result: Result<Box<LogData>, String>,
    },
    Details {
        hash: String,
        result: Result<CommitDetails, String>,
    },
    Containing {
        hash: String,
        result: Result<Vec<String>, String>,
    },
    Diff {
        request: DiffRequest,
        result: Result<DiffData, String>,
    },
    WorktreeDiff {
        rev: String,
        result: Result<Vec<FileChange>, String>,
    },
    Status(Result<(Vec<StatusEntry>, RepoState), String>),
    Stashes(Result<Vec<Stash>, String>),
    StashDetails {
        hash: String,
        result: Result<CommitDetails, String>,
    },
    LastMessage(String),
    Conflict {
        path: String,
        result: Result<(ConflictFile, MergeContext), String>,
    },
    /// Conflicts dialog contents: description and unmerged files.
    ConflictInfo(Result<(MergeContext, Vec<UnmergedPath>), String>),
    Progress(String),
    OpDone {
        op: Op,
        result: Result<String, String>,
    },
    FsChanged,
}

#[derive(Clone)]
pub struct Jobs {
    pub git: Git,
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Jobs {
    pub fn new(git: Git, tx: Sender<Msg>, ctx: egui::Context) -> Self {
        Jobs { git, tx, ctx }
    }

    fn spawn(&self, f: impl FnOnce(&Git, &Sender<Msg>) + Send + 'static) {
        let git = self.git.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            f(&git, &tx);
            ctx.request_repaint();
        });
    }

    pub fn load_log(&self, generation: u64, filter: LogFilter) {
        self.spawn(move |git, tx| {
            let result = build_log(git, &filter).map(Box::new).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Log { generation, result });
        });
    }

    pub fn load_details(&self, hash: String) {
        self.spawn(move |git, tx| {
            let result = git_core::log::read_details(git, &hash).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Details { hash, result });
        });
    }

    pub fn load_stash_details(&self, hash: String) {
        self.spawn(move |git, tx| {
            let result = git_core::log::read_details(git, &hash).map_err(|e| e.to_string());
            let _ = tx.send(Msg::StashDetails { hash, result });
        });
    }

    pub fn load_containing(&self, hash: String) {
        self.spawn(move |git, tx| {
            let result = git_core::log::containing_branches(git, &hash).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Containing { hash, result });
        });
    }

    pub fn load_diff(&self, request: DiffRequest) {
        self.spawn(move |git, tx| {
            let result = (|| -> git_core::Result<DiffData> {
                let old = content::read(git, &request.old, &request.old_path)?;
                let new = content::read(git, &request.new, &request.new_path)?;
                Ok(DiffData {
                    old_label: source_label(&request.old),
                    new_label: source_label(&request.new),
                    diff: content::diff(old.as_deref(), new.as_deref(), 3),
                    old_bytes: old,
                    new_bytes: new,
                })
            })()
            .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Diff { request, result });
        });
    }

    pub fn load_worktree_diff(&self, rev: String) {
        self.spawn(move |git, tx| {
            let result = git_core::log::read_worktree_changes(git, &rev).map_err(|e| e.to_string());
            let _ = tx.send(Msg::WorktreeDiff { rev, result });
        });
    }

    pub fn load_status(&self) {
        self.spawn(|git, tx| {
            let result = git_core::status::read_status(git)
                .map(|s| (s, repo_state::read_state(git)))
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Status(result));
        });
    }

    pub fn load_stashes(&self) {
        self.spawn(|git, tx| {
            let result = git_core::stash::list(git).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Stashes(result));
        });
    }

    pub fn load_last_message(&self) {
        self.spawn(|git, tx| {
            if let Ok(m) = git_core::ops::last_commit_message(git) {
                let _ = tx.send(Msg::LastMessage(m));
            }
        });
    }

    pub fn load_conflict(&self, path: String) {
        self.spawn(move |git, tx| {
            let result = git_core::conflicts::load(git, &path)
                .map(|f| {
                    let n = git_core::conflicts::list_unmerged(git).map(|u| u.len()).unwrap_or(1);
                    (f, git_core::merge_context::read(git, n))
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Conflict { path, result });
        });
    }

    pub fn load_conflict_info(&self) {
        self.spawn(|git, tx| {
            let result = git_core::conflicts::list_unmerged(git)
                .map(|files| (git_core::merge_context::read(git, files.len()), files))
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::ConflictInfo(result));
        });
    }

    pub fn run_op(&self, op: Op) {
        self.spawn(move |git, tx| {
            let progress_tx = tx.clone();
            let mut progress = |line: &str| {
                let _ = progress_tx.send(Msg::Progress(line.to_string()));
            };
            let result = op.clone().run(git, &mut progress).map_err(|e| e.output());
            let _ = tx.send(Msg::OpDone { op, result });
        });
    }
}

fn source_label(s: &Source) -> String {
    match s {
        Source::Empty => "(none)".into(),
        Source::Revision(r) if r.len() == 40 => r[..8].to_string(),
        Source::Revision(r) => r.clone(),
        Source::Index => "Staged".into(),
        Source::WorkTree => "Working tree".into(),
    }
}

fn build_log(git: &Git, filter: &LogFilter) -> git_core::Result<LogData> {
    let refs = refs::read_refs(git)?;
    let labels = refs.labels_by_commit();
    let commits = git_core::log::read_log(git, filter)?;
    let truncated = filter.max_count.is_some_and(|m| commits.len() >= m);
    let index: HashMap<String, usize> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.hash.clone(), i))
        .collect();
    let parents = commits
        .iter()
        .map(|c| c.parents.iter().map(|p| index.get(p).copied()).collect())
        .collect();
    // Heads in branch-layout priority order (GitRefManager.GitBranchLayoutComparator).
    let mut heads: Vec<(u8, String, usize)> = commits
        .iter()
        .enumerate()
        .filter_map(|(row, c)| {
            let best = labels
                .get(&c.hash)?
                .iter()
                .min_by_key(|l| (refs::layout_priority(l), l.name.clone()))?;
            Some((refs::layout_priority(best), best.name.clone(), row))
        })
        .collect();
    heads.sort();
    let heads: Vec<(usize, String)> = heads.into_iter().map(|(_, n, r)| (r, n)).collect();
    let graph = Graph::build(parents, &heads);
    Ok(LogData {
        refs,
        labels,
        commits,
        graph,
        index,
        truncated,
    })
}

/// Watches the `.git` directory and reports changes (debounced by the UI).
pub fn watch(git_dir: &std::path::Path, tx: Sender<Msg>, ctx: egui::Context) -> Option<notify::RecommendedWatcher> {
    use notify::Watcher;
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        let relevant = event.paths.iter().any(|p| {
            let s = p.to_string_lossy();
            !s.contains("/objects/") && !s.ends_with(".lock") && !s.contains("/logs/")
        });
        if relevant {
            let _ = tx.send(Msg::FsChanged);
            ctx.request_repaint();
        }
    })
    .ok()?;
    watcher
        .watch(git_dir, notify::RecursiveMode::Recursive)
        .ok()?;
    Some(watcher)
}
