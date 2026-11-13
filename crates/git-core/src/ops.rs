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
}

pub fn push(git: &Git, opts: &PushOptions, progress: impl FnMut(&str)) -> Result<String> {
    let spec = format!("refs/heads/{}:refs/heads/{}", opts.local_branch, opts.remote_branch);
    let mut args = vec!["push", "--progress", "--porcelain"];
    if opts.set_upstream {
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
