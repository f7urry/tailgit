//! Write operations the UI can request. Each runs on a worker thread.

use git_core::ops::{self, PullMode, PushOptions, ResetMode, TodoItem};
use git_core::partial::{self, LineKey, PartialKind};
use git_core::{conflicts, stash, Git, Result};

#[derive(Debug, Clone)]
pub enum Op {
    Checkout(String),
    CheckoutDetached(String),
    CheckoutTracking { local: String, remote: String },
    CheckoutNewBranch { name: String, start: String },
    CreateBranch { name: String, start: String },
    DeleteBranch { name: String, force: bool },
    DeleteRemoteBranch { remote: String, branch: String },
    RenameBranch { old: String, new: String },
    SetUpstream { branch: String, upstream: String },
    UnsetUpstream { branch: String },
    CreateTag { name: String, target: String, message: String },
    DeleteTag(String),
    PushTag { remote: String, tag: String },
    Merge { branch: String, no_ff: bool, squash: bool },
    MergeContinue,
    MergeAbort,
    Rebase { onto: String },
    DropCommits(Vec<String>),
    UndoDrop { old_head: String, new_head: String },
    /// Reset HEAD to its parent; the commit's changes return to the Changes tab.
    UndoCommit(String),
    EditMessage { hash: String, message: String },
    /// Rewrite `from` and every later commit following `items` (new order, oldest first).
    InteractiveRebase { from: String, items: Vec<TodoItem> },
    RebaseContinue,
    RebaseSkip,
    RebaseAbort,
    CherryPick(Vec<String>),
    CherryPickContinue,
    CherryPickSkip,
    CherryPickAbort,
    Revert(Vec<String>),
    RevertContinue,
    RevertAbort,
    Reset { target: String, mode: ResetMode },
    Fetch,
    Pull(PullMode),
    Push(PushOptions),
    UpdateBranch { branch: String, remote: String, remote_branch: String, current: bool },
    Stage(Vec<String>),
    StageAll,
    Unstage(Vec<String>),
    Rollback(Vec<String>),
    DeleteUntracked(Vec<String>),
    Commit { message: String, amend: bool, push_after: bool },
    StashPush { message: String, include_untracked: bool, keep_index: bool },
    StashApply { selector: String, index: bool },
    StashPop { selector: String, index: bool },
    StashDrop(String),
    StashBranch { name: String, selector: String },
    AcceptSide { paths: Vec<String>, yours: bool },
    WriteResolved { path: String, content: Vec<u8> },
    /// Save the partial merge result without marking the file resolved.
    SaveResult { path: String, content: Vec<u8> },
    /// Resolve every conflict that can be resolved automatically.
    ResolveSimple { paths: Vec<String> },
    /// Stage / unstage / roll back selected lines. `old`/`new` are the contents the
    /// selection was made on, so a stale selection is refused.
    PartialLines {
        kind: PartialKind,
        path: String,
        keys: Vec<LineKey>,
        old: Option<Vec<u8>>,
        new: Option<Vec<u8>>,
    },
}

fn short(s: &str) -> &str {
    if s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        &s[..8]
    } else {
        s
    }
}

