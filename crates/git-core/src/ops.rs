//! Write operations: branches, tags, history rewriting, staging, commits and sync.
//! Argument lists follow git4idea's `GitImpl`.

use crate::error::{GitError, Result};
use crate::refs::Head;
use crate::runner::Git;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
    Keep,
}

impl ResetMode {
    pub fn flag(self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
            ResetMode::Keep => "--keep",
        }
    }
}

fn with_paths<'a>(mut args: Vec<&'a str>, paths: &'a [String]) -> Vec<&'a str> {
    args.push("--");
    args.extend(paths.iter().map(|s| s.as_str()));
    args
}

fn validate_ref_name(git: &Git, name: &str) -> Result<()> {
    let out = git.exec(
        &["check-ref-format", "--branch", name],
        None,
        crate::runner::Access::Read,
    )?;
    if out.success() && !name.trim().is_empty() {
        Ok(())
    } else {
        Err(GitError::Invalid(format!("'{name}' is not a valid branch name")))
    }
}

// ---- branches & tags -------------------------------------------------------

pub fn checkout(git: &Git, reference: &str) -> Result<String> {
    git.write(&["checkout", reference, "--"])
}

/// Checks out a revision in detached HEAD mode (`<rev>^0`, like git4idea).
pub fn checkout_detached(git: &Git, revision: &str) -> Result<String> {
    let target = format!("{revision}^0");
    git.write(&["checkout", &target, "--"])
}

/// Checks out a remote branch by creating a tracking local branch.
pub fn checkout_tracking(git: &Git, local_name: &str, remote_branch: &str) -> Result<String> {
    validate_ref_name(git, local_name)?;
    git.write(&["checkout", "-b", local_name, "--track", remote_branch, "--"])
}

pub fn checkout_new_branch(git: &Git, name: &str, start: &str) -> Result<String> {
    validate_ref_name(git, name)?;
    git.write(&["checkout", "-b", name, start, "--"])
}

pub fn create_branch(git: &Git, name: &str, start: &str) -> Result<String> {
    validate_ref_name(git, name)?;
    git.write(&["branch", name, start])
}

pub fn delete_branch(git: &Git, name: &str, force: bool) -> Result<String> {
    git.write(&["branch", if force { "-D" } else { "-d" }, name])
}

pub fn delete_remote_branch(git: &Git, remote: &str, branch: &str) -> Result<String> {
    let spec = format!(":refs/heads/{branch}");
    git.write(&["push", "--porcelain", remote, &spec])
}

pub fn rename_branch(git: &Git, old: &str, new: &str) -> Result<String> {
    validate_ref_name(git, new)?;
    git.write(&["branch", "-m", old, new])
}

pub fn set_upstream(git: &Git, branch: &str, upstream: &str) -> Result<String> {
    git.write(&["branch", "--set-upstream-to", upstream, branch])
}

pub fn unset_upstream(git: &Git, branch: &str) -> Result<String> {
    git.write(&["branch", "--unset-upstream", branch])
}

pub fn create_tag(git: &Git, name: &str, target: &str, message: Option<&str>) -> Result<String> {
    match message.filter(|m| !m.trim().is_empty()) {
        Some(m) => git.write(&["tag", "-a", name, "-m", m, target]),
        None => git.write(&["tag", name, target]),
    }
}

pub fn delete_tag(git: &Git, name: &str) -> Result<String> {
    git.write(&["tag", "-d", name])
}

// ---- history ---------------------------------------------------------------

pub fn merge(git: &Git, branch: &str, no_ff: bool, squash: bool) -> Result<String> {
    let mut args = vec!["merge", "--no-stat", "-v"];
    if no_ff {
        args.push("--no-ff");
    }
    if squash {
        args.push("--squash");
    }
    args.push(branch);
    git.write(&args)
}

pub fn merge_abort(git: &Git) -> Result<String> {
    git.write(&["merge", "--abort"])
}

/// Concludes a merge after conflicts were resolved, keeping the prepared message.
pub fn merge_continue(git: &Git) -> Result<String> {
    git.write(&["commit", "--no-edit"])
}

const REBASE_CFG: [&str; 2] = ["-c", "core.commentChar=\u{1}"];

