//! State of one open repository and the handling of background results.

use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver};
use eframe::egui;
use git_core::content::Source;
use git_core::log::{ChangeKind, CommitDetails, FileChange, LogFilter};
use git_core::repo_state::RepoState;
use git_core::stash::Stash;
use git_core::status::StatusEntry;
use git_core::Git;

use crate::jobs::{DiffData, DiffRequest, Jobs, LogData, Msg};
use crate::ops::Op;
use crate::ui::conflicts::{ConflictsDialog, MergeEditor};
use crate::ui::dialogs::Dialog;

pub const PAGE: usize = 50_000;

/// Containing branches, or the error loading them.
pub type BranchList = Result<Vec<String>, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Log,
    Commit,
    Stash,
}

/// Filter inputs as typed in the filter bar.
#[derive(Debug, Clone, Default)]
pub struct FilterInputs {
    pub text: String,
    pub regex: bool,
    pub match_case: bool,
    pub no_merges: bool,
    pub branch: Option<String>,
    pub user: String,
    pub after: String,
    pub before: String,
    pub paths: String,
}

impl FilterInputs {
    pub fn to_filter(&self, max_count: usize) -> LogFilter {
        let split = |s: &str| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        LogFilter {
            revisions: self.branch.iter().cloned().collect(),
            text: self.text.trim().to_string(),
            regex: self.regex,
            match_case: self.match_case,
            no_merges: self.no_merges,
            users: split(&self.user),
            after: self.after.clone(),
            before: self.before.clone(),
            paths: split(&self.paths),
            max_count: Some(max_count),
            topo_order: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && !self.no_merges
            && self.branch.is_none()
            && self.user.trim().is_empty()
            && self.after.trim().is_empty()
            && self.before.trim().is_empty()
            && self.paths.trim().is_empty()
    }
}

pub struct LogState {
    pub data: Option<Box<LogData>>,
    pub loading: bool,
    pub generation: u64,
    pub error: Option<String>,
    pub inputs: FilterInputs,
    pub max_count: usize,
    pub selected: Option<usize>,
    pub selected_hash: Option<String>,
    /// Every selected commit (multi-select with Shift / Cmd / Ctrl), including
    /// `selected_hash`, which is the focused one shown in the details pane.
    pub multi: std::collections::HashSet<String>,
    /// Row a Shift range is extended from.
    pub anchor: Option<String>,
    pub scroll_to: Option<usize>,
    pub visible: std::ops::Range<usize>,
}

pub struct DiffView {
    pub request: DiffRequest,
    pub data: Option<Result<DiffData, String>>,
    /// Lines selected for partial stage / unstage / rollback.
    pub selected: std::collections::BTreeSet<git_core::partial::LineKey>,
}

pub struct OutputEntry {
    pub title: String,
    pub ok: bool,
    pub text: String,
    pub time: chrono::DateTime<chrono::Local>,
}

/// Heads needed to undo the last drop.
#[derive(Clone)]
pub struct DropUndo {
    pub old_head: String,
    pub new_head: String,
}

pub struct RepoView {
    pub jobs: Jobs,
    rx: Receiver<Msg>,
    _watcher: Option<notify::RecommendedWatcher>,
    pub root: String,
    pub name: String,
    pub log: LogState,
    pub details: Option<Result<CommitDetails, String>>,
    pub containing: Option<(String, Option<BranchList>)>,
    pub diff: Option<DiffView>,
    pub status: Vec<StatusEntry>,
    pub state: RepoState,
    pub status_error: Option<String>,
    pub stashes: Vec<Stash>,
    pub stash_selected: Option<String>,
    pub stash_details: Option<Result<CommitDetails, String>>,
    pub commit_message: String,
    pub amend: bool,
    pub tab: Tab,
    pub dialog: Option<Dialog>,
    pub conflicts_dialog: Option<ConflictsDialog>,
    /// Mirrors the "Automatically apply non-conflicting changes" setting.
    pub auto_apply_non_conflicting: bool,
    /// Settings > Sort commits: topologically (otherwise by date).
    pub topo_order: bool,
    pub merge_editor: Option<MergeEditor>,
    pub busy: Option<String>,
    pub progress: String,
    pub output: Vec<OutputEntry>,
    pub show_output: bool,
    pub notice: Option<(bool, String, Instant)>,
    /// Set after a successful "Drop Commit"; enables the status bar Undo button.
    pub last_drop: Option<DropUndo>,
    pub branch_search: String,
    pub commit_selection: Option<(bool, String)>,
    fs_dirty: Option<Instant>,
    open_conflicts_on_status: bool,
}

impl RepoView {
    pub fn new(git: Git, ctx: &egui::Context) -> RepoView {
        let (tx, rx) = unbounded();
        let watcher = crate::jobs::watch(git.git_dir(), tx.clone(), ctx.clone());
        let root = git.root().display().to_string();
        let name = git
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone());
        let mut view = RepoView {
            jobs: Jobs::new(git, tx, ctx.clone()),
            rx,
            _watcher: watcher,
            root,
            name,
            log: LogState {
                data: None,
                loading: false,
                generation: 0,
                error: None,
                inputs: FilterInputs::default(),
                max_count: PAGE,
                selected: None,
                selected_hash: None,
                multi: Default::default(),
                anchor: None,
                scroll_to: None,
                visible: 0..0,
            },
            details: None,
            containing: None,
            diff: None,
            status: Vec::new(),
            state: RepoState::Normal,
            status_error: None,
            stashes: Vec::new(),
            stash_selected: None,
            stash_details: None,
            commit_message: String::new(),
            amend: false,
            tab: Tab::Log,
            dialog: None,
            conflicts_dialog: None,
            auto_apply_non_conflicting: false,
            topo_order: false,
            merge_editor: None,
            busy: None,
            progress: String::new(),
            output: Vec::new(),
            show_output: false,
            notice: None,
            last_drop: None,
            branch_search: String::new(),
            commit_selection: None,
            fs_dirty: None,
            open_conflicts_on_status: false,
        };
        view.refresh_all();
        view
    }