impl Op {
    pub fn title(&self) -> String {
        match self {
            Op::Checkout(r) => format!("Checkout {}", short(r)),
            Op::CheckoutDetached(r) => format!("Checkout revision {}", short(r)),
            Op::CheckoutTracking { local, remote } => format!("Checkout {remote} as {local}"),
            Op::CheckoutNewBranch { name, .. } => format!("Create and checkout {name}"),
            Op::CreateBranch { name, .. } => format!("Create branch {name}"),
            Op::DeleteBranch { name, .. } => format!("Delete branch {name}"),
            Op::DeleteRemoteBranch { remote, branch } => format!("Delete {remote}/{branch}"),
            Op::RenameBranch { old, new } => format!("Rename {old} to {new}"),
            Op::SetUpstream { branch, upstream } => format!("Track {upstream} from {branch}"),
            Op::UnsetUpstream { branch } => format!("Unset upstream of {branch}"),
            Op::CreateTag { name, .. } => format!("Create tag {name}"),
            Op::DeleteTag(n) => format!("Delete tag {n}"),
            Op::PushTag { tag, .. } => format!("Push tag {tag}"),
            Op::Merge { branch, .. } => format!("Merge {}", short(branch)),
            Op::MergeContinue => "Conclude merge".into(),
            Op::MergeAbort => "Abort merge".into(),
            Op::Rebase { onto } => format!("Rebase onto {}", short(onto)),
            Op::DropCommits(h) if h.len() == 1 => format!("Drop commit {}", short(&h[0])),
            Op::DropCommits(h) => format!("Drop {} commits", h.len()),
            Op::UndoDrop { .. } => "Undo drop".into(),
            Op::UndoCommit(h) => format!("Undo commit {}", short(h)),
            Op::EditMessage { hash, .. } => format!("Edit message of {}", short(hash)),
            Op::InteractiveRebase { from, .. } => format!("Interactive rebase from {}", short(from)),
            Op::RebaseContinue => "Continue rebase".into(),
            Op::RebaseSkip => "Skip commit".into(),
            Op::RebaseAbort => "Abort rebase".into(),
            Op::CherryPick(h) => format!("Cherry-pick {} commit(s)", h.len()),
            Op::CherryPickContinue => "Continue cherry-pick".into(),
            Op::CherryPickSkip => "Skip cherry-pick".into(),
            Op::CherryPickAbort => "Abort cherry-pick".into(),
            Op::Revert(h) => format!("Revert {} commit(s)", h.len()),
            Op::RevertContinue => "Continue revert".into(),
            Op::RevertAbort => "Abort revert".into(),
            Op::Reset { target, mode } => format!("Reset ({mode:?}) to {}", short(target)),
            Op::Fetch => "Fetch".into(),
            Op::Pull(m) => format!("Pull ({m:?})"),
            Op::UpdateBranch { branch, .. } => format!("Update {branch}"),
            Op::Push(o) => format!("Push {} to {}/{}", o.local_branch, o.remote, o.remote_branch),
            Op::Stage(p) => format!("Stage {} file(s)", p.len()),
            Op::StageAll => "Stage all".into(),
            Op::Unstage(p) => format!("Unstage {} file(s)", p.len()),
            Op::Rollback(p) => format!("Rollback {} file(s)", p.len()),
            Op::DeleteUntracked(p) => format!("Delete {} file(s)", p.len()),
            Op::Commit { amend, .. } => if *amend { "Amend commit" } else { "Commit" }.into(),
            Op::StashPush { .. } => "Stash changes".into(),
            Op::StashApply { selector, .. } => format!("Apply {selector}"),
            Op::StashPop { selector, .. } => format!("Pop {selector}"),
            Op::StashDrop(s) => format!("Drop {s}"),
            Op::StashBranch { name, .. } => format!("Unstash as branch {name}"),
            Op::AcceptSide { yours, .. } => {
                if *yours { "Accept yours" } else { "Accept theirs" }.into()
            }
            Op::WriteResolved { path, .. } => format!("Resolve {path}"),
            Op::SaveResult { path, .. } => format!("Save {path}"),
            Op::ResolveSimple { .. } => "Resolve simple conflicts".into(),
            Op::PartialLines { kind, path, keys, .. } => {
                let verb = match kind {
                    PartialKind::Stage => "Stage",
                    PartialKind::Unstage => "Unstage",
                    PartialKind::Rollback => "Roll back",
                };
                format!("{verb} {} line(s) of {path}", keys.len())
            }
        }
    }