pub fn rebase(git: &Git, upstream: &str) -> Result<String> {
    let mut args = REBASE_CFG.to_vec();
    args.extend(["rebase", upstream]);
    git.write(&args)
}

pub fn rebase_onto(git: &Git, new_base: &str, upstream: &str, branch: Option<&str>) -> Result<String> {
    let mut args = REBASE_CFG.to_vec();
    args.extend(["rebase", "--onto", new_base, upstream]);
    if let Some(b) = branch {
        args.push(b);
    }
    git.write(&args)
}

/// Resolves `rev` to a full commit hash.
pub fn rev_parse(git: &Git, rev: &str) -> Result<String> {
    Ok(git.read(&["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")])?.trim().to_string())
}

/// Removes `hash` from the current branch (`rebase --autostash --onto <hash>^ <hash>`).
/// Only a non-merge, non-root commit that is reachable from HEAD can be dropped, and
/// not while another operation (rebase, merge, cherry-pick, revert) is in progress.
pub fn drop_commit(git: &Git, hash: &str) -> Result<String> {
    use crate::error::GitError;
    use crate::repo_state::{read_state, RepoState};
    let state = read_state(git);
    if state != RepoState::Normal {
        return Err(GitError::Invalid(format!(
            "Cannot drop commit during {}",
            match state {
                RepoState::Rebasing => "rebase",
                RepoState::Merging => "merge",
                RepoState::CherryPicking => "cherry-pick",
                _ => "revert",
            }
        )));
    }
    let parents = git.read(&["rev-list", "--parents", "-n", "1", hash])?;
    let parts: Vec<&str> = parents.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(GitError::Invalid(format!(
            "Selected commit has {} parents; only a commit with exactly one parent can be dropped",
            parts.len().saturating_sub(1)
        )));
    }
    let reachable = git.exec(&["merge-base", "--is-ancestor", hash, "HEAD"], None, crate::runner::Access::Read)?;
    if !reachable.success() {
        return Err(GitError::Invalid("The commit is not in the current branch".into()));
    }
    let mut args = REBASE_CFG.to_vec();
    args.extend(["rebase", "--autostash", "--onto", parts[1], hash]);
    git.write(&args)
}

/// Drops several commits from the current branch. Commits are dropped newest first,
/// so the older ones keep their hashes between steps. All are validated up front;
/// on success `ORIG_HEAD` points at the HEAD from before the first drop.
pub fn drop_commits(git: &Git, hashes: &[String]) -> Result<String> {
    use crate::error::GitError;
    if hashes.is_empty() {
        return Err(GitError::Invalid("No commits to drop".into()));
    }
    if hashes.len() == 1 {
        return drop_commit(git, &hashes[0]);
    }
    let mut order: Vec<(usize, &String)> = Vec::with_capacity(hashes.len());
    for h in hashes {
        let parents = git.read(&["rev-list", "--parents", "-n", "1", h])?;
        let n = parents.split_whitespace().count().saturating_sub(1);
        if n != 1 {
            return Err(GitError::Invalid(format!(
                "Commit {} has {n} parents; only commits with exactly one parent can be dropped",
                &h[..h.len().min(7)]
            )));
        }
        let reachable = git.exec(&["merge-base", "--is-ancestor", h, "HEAD"], None, crate::runner::Access::Read)?;
        if !reachable.success() {
            return Err(GitError::Invalid(format!("Commit {} is not in the current branch", &h[..h.len().min(7)])));
        }
        let range = format!("{h}..HEAD");
        let depth: usize = git.read(&["rev-list", "--count", &range])?.trim().parse().unwrap_or(0);
        order.push((depth, h));
    }
    order.sort_by_key(|(d, _)| *d);
    order.dedup_by(|a, b| a.1 == b.1);
    let orig = rev_parse(git, "HEAD")?;
    let mut out = String::new();
    for (k, (_, h)) in order.iter().enumerate() {
        match drop_commit(git, h) {
            Ok(o) => out.push_str(&o),
            Err(e) if k > 0 => {
                return Err(GitError::Invalid(format!("Dropped {k} of {} commits, then failed: {e}", order.len())));
            }
            Err(e) => return Err(e),
        }
    }
    git.write(&["update-ref", "ORIG_HEAD", &orig])?;
    Ok(out)
}