    pub fn refresh_all(&mut self) {
        self.reload_log();
        self.jobs.load_status();
        self.jobs.load_stashes();
    }

    pub fn reload_log(&mut self) {
        self.log.generation += 1;
        self.log.loading = true;
        let mut filter = self.log.inputs.to_filter(self.log.max_count);
        filter.topo_order = self.topo_order;
        self.jobs.load_log(self.log.generation, filter);
    }

    pub fn apply_filter(&mut self) {
        self.log.max_count = PAGE;
        self.reload_log();
    }

    pub fn load_more(&mut self) {
        self.log.max_count += PAGE;
        self.reload_log();
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &StatusEntry> {
        self.status.iter().filter(|e| e.is_conflicted())
    }

    pub fn conflict_count(&self) -> usize {
        self.conflicts().count()
    }

    pub fn change_count(&self) -> usize {
        self.status.iter().filter(|e| !e.is_ignored()).count()
    }

    pub fn run(&mut self, op: Op) {
        if self.busy.is_some() {
            self.notify(false, "Another operation is running".into());
            return;
        }
        self.busy = Some(op.title());
        self.progress.clear();
        self.jobs.run_op(op);
    }

    pub fn notify(&mut self, ok: bool, text: String) {
        self.notice = Some((ok, text, Instant::now()));
    }

    /// Plain selection: `row` becomes the only selected commit.
    pub fn select_row(&mut self, row: usize) {
        let Some(c) = self.log.data.as_ref().and_then(|d| d.commits.get(row)) else { return };
        let hash = c.hash.clone();
        self.log.multi.clear();
        self.log.multi.insert(hash.clone());
        self.log.anchor = Some(hash);
        self.focus_row(row);
    }

    /// Cmd/Ctrl-click: toggles `row` in the selection.
    pub fn toggle_row(&mut self, row: usize) {
        let Some(c) = self.log.data.as_ref().and_then(|d| d.commits.get(row)) else { return };
        let hash = c.hash.clone();
        self.log.anchor = Some(hash.clone());
        if self.log.multi.remove(&hash) {
            if self.log.multi.is_empty() {
                self.log.multi.insert(hash);
            } else if self.log.selected_hash.as_deref() == Some(hash.as_str()) {
                // Move focus to the nearest remaining selected row.
                if let Some(next) = self.selected_rows().into_iter().min_by_key(|r| r.abs_diff(row)) {
                    self.focus_row(next);
                }
            }
        } else {
            self.log.multi.insert(hash);
            self.focus_row(row);
        }
    }

    /// Shift-click / Shift-arrow: selects the range from the anchor to `row`.
    pub fn extend_to_row(&mut self, row: usize) {
        let Some(data) = self.log.data.as_ref() else { return };
        if row >= data.commits.len() {
            return;
        }
        let anchor = self
            .log
            .anchor
            .as_ref()
            .and_then(|h| data.index.get(h).copied())
            .or(self.log.selected)
            .unwrap_or(row);
        self.log.multi = data.commits[anchor.min(row)..=anchor.max(row)].iter().map(|c| c.hash.clone()).collect();
        if self.log.anchor.is_none() {
            self.log.anchor = Some(data.commits[anchor].hash.clone());
        }
        self.focus_row(row);
    }

    pub fn select_all_rows(&mut self) {
        let Some(data) = self.log.data.as_ref() else { return };
        self.log.multi = data.commits.iter().map(|c| c.hash.clone()).collect();
        if self.log.selected.is_none() && !data.commits.is_empty() {
            self.focus_row(0);
        }
    }

    /// Selected rows in log order (newest first).
    pub fn selected_rows(&self) -> Vec<usize> {
        let Some(data) = self.log.data.as_ref() else { return Vec::new() };
        let mut rows: Vec<usize> = self.log.multi.iter().filter_map(|h| data.index.get(h).copied()).collect();
        rows.sort_unstable();
        rows
    }

    /// Makes `row` the focused commit (details pane) without touching the selection set.
    fn focus_row(&mut self, row: usize) {
        let Some(data) = &self.log.data else { return };
        let Some(c) = data.commits.get(row) else { return };
        if self.log.selected_hash.as_deref() == Some(c.hash.as_str()) {
            self.log.selected = Some(row);
            return;
        }
        self.log.selected = Some(row);
        self.log.selected_hash = Some(c.hash.clone());
        self.details = None;
        self.containing = None;
        self.jobs.load_details(c.hash.clone());
    }

    pub fn select_hash(&mut self, hash: &str) {
        let row = self
            .log
            .data
            .as_ref()
            .and_then(|d| d.index.get(hash).copied());
        if let Some(row) = row {
            self.select_row(row);
            self.log.scroll_to = Some(row);
        } else {
            self.notify(false, format!("Commit {} is not in the loaded log", &hash[..hash.len().min(8)]));
        }
    }

    pub fn request_containing(&mut self, hash: String) {
        self.jobs.load_containing(hash.clone());
        self.containing = Some((hash, None));
    }

    pub fn open_diff(&mut self, request: DiffRequest) {
        if self.diff.as_ref().is_some_and(|d| d.request == request && d.data.is_some()) {
            return;
        }
        self.jobs.load_diff(request.clone());
        self.diff = Some(DiffView {
            request,
            data: None,
            selected: Default::default(),
        });
    }

    /// Diff of a file changed by a commit, against its first parent.
    pub fn open_commit_file_diff(&mut self, details: &CommitDetails, change: &FileChange) {
        let parent = details.parents.first().cloned();
        let old = match (change.kind, &parent) {
            (ChangeKind::Added, _) | (_, None) => Source::Empty,
            (_, Some(p)) => Source::Revision(p.clone()),
        };
        let new = if change.kind == ChangeKind::Deleted {
            Source::Empty
        } else {
            Source::Revision(details.hash.clone())
        };
        self.open_diff(DiffRequest {
            title: change.path.clone(),
            old,
            old_path: change.old_path.clone().unwrap_or_else(|| change.path.clone()),
            new,
            new_path: change.path.clone(),
        });
    }

    /// Diff of a working tree entry. `staged` selects HEAD↔index, otherwise index↔work tree.
    pub fn open_status_diff(&mut self, entry: &StatusEntry, staged: bool) {
        self.commit_selection = Some((staged, entry.path.clone()));
        let (old, new) = if entry.is_untracked() {
            (Source::Empty, Source::WorkTree)
        } else if entry.is_conflicted() {
            (Source::Revision("HEAD".into()), Source::WorkTree)
        } else if staged {
            let old = if entry.index == 'A' {
                Source::Empty
            } else {
                Source::Revision("HEAD".into())
            };
            let new = if entry.index == 'D' { Source::Empty } else { Source::Index };
            (old, new)
        } else {
            let new = if entry.worktree == 'D' {
                Source::Empty
            } else {
                Source::WorkTree
            };
            (Source::Index, new)
        };
        let old_path = if staged {
            entry.orig_path.clone().unwrap_or_else(|| entry.path.clone())
        } else {
            entry.path.clone()
        };
        self.open_diff(DiffRequest {
            title: entry.path.clone(),
            old,
            old_path,
            new,
            new_path: entry.path.clone(),
        });
    }

    /// Opens the Conflicts dialog (IntelliJ's `MultipleFileMergeDialog`).
    pub fn open_conflicts(&mut self) {
        if self.conflicts_dialog.is_none() {
            self.conflicts_dialog = Some(ConflictsDialog::default());
        }
        self.jobs.load_conflict_info();
    }

    /// Opens the merge window for the first queued file of the Conflicts dialog.
    pub fn merge_next_queued(&mut self) {
        if let Some(d) = &mut self.conflicts_dialog {
            if let Some(p) = d.queue.pop_front() {
                self.merge_editor = None;
                self.jobs.load_conflict(p);
            }
        }
    }

    /// `GitConflictResolver.proceedAfterAllMerged`: what follows once nothing is left unmerged.
    fn proceed_after_all_merged(&mut self, state: RepoState) {
        match state {
            RepoState::Rebasing => {
                self.dialog = crate::ui::dialogs::confirm_with(
                    "Resolve Conflicts",
                    "All conflicts have been resolved. Do you want to continue rebase?",
                    Op::RebaseContinue,
                    false,
                    "Continue Rebase",
                );
            }
            RepoState::CherryPicking | RepoState::Reverting => {
                // Like IntelliJ, open the commit dialog with the original message.
                let msg = std::fs::read_to_string(self.jobs.git.git_dir().join("MERGE_MSG")).unwrap_or_default();
                let msg: Vec<&str> = msg.lines().filter(|l| !l.starts_with('#')).collect();
                self.commit_message = msg.join("\n").trim().to_string();
                self.tab = Tab::Commit;
                self.notify(true, "All conflicts resolved: commit to finish".into());
            }
            RepoState::Merging => self.notify(true, "All conflicts resolved".into()),
            RepoState::Normal => {}
        }
    }

    /// Processes finished background work. Returns true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            self.handle(msg);
        }
        if let Some(t) = self.fs_dirty {
            if t.elapsed() > Duration::from_millis(400) && self.busy.is_none() {
                self.fs_dirty = None;
                self.refresh_all();
            }
        }
        if let Some((_, _, t)) = &self.notice {
            if t.elapsed() > Duration::from_secs(8) {
                self.notice = None;
            }
        }
        changed
    }

    pub fn needs_repaint_soon(&self) -> bool {
        self.fs_dirty.is_some() || self.notice.is_some()
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Log { generation, result } => {
                if generation != self.log.generation {
                    return;
                }
                self.log.loading = false;
                match result {
                    Ok(data) => {
                        self.log.error = None;
                        let selected = self
                            .log
                            .selected_hash
                            .as_ref()
                            .and_then(|h| data.index.get(h).copied());
                        self.log.selected = selected;
                        self.log.multi.retain(|h| data.index.contains_key(h));
                        if selected.is_none() {
                            self.log.selected_hash = None;
                            self.details = None;
                            self.log.multi.clear();
                        }
                        self.log.data = Some(data);
                        if self.log.selected.is_none() && !self.log.inputs.is_empty() {
                            if let Some(d) = &self.log.data {
                                if !d.commits.is_empty() {
                                    self.select_row(0);
                                }
                            }
                        }
                    }
                    Err(e) => self.log.error = Some(e),
                }
            }
            Msg::Details { hash, result } => {
                if self.log.selected_hash.as_deref() == Some(hash.as_str()) {
                    self.details = Some(result);
                }
            }
            Msg::Containing { hash, result } => {
                if self.log.selected_hash.as_deref() == Some(hash.as_str()) {
                    self.containing = Some((hash, Some(result)));
                }
            }
            Msg::WorktreeDiff { rev, result } => {
                if let Some(Dialog::WorktreeDiff { rev: r, files }) = &mut self.dialog {
                    if *r == rev {
                        *files = Some(result);
                    }
                }
            }
            Msg::Diff { request, result } => {
                if let Some(d) = &mut self.diff {
                    if d.request == request {
                        d.data = Some(result);
                    }
                }
            }
            Msg::Status(result) => match result {
                Ok((status, state)) => {
                    self.status = status;
                    self.state = state;
                    self.status_error = None;
                    if self.open_conflicts_on_status {
                        self.open_conflicts_on_status = false;
                        // A rebase reports its stop through the banner instead (GitRebaseProcess).
                        if self.conflict_count() > 0 && self.state != RepoState::Rebasing {
                            self.open_conflicts();
                        }
                    }
                    if self.conflicts_dialog.is_some() {
                        self.jobs.load_conflict_info();
                    }
                    if self.conflict_count() == 0 {
                        self.merge_editor = None;
                    }
                }
                Err(e) => self.status_error = Some(e),
            },
            Msg::Stashes(result) => {
                if let Ok(s) = result {
                    if let Some(sel) = &self.stash_selected {
                        if !s.iter().any(|x| &x.hash == sel) {
                            self.stash_selected = None;
                            self.stash_details = None;
                        }
                    }
                    self.stashes = s;
                }
            }
            Msg::StashDetails { hash, result } => {
                if self.stash_selected.as_deref() == Some(hash.as_str()) {
                    self.stash_details = Some(result);
                }
            }
            Msg::LastMessage(m) => {
                if self.amend && self.commit_message.trim().is_empty() {
                    self.commit_message = m;
                }
            }
            Msg::ConflictInfo(result) => {
                let Some(d) = &mut self.conflicts_dialog else { return };
                let all_resolved = matches!(&result, Ok((_, files)) if files.is_empty());
                let state = result.as_ref().map(|(c, _)| c.state).unwrap_or(self.state);
                if let Ok((_, files)) = &result {
                    d.selected.retain(|p| files.iter().any(|f| &f.path == p));
                    if d.selected.is_empty() && !files.is_empty() {
                        let i = d.last_index.min(files.len() - 1);
                        d.selected.insert(files[i].path.clone());
                    }
                }
                d.info = Some(result);
                if all_resolved {
                    // The dialog closes itself once every file is resolved.
                    self.conflicts_dialog = None;
                    self.merge_editor = None;
                    self.proceed_after_all_merged(state);
                }
            }
            Msg::Conflict { path, result } => match result {
                Ok((file, context)) => {
                    self.merge_editor = Some(MergeEditor::new(file, context, self.auto_apply_non_conflicting));
                }
                Err(e) => {
                    self.notify(false, format!("Error loading revisions to merge: {path}: {e}"));
                    self.merge_next_queued();
                }
            },
            Msg::Progress(line) => self.progress = line,
            Msg::OpDone { op, result } => {
                self.busy = None;
                self.progress.clear();
                let title = op.title();
                let ok = result.is_ok();
                let text = match &result {
                    Ok(s) | Err(s) => s.trim().to_string(),
                };
                self.output.push(OutputEntry {
                    title: title.clone(),
                    ok,
                    text: text.clone(),
                    time: chrono::Local::now(),
                });
                // Undo stays available only until the next operation finishes.
                let prev_drop = self.last_drop.take();
                if ok {
                    self.notify(true, format!("{title}: done"));
                    match op {
                        Op::DropCommits(ref h) => {
                            let git = &self.jobs.git;
                            let msg = if h.len() == 1 {
                                let subject = git.read(&["log", "-1", "--format=%s", &h[0]]).unwrap_or_default().trim().to_string();
                                format!("Dropped 1 commit \"{subject}\"")
                            } else {
                                format!("Dropped {} commits", h.len())
                            };
                            if let (Ok(old_head), Ok(new_head)) = (git_core::ops::rev_parse(git, "ORIG_HEAD"), git_core::ops::rev_parse(git, "HEAD")) {
                                self.notify(true, msg);
                                self.last_drop = Some(DropUndo { old_head, new_head });
                            }
                        }
                        Op::UndoDrop { .. } => self.notify(true, "Drop undone".into()),
                        Op::UndoCommit(ref h) => {
                            // Put the undone commit's message back in the commit box.
                            if self.commit_message.trim().is_empty() {
                                if let Ok(m) = self.jobs.git.read(&["log", "-1", "--format=%B", h]) {
                                    self.commit_message = m.trim().to_string();
                                }
                            }
                            self.amend = false;
                        }
                        Op::Commit { push_after, .. } => {
                            self.commit_message.clear();
                            self.amend = false;
                            if push_after {
                                self.dialog = crate::ui::dialogs::push_dialog(self);
                            }
                        }
                        Op::WriteResolved { .. } | Op::SaveResult { .. } | Op::AcceptSide { .. } => {
                            self.merge_editor = None;
                            self.merge_next_queued();
                        }
                        _ => {}
                    }
                } else {
                    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("failed");
                    if matches!(op, Op::UndoDrop { .. }) {
                        self.notify(false, format!("Undo drop failed: {first}"));
                    } else {
                        self.notify(false, format!("{title}: {first}"));
                        if matches!(op, Op::DropCommits(_)) {
                            self.last_drop = prev_drop;
                        }
                    }
                    self.open_conflicts_on_status = true;
                }
                if let Some(d) = &mut self.diff {
                    d.data = None;
                    d.selected.clear();
                    let req = d.request.clone();
                    self.jobs.load_diff(req);
                }
                self.refresh_all();
            }
            Msg::FsChanged => {
                self.fs_dirty = Some(Instant::now());
            }
        }
    }
}