    pub fn run(self, git: &Git, progress: &mut dyn FnMut(&str)) -> Result<String> {
        match self {
            Op::Checkout(r) => ops::checkout(git, &r),
            Op::CheckoutDetached(r) => ops::checkout_detached(git, &r),
            Op::CheckoutTracking { local, remote } => ops::checkout_tracking(git, &local, &remote),
            Op::CheckoutNewBranch { name, start } => ops::checkout_new_branch(git, &name, &start),
            Op::CreateBranch { name, start } => ops::create_branch(git, &name, &start),
            Op::DeleteBranch { name, force } => ops::delete_branch(git, &name, force),
            Op::DeleteRemoteBranch { remote, branch } => ops::delete_remote_branch(git, &remote, &branch),
            Op::RenameBranch { old, new } => ops::rename_branch(git, &old, &new),
            Op::SetUpstream { branch, upstream } => ops::set_upstream(git, &branch, &upstream),
            Op::UnsetUpstream { branch } => ops::unset_upstream(git, &branch),
            Op::CreateTag { name, target, message } => {
                ops::create_tag(git, &name, &target, Some(message.as_str()))
            }
            Op::DeleteTag(n) => ops::delete_tag(git, &n),
            Op::PushTag { remote, tag } => ops::push_tag(git, &remote, &tag),
            Op::Merge { branch, no_ff, squash } => ops::merge(git, &branch, no_ff, squash),
            Op::MergeContinue => ops::merge_continue(git),
            Op::MergeAbort => ops::merge_abort(git),
            Op::Rebase { onto } => ops::rebase(git, &onto),
            Op::DropCommits(h) => ops::drop_commits(git, &h),
            Op::UndoDrop { old_head, new_head } => ops::undo_drop(git, &old_head, &new_head),
            Op::UndoCommit(h) => ops::undo_commit(git, &h),
            Op::EditMessage { hash, message } => ops::edit_commit_message(git, &hash, &message),
            Op::InteractiveRebase { from, items } => ops::interactive_rebase(git, &from, &items),
            Op::RebaseContinue => ops::rebase_continue(git),
            Op::RebaseSkip => ops::rebase_skip(git),
            Op::RebaseAbort => ops::rebase_abort(git),
            Op::CherryPick(h) => ops::cherry_pick(git, &h, true),
            Op::CherryPickContinue => ops::cherry_pick_continue(git),
            Op::CherryPickSkip => ops::cherry_pick_skip(git),
            Op::CherryPickAbort => ops::cherry_pick_abort(git),
            Op::Revert(h) => ops::revert(git, &h),
            Op::RevertContinue => ops::revert_continue(git),
            Op::RevertAbort => ops::revert_abort(git),
            Op::Reset { target, mode } => ops::reset(git, &target, mode),
            Op::Fetch => ops::fetch(git, None, progress),
            Op::Pull(mode) => ops::pull(git, mode, progress),
            Op::Push(o) => ops::push(git, &o, progress),
            Op::UpdateBranch { branch, remote, remote_branch, current } => {
                ops::update_branch(git, &branch, &remote, &remote_branch, current, progress)
            }
            Op::Stage(p) => ops::stage(git, &p),
            Op::StageAll => ops::stage_all(git),
            Op::Unstage(p) => ops::unstage(git, &p),
            Op::Rollback(p) => ops::rollback(git, &p),
            Op::DeleteUntracked(p) => ops::delete_untracked(git, &p),
            Op::Commit { message, amend, .. } => ops::commit(git, &message, amend, false),
            Op::StashPush { message, include_untracked, keep_index } => {
                stash::push(git, &message, include_untracked, keep_index)
            }
            Op::StashApply { selector, index } => stash::apply(git, &selector, index),
            Op::StashPop { selector, index } => stash::pop(git, &selector, index),
            Op::StashDrop(s) => stash::drop(git, &s),
            Op::StashBranch { name, selector } => stash::branch(git, &name, &selector),
            Op::AcceptSide { paths, yours } => conflicts::accept_side(git, &paths, yours),
            Op::WriteResolved { path, content } => conflicts::write_resolved(git, &path, &content),
            Op::SaveResult { path, content } => conflicts::write_result(git, &path, &content),
            Op::ResolveSimple { paths } => conflicts::resolve_simple(git, &paths),
            Op::PartialLines { kind, path, keys, old, new } => {
                partial::apply(git, kind, &path, &keys, old.as_deref(), new.as_deref())
            }
        }
    }
}