/// Undoes a drop: resets back to `old_head`, but only while HEAD is still `new_head`.
pub fn undo_drop(git: &Git, old_head: &str, new_head: &str) -> Result<String> {
    let head = rev_parse(git, "HEAD")?;
    if head != new_head {
        return Err(crate::error::GitError::Invalid("Cannot undo drop: HEAD has changed".into()));
    }
    git.write(&["reset", "--keep", old_head])
}

/// Undo the last commit: mixed-reset HEAD to its parent so the changes return as local changes.
/// Refuses unless `hash` is still HEAD.
pub fn undo_commit(git: &Git, hash: &str) -> Result<String> {
    let head = rev_parse(git, "HEAD")?;
    if head != hash {
        return Err(GitError::Invalid("Only the current HEAD commit can be undone".into()));
    }
    let parent = format!("{hash}^");
    reset(git, &parent, ResetMode::Mixed)
}

// ---- interactive rebase ----------------------------------------------------

/// What to do with one commit in an interactive rebase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoAction {
    Pick,
    /// Keep the commit with a new message.
    Reword(String),
    /// Meld into the previous commit, keeping both messages.
    Squash,
    /// Meld into the previous commit, discarding this message.
    Fixup,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub hash: String,
    pub action: TodoAction,
}

/// A commit that an interactive rebase from `hash` would rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseCommit {
    pub hash: String,
    pub subject: String,
    pub message: String,
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn ensure_normal_state(git: &Git, what: &str) -> Result<()> {
    use crate::repo_state::{read_state, RepoState};
    let op = match read_state(git) {
        RepoState::Normal => return Ok(()),
        RepoState::Rebasing => "rebase",
        RepoState::Merging => "merge",
        RepoState::CherryPicking => "cherry-pick",
        _ => "revert",
    };
    Err(GitError::Invalid(format!("Cannot {what} during {op}")))
}

/// Base argument for rebasing `hash` and everything after it: its parent, or `--root`.
fn rebase_base(git: &Git, hash: &str) -> Result<Option<String>> {
    let parents = git.read(&["rev-list", "--parents", "-n", "1", hash])?;
    let parts: Vec<&str> = parents.split_whitespace().collect();
    match parts.len() {
        1 => Ok(None),
        2 => Ok(Some(parts[1].to_string())),
        n => Err(GitError::Invalid(format!("Commit has {} parents; merge commits cannot be rewritten", n - 1))),
    }
}

/// Commits from `hash` (inclusive) up to HEAD, oldest first. Fails if `hash` is not
/// in the current branch or the range contains a merge commit.
pub fn rebase_commits(git: &Git, hash: &str) -> Result<Vec<RebaseCommit>> {
    let reachable = git.exec(&["merge-base", "--is-ancestor", hash, "HEAD"], None, crate::runner::Access::Read)?;
    if !reachable.success() {
        return Err(GitError::Invalid("The commit is not in the current branch".into()));
    }
    let base = rebase_base(git, hash)?;
    let range = match &base {
        Some(b) => format!("{b}..HEAD"),
        None => "HEAD".to_string(),
    };
    let merges = git.read(&["rev-list", "--merges", &range])?;
    if !merges.trim().is_empty() {
        return Err(GitError::Invalid("The range contains merge commits; they cannot be rewritten".into()));
    }
    let out = git.read(&["log", "--reverse", "--format=%H%x00%B%x01", &range])?;
    Ok(out
        .split('\u{1}')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            let (h, msg) = rec.split_once('\0')?;
            let message = msg.trim_end().to_string();
            Some(RebaseCommit {
                hash: h.to_string(),
                subject: message.lines().next().unwrap_or("").to_string(),
                message,
            })
        })
        .collect())
}

