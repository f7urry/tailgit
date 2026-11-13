//! Stash list and operations. Port of git4idea's `GitStashUtils`.

use crate::error::Result;
use crate::runner::Git;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub hash: String,
    /// Reflog selector, e.g. `stash@{0}`.
    pub selector: String,
    pub time: i64,
    pub message: String,
}

pub fn list(git: &Git) -> Result<Vec<Stash>> {
    let out = git.exec(
        &["stash", "list", "--pretty=format:%H%x00%at%x00%gd%x00%gs"],
        None,
        crate::runner::Access::Read,
    )?;
    if !out.success() {
        return Ok(Vec::new());
    }
    Ok(parse_list(&out.stdout_str()))
}

pub fn parse_list(text: &str) -> Vec<Stash> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\0').collect();
            (f.len() == 4).then(|| Stash {
                hash: f[0].to_string(),
                time: f[1].parse().unwrap_or(0),
                selector: f[2].to_string(),
                message: f[3].to_string(),
            })
        })
        .collect()
}

pub fn push(git: &Git, message: &str, include_untracked: bool, keep_index: bool) -> Result<String> {
    let mut args = vec!["stash", "push"];
    if include_untracked {
        args.push("--include-untracked");
    }
    if keep_index {
        args.push("--keep-index");
    }
    if !message.trim().is_empty() {
        args.push("-m");
        args.push(message);
    }
    git.write(&args)
}

pub fn apply(git: &Git, selector: &str, restore_index: bool) -> Result<String> {
    let mut args = vec!["stash", "apply"];
    if restore_index {
        args.push("--index");
    }
    args.push(selector);
    git.write(&args)
}

pub fn pop(git: &Git, selector: &str, restore_index: bool) -> Result<String> {
    let mut args = vec!["stash", "pop"];
    if restore_index {
        args.push("--index");
    }
    args.push(selector);
    git.write(&args)
}

pub fn drop(git: &Git, selector: &str) -> Result<String> {
    git.write(&["stash", "drop", selector])
}

pub fn branch(git: &Git, name: &str, selector: &str) -> Result<String> {
    git.write(&["stash", "branch", name, selector])
}
