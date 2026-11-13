//! What a conflict is about, worded like IntelliJ's Conflicts dialog and merge
//! window. Port of `GitDefaultMergeDialogCustomizer`, `GitRebaseMergeDialogCustomizer`
//! and the cherry-pick/revert descriptions of `GitApplyChangesProcess`.

use crate::conflicts::UnmergedPath;
use crate::repo_state::{self, RepoState};
use crate::runner::Git;

/// A piece of rich text: plain, **bold** (branch names) or `code` (hashes, messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Plain(String),
    Bold(String),
    Code(String),
}

pub type Rich = Vec<Span>;

fn plain(s: impl Into<String>) -> Span {
    Span::Plain(s.into())
}
fn bold(s: impl Into<String>) -> Span {
    Span::Bold(s.into())
}
fn code(s: impl Into<String>) -> Span {
    Span::Code(s.into())
}

/// Plain text of a rich string (for tooltips and tests).
pub fn to_plain(r: &[Span]) -> String {
    r.iter()
        .map(|s| match s {
            Span::Plain(t) | Span::Bold(t) | Span::Code(t) => t.as_str(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeContext {
    pub state: RepoState,
    /// True when "yours" is index stage 3 (rebase), like `GitMergeProvider`'s reversed roots.
    pub reversed: bool,
    /// Dialog description, e.g. "Merging branch **feature** into branch **main**".
    pub description: Rich,
    /// Up to 3 lines of the commit being applied, shown under the description.
    pub commit_message: Option<String>,
    /// Column headers of the file table: "Yours (main)", "Theirs (feature)".
    pub yours_column: String,
    pub theirs_column: String,
    /// Merge window pane titles (left = yours, right = theirs).
    pub left_title: Rich,
    pub right_title: Rich,
}

/// Status of one side of a conflicted file in the dialog's Yours/Theirs columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideStatus {
    Modified,
    Deleted,
}

impl SideStatus {
    pub fn label(self) -> &'static str {
        match self {
            SideStatus::Modified => "Modified",
            SideStatus::Deleted => "Deleted",
        }
    }
}

impl MergeContext {
    /// (yours, theirs) statuses of a file; stages 2 and 3 are swapped in reversed roots.
    pub fn statuses(&self, u: &UnmergedPath) -> (SideStatus, SideStatus) {
        let s = |present: bool| if present { SideStatus::Modified } else { SideStatus::Deleted };
        let (ours, theirs) = (s(u.has_ours), s(u.has_theirs));
        if self.reversed {
            (theirs, ours)
        } else {
            (ours, theirs)
        }
    }
}

fn read_trim(git: &Git, args: &[&str]) -> Option<String> {
    git.read(args).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn short(git: &Git, rev: &str) -> String {
    read_trim(git, &["rev-parse", "--short", rev]).unwrap_or_else(|| rev.chars().take(7).collect())
}

fn git_file(git: &Git, name: &str) -> Option<String> {
    std::fs::read_to_string(git.git_dir().join(name))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// First `'quoted'` name on the first line of MERGE_MSG ("Merge branch 'x' into y").
fn quoted_in_merge_msg(msg: &str) -> Option<String> {
    let line = msg.lines().next()?;
    let start = line.find('\'')? + 1;
    let end = start + line[start..].find('\'')?;
    Some(line[start..end].to_string())
}

/// The branch name for a commit: the single local branch pointing at it, else the
/// single remote branch; ties are broken by the name quoted in MERGE_MSG.
pub fn branch_at(git: &Git, rev: &str, merge_msg: Option<&str>) -> Option<String> {
    let hash = read_trim(git, &["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")])?;
    let text = git
        .read(&["for-each-ref", "--points-at", &hash, "--format=%(refname)", "refs/heads", "refs/remotes"])
        .ok()?;
    let mut local = Vec::new();
    let mut remote = Vec::new();
    for r in text.lines() {
        if let Some(n) = r.strip_prefix("refs/heads/") {
            local.push(n.to_string());
        } else if let Some(n) = r.strip_prefix("refs/remotes/") {
            if !n.ends_with("/HEAD") {
                remote.push(n.to_string());
            }
        }
    }
    let pick = |names: &[String]| -> Option<String> {
        match names.len() {
            0 => None,
            1 => Some(names[0].clone()),
            _ => {
                let q = merge_msg.and_then(quoted_in_merge_msg)?;
                names.iter().find(|n| **n == q).cloned()
            }
        }
    };
    if !local.is_empty() {
        pick(&local)
    } else {
        pick(&remote)
    }
}

/// Current branch, or the short hash of a detached HEAD.
fn current(git: &Git) -> String {
    match crate::refs::read_head(git) {
        Ok(crate::refs::Head::Branch { name, .. }) | Ok(crate::refs::Head::Unborn { name }) => name,
        Ok(crate::refs::Head::Detached { hash }) => short(git, &hash),
        Err(_) => "HEAD".into(),
    }
}

/// (short hash, author, message preview of at most 3 lines).
fn commit_meta(git: &Git, rev: &str) -> Option<(String, String, String)> {
    let out = git.read(&["log", "-1", "--format=%h%x00%an%x00%B", rev]).ok()?;
    let mut f = out.splitn(3, '\0');
    let h = f.next()?.trim().to_string();
    let author = f.next()?.to_string();
    let msg = f.next().unwrap_or_default().trim_end();
    let lines: Vec<&str> = msg.lines().collect();
    let mut preview = lines.iter().take(3).copied().collect::<Vec<_>>();
    while preview.last().is_some_and(|l| l.trim().is_empty()) {
        preview.pop();
    }
    let mut preview = preview.join("\n");
    if lines.len() > 3 {
        preview.push('…');
    }
    Some((h, author, preview))
}

/// Builds the context for the repository's current state.
pub fn read(git: &Git, conflicted_files: usize) -> MergeContext {
    let state = repo_state::read_state(git);
    let cur = current(git);
    let default_description = vec![plain(if conflicted_files == 1 {
        "The following file has conflicts:".to_string()
    } else {
        "The following files have conflicts:".to_string()
    })];
    match state {
        RepoState::Merging => {
            let msg = git_file(git, "MERGE_MSG");
            let theirs = branch_at(git, "MERGE_HEAD", msg.as_deref()).unwrap_or_else(|| short(git, "MERGE_HEAD"));
            MergeContext {
                state,
                reversed: false,
                description: vec![
                    plain("Merging branch "),
                    bold(&theirs),
                    plain(" into branch "),
                    bold(&cur),
                ],
                commit_message: None,
                yours_column: format!("Yours ({cur})"),
                theirs_column: format!("Theirs ({theirs})"),
                left_title: vec![plain("Changes from "), bold(&cur)],
                right_title: vec![plain("Changes from "), bold(&theirs)],
            }
        }
        RepoState::Rebasing => {
            let dir = if git.git_dir().join("rebase-merge").is_dir() { "rebase-merge" } else { "rebase-apply" };
            let branch = git_file(git, &format!("{dir}/head-name"))
                .map(|h| h.strip_prefix("refs/heads/").unwrap_or(&h).to_string())
                .filter(|h| h != "detached HEAD");
            let onto = git_file(git, &format!("{dir}/onto"));
            let onto_branch = onto.as_deref().and_then(|o| branch_at(git, o, None));
            let onto_label = onto_branch.clone().or_else(|| onto.as_deref().map(|o| short(git, o)));
            let mut description = vec![plain("Rebasing")];
            if let Some(b) = &branch {
                description.extend([plain(" branch "), bold(b)]);
            }
            match (&onto_branch, &onto) {
                (Some(b), _) => description.extend([plain(" onto branch "), bold(b)]),
                (None, Some(o)) => description.extend([plain(" onto "), bold(short(git, o))]),
                (None, None) => description.push(plain(" onto diverging branches")),
            }
            let meta = commit_meta(git, "REBASE_HEAD");
            if let Some((h, author, _)) = &meta {
                description.extend([plain(". Current commit "), code(h), plain(format!(" made by {author}:"))]);
            }
            let from = branch.clone().unwrap_or_else(|| cur.clone());
            let rebasing = meta.as_ref().map(|m| m.0.clone()).unwrap_or_else(|| short(git, "HEAD"));
            let mut right = vec![plain("Already rebased commits")];
            if let Some(o) = &onto_label {
                right.extend([plain(" and commits from "), bold(o)]);
            }
            MergeContext {
                state,
                reversed: true,
                description,
                commit_message: meta.map(|m| m.2).filter(|m| !m.is_empty()),
                yours_column: format!("Yours ({from})"),
                theirs_column: match &onto_label {
                    Some(o) => format!("Theirs ({o})"),
                    None => "Theirs".into(),
                },
                left_title: vec![plain(format!("Rebasing {rebasing} from ")), bold(&from)],
                right_title: right,
            }
        }
        RepoState::CherryPicking | RepoState::Reverting => {
            let (head, op, theirs_col) = if state == RepoState::CherryPicking {
                ("CHERRY_PICK_HEAD", "cherry-picking", "cherry-pick")
            } else {
                ("REVERT_HEAD", "reverting", "revert")
            };
            let meta = commit_meta(git, head);
            let mut description = vec![plain(format!("Conflicts during {op} commit "))];
            let h = meta.as_ref().map(|m| m.0.clone()).unwrap_or_else(|| short(git, head));
            description.push(code(&h));
            if let Some((_, author, _)) = &meta {
                description.push(plain(format!(" made by {author}")));
            }
            MergeContext {
                state,
                reversed: false,
                description,
                commit_message: meta.map(|m| m.2).filter(|m| !m.is_empty()),
                yours_column: format!("Yours ({cur})"),
                theirs_column: format!("Theirs ({theirs_col})"),
                left_title: vec![plain("Local Changes")],
                right_title: vec![plain(format!("Changes from {theirs_col} {h}"))],
            }
        }
        RepoState::Normal => MergeContext {
            state,
            reversed: false,
            description: default_description,
            commit_message: None,
            yours_column: "Yours".into(),
            theirs_column: "Theirs".into(),
            left_title: vec![plain("Your version")],
            right_title: vec![plain("Their version")],
        },
    }
}

/// Revision whose changes are being brought in ("theirs" in git's terms), as in
/// `GitMergeUtil`: MERGE_HEAD, CHERRY_PICK_HEAD, REVERT_HEAD, or the commit a rebase stopped at.
pub fn theirs_revision(git: &Git) -> Option<String> {
    for head in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
        if git.git_dir().join(head).is_file() {
            return Some(head.to_string());
        }
    }
    git_file(git, "rebase-apply/original-commit").or_else(|| git_file(git, "rebase-merge/stopped-sha"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_branch() {
        assert_eq!(quoted_in_merge_msg("Merge branch 'feat/x' into main\n"), Some("feat/x".into()));
        assert_eq!(quoted_in_merge_msg("no quotes"), None);
    }
}