/// Runs `git rebase -i` over `from` and every later commit, with a prepared todo list
/// (`items` in the new order, oldest first; it must hold exactly those commits). Rewords run as `exec git commit --amend` steps,
/// so no editor is needed. Uncommitted changes are autostashed.
pub fn interactive_rebase(git: &Git, from: &str, items: &[TodoItem]) -> Result<String> {
    ensure_normal_state(git, "rebase")?;
    let first = items.first().ok_or_else(|| GitError::Invalid("Nothing to rebase".into()))?;
    if matches!(first.action, TodoAction::Squash | TodoAction::Fixup) {
        return Err(GitError::Invalid("The first commit cannot be squashed or fixed up".into()));
    }
    // Every item must belong to the range being rewritten.
    let expected = rebase_commits(git, from)?;
    let mut want: Vec<&str> = expected.iter().map(|c| c.hash.as_str()).collect();
    let mut got: Vec<&str> = items.iter().map(|i| i.hash.as_str()).collect();
    want.sort_unstable();
    got.sort_unstable();
    if want != got {
        return Err(GitError::Invalid("The branch changed; reopen the rebase dialog".into()));
    }
    let base = rebase_base(git, from)?;

    let dir = git.git_dir().join("tailgit-rebase");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(GitError::Spawn)?;
    let mut todo = String::new();
    let mut prev_kept = false;
    for (i, it) in items.iter().enumerate() {
        let h = &it.hash;
        match &it.action {
            TodoAction::Pick => todo.push_str(&format!("pick {h}\n")),
            TodoAction::Drop => todo.push_str(&format!("drop {h}\n")),
            TodoAction::Squash | TodoAction::Fixup if !prev_kept => {
                return Err(GitError::Invalid("A squash or fixup needs a kept commit before it".into()));
            }
            TodoAction::Squash => todo.push_str(&format!("squash {h}\n")),
            TodoAction::Fixup => todo.push_str(&format!("fixup {h}\n")),
            TodoAction::Reword(msg) => {
                if msg.trim().is_empty() {
                    return Err(GitError::Invalid("Commit message cannot be empty".into()));
                }
                let f = dir.join(format!("msg-{i}"));
                std::fs::write(&f, msg).map_err(GitError::Spawn)?;
                todo.push_str(&format!("pick {h}\n"));
                todo.push_str(&format!(
                    "exec git commit --amend --only --allow-empty --no-verify --cleanup=strip -F {}\n",
                    sh_quote(&f.to_string_lossy())
                ));
            }
        }
        if !matches!(it.action, TodoAction::Drop) {
            prev_kept = true;
        }
    }
    let todo_file = dir.join("todo");
    std::fs::write(&todo_file, todo).map_err(GitError::Spawn)?;
    let editor = format!("sequence.editor=cp {}", sh_quote(&todo_file.to_string_lossy()));
    let mut args = REBASE_CFG.to_vec();
    args.extend(["-c", editor.as_str(), "rebase", "-i", "--autostash"]);
    match &base {
        Some(b) => args.push(b),
        None => args.push("--root"),
    }
    git.write(&args)
}

/// Squashes the selected commits into one, placed where the oldest of them is.
/// Other commits in between keep their order after it. The resulting commit gets
/// `message`, or every selected message (oldest first) joined when `None`.
pub fn squash_commits(git: &Git, hashes: &[String], message: Option<&str>) -> Result<String> {
    ensure_normal_state(git, "squash commits")?;
    if hashes.len() < 2 {
        return Err(GitError::Invalid("Select at least two commits to squash".into()));
    }
    let mut full = Vec::with_capacity(hashes.len());
    for h in hashes {
        let r = rev_parse(git, h)?;
        let reachable = git.exec(&["merge-base", "--is-ancestor", &r, "HEAD"], None, crate::runner::Access::Read)?;
        if !reachable.success() {
            return Err(GitError::Invalid(format!("Commit {} is not in the current branch", &h[..h.len().min(7)])));
        }
        full.push(r);
    }
    // Oldest selected commit is the one with the most commits after it.
    let mut oldest = full[0].clone();
    let mut best = 0usize;
    for h in &full {
        let d: usize = git.read(&["rev-list", "--count", &format!("{h}..HEAD")])?.trim().parse().unwrap_or(0);
        if d >= best {
            best = d;
            oldest = h.clone();
        }
    }
    let range = rebase_commits(git, &oldest)?;
    let selected: Vec<&RebaseCommit> = range.iter().filter(|c| full.contains(&c.hash)).collect();
    let msg = match message {
        Some(m) => m.to_string(),
        None => selected.iter().map(|c| c.message.as_str()).collect::<Vec<_>>().join("\n\n"),
    };
    let mut items = vec![TodoItem { hash: selected[0].hash.clone(), action: TodoAction::Reword(msg) }];
    for c in &selected[1..] {
        items.push(TodoItem { hash: c.hash.clone(), action: TodoAction::Fixup });
    }
    for c in range.iter().filter(|c| !full.contains(&c.hash)) {
        items.push(TodoItem { hash: c.hash.clone(), action: TodoAction::Pick });
    }
    interactive_rebase(git, &oldest, &items)
}

/// Changes the message of `hash`. HEAD is amended directly (staged changes are left
/// alone); an older commit is reworded through an interactive rebase.
pub fn edit_commit_message(git: &Git, hash: &str, message: &str) -> Result<String> {
    ensure_normal_state(git, "edit commit message")?;
    if message.trim().is_empty() {
        return Err(GitError::Invalid("Commit message cannot be empty".into()));
    }
    let head = rev_parse(git, "HEAD")?;
    if head == hash {
        return git.write_stdin(
            &["commit", "--amend", "--only", "--allow-empty", "--no-verify", "--cleanup=strip", "-F", "-"],
            message.as_bytes(),
        );
    }
    let items: Vec<TodoItem> = rebase_commits(git, hash)?
        .into_iter()
        .map(|c| TodoItem {
            action: if c.hash == hash { TodoAction::Reword(message.to_string()) } else { TodoAction::Pick },
            hash: c.hash,
        })
        .collect();
    interactive_rebase(git, hash, &items)
}

pub fn rebase_continue(git: &Git) -> Result<String> {
    let mut args = REBASE_CFG.to_vec();
    args.extend(["rebase", "--continue"]);
    git.write(&args)
}

pub fn rebase_skip(git: &Git) -> Result<String> {
    let mut args = REBASE_CFG.to_vec();
    args.extend(["rebase", "--skip"]);
    git.write(&args)
}

pub fn rebase_abort(git: &Git) -> Result<String> {
    git.write(&["rebase", "--abort"])
}

pub fn cherry_pick(git: &Git, hashes: &[String], append_origin: bool) -> Result<String> {
    let mut args = vec!["cherry-pick"];
    if append_origin {
        args.push("-x");
    }
    args.extend(hashes.iter().map(|s| s.as_str()));
    git.write(&args)
}

pub fn cherry_pick_continue(git: &Git) -> Result<String> {
    git.write(&["cherry-pick", "--continue"])
}

pub fn cherry_pick_abort(git: &Git) -> Result<String> {
    git.write(&["cherry-pick", "--abort"])
}

pub fn cherry_pick_skip(git: &Git) -> Result<String> {
    git.write(&["cherry-pick", "--skip"])
}

pub fn revert(git: &Git, hashes: &[String]) -> Result<String> {
    let mut args = vec!["revert", "--no-edit"];
    args.extend(hashes.iter().map(|s| s.as_str()));
    git.write(&args)
}

pub fn revert_continue(git: &Git) -> Result<String> {
    git.write(&["revert", "--continue"])
}

pub fn revert_abort(git: &Git) -> Result<String> {
    git.write(&["revert", "--abort"])
}

pub fn reset(git: &Git, target: &str, mode: ResetMode) -> Result<String> {
    git.write(&["reset", mode.flag(), target])
}

// ---- working tree ----------------------------------------------------------

/// Stages files, including deletions (`git add -A -- paths`).
pub fn stage(git: &Git, paths: &[String]) -> Result<String> {
    git.write(&with_paths(vec!["add", "-A"], paths))
}

pub fn stage_all(git: &Git) -> Result<String> {
    git.write(&["add", "-A"])
}

/// Unstages files. Works on an unborn branch too.
pub fn unstage(git: &Git, paths: &[String]) -> Result<String> {
    let head = crate::refs::read_head(git)?;
    if matches!(head, Head::Unborn { .. }) {
        git.write(&with_paths(vec!["rm", "--cached", "-r", "-q"], paths))
    } else {
        git.write(&with_paths(vec!["reset", "-q", "HEAD"], paths))
    }
}

/// Reverts tracked files to HEAD, both index and work tree (IntelliJ "Rollback").
pub fn rollback(git: &Git, paths: &[String]) -> Result<String> {
    git.write(&with_paths(
        vec!["restore", "--source=HEAD", "--staged", "--worktree"],
        paths,
    ))
}

/// Discards unstaged changes only (restores from the index).
pub fn discard_worktree(git: &Git, paths: &[String]) -> Result<String> {
    git.write(&with_paths(vec!["restore", "--worktree"], paths))
}

/// Deletes untracked files from disk.
pub fn delete_untracked(git: &Git, paths: &[String]) -> Result<String> {
    for p in paths {
        let full = git.root().join(p);
        if full.is_dir() {
            std::fs::remove_dir_all(&full)?;
        } else if full.exists() {
            std::fs::remove_file(&full)?;
        }
    }
    Ok(format!("Deleted {} untracked file(s)", paths.len()))
}

pub fn commit(git: &Git, message: &str, amend: bool, sign_off: bool) -> Result<String> {
    if message.trim().is_empty() {
        return Err(GitError::Invalid("commit message is empty".into()));
    }
    let mut args = vec!["commit", "--cleanup=strip", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    if sign_off {
        args.push("--signoff");
    }
    git.write_stdin(&args, message.as_bytes())
}

/// The full message of the HEAD commit (used to prefill amend).
pub fn last_commit_message(git: &Git) -> Result<String> {
    Ok(git
        .read(&["log", "-1", "--encoding=UTF-8", "--pretty=format:%B", "HEAD"])?
        .trim_end()
        .to_string())
}

// ---- sync ------------------------------------------------------------------

pub fn fetch(git: &Git, remote: Option<&str>, progress: impl FnMut(&str)) -> Result<String> {
    let mut args = vec!["fetch", "--progress", "--prune"];
    match remote {
        Some(r) => args.push(r),
        None => args.push("--all"),
    }
    git.write_with_progress(&args, progress)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    Merge,
    Rebase,
    FastForwardOnly,
}

pub fn pull(git: &Git, mode: PullMode, progress: impl FnMut(&str)) -> Result<String> {
    let mut args = REBASE_CFG.to_vec();
    args.extend(["pull", "--no-stat", "-v", "--progress"]);
    args.push(match mode {
        PullMode::Merge => "--no-rebase",
        PullMode::Rebase => "--rebase",
        PullMode::FastForwardOnly => "--ff-only",
    });
    git.write_with_progress(&args, progress)
}

/// Fast-forwards a local branch to its upstream. The current branch is pulled
/// (`--ff-only`); any other branch is updated in place via `fetch <remote> <rb>:<branch>`,
/// which git refuses unless it is a fast-forward.
pub fn update_branch(
    git: &Git,
    branch: &str,
    remote: &str,
    remote_branch: &str,
    current: bool,
    progress: impl FnMut(&str),
) -> Result<String> {
    if current {
        let args = ["pull", "--no-stat", "-v", "--progress", "--ff-only", remote, remote_branch];
        return git.write_with_progress(&args, progress);
    }
    let spec = format!("refs/heads/{remote_branch}:refs/heads/{branch}");
    git.write_with_progress(&["fetch", "--progress", remote, &spec], progress)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushOptions {
    pub remote: String,
    pub local_branch: String,
    pub remote_branch: String,
    pub set_upstream: bool,
    pub force_with_lease: bool,
    pub tags: bool,
    pub no_verify: bool,
    /// Push only up to this commit instead of the branch tip.
    pub revision: Option<String>,
}

pub fn push(git: &Git, opts: &PushOptions, progress: impl FnMut(&str)) -> Result<String> {
    let src = match &opts.revision {
        Some(rev) => rev.clone(),
        None => format!("refs/heads/{}", opts.local_branch),
    };
    let spec = format!("{src}:refs/heads/{}", opts.remote_branch);
    let mut args = vec!["push", "--progress", "--porcelain"];
    if opts.set_upstream && opts.revision.is_none() {
        args.push("--set-upstream");
    }
    if opts.force_with_lease {
        args.push("--force-with-lease");
    }
    if opts.tags {
        args.push("--follow-tags");
    }
    if opts.no_verify {
        args.push("--no-verify");
    }
    args.push(&opts.remote);
    args.push(&spec);
    git.write_with_progress(&args, progress)
}

pub fn push_tag(git: &Git, remote: &str, tag: &str) -> Result<String> {
    let spec = format!("refs/tags/{tag}");
    git.write(&["push", "--porcelain", remote, &spec])
}
