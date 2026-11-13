//! Partial staging: stage, unstage or roll back a subset of changed lines
//! (IntelliJ's staging area "Stage hunk" / "Stage selected lines").
//!
//! The new content is computed in memory and written straight into the index
//! (`hash-object -w` + `update-index --cacheinfo`) or the work tree, so no patch
//! context has to match.

use std::collections::HashSet;

use similar::{ChangeTag, TextDiff};

use crate::content::{self, Source};
use crate::error::{GitError, Result};
use crate::runner::{Access, Git};

/// Identifies one changed line of a diff: a deleted line by its 0-based index in
/// the old text, an inserted line by its 0-based index in the new text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LineKey {
    Old(usize),
    New(usize),
}

/// What to do with the selected lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialKind {
    /// Index ↔ work tree: copy the selected work tree changes into the index.
    Stage,
    /// HEAD ↔ index: take the selected changes back out of the index.
    Unstage,
    /// Index ↔ work tree: discard the selected changes from the work tree.
    Rollback,
}

/// `old` with the selected changes from `old → new` applied.
pub fn apply_selected(old: &str, new: &str, selected: &HashSet<LineKey>) -> String {
    rebuild(old, new, selected, false)
}

/// `new` with the selected changes from `old → new` undone.
pub fn revert_selected(old: &str, new: &str, selected: &HashSet<LineKey>) -> String {
    rebuild(old, new, selected, true)
}

fn rebuild(old: &str, new: &str, selected: &HashSet<LineKey>, revert: bool) -> String {
    let td = TextDiff::from_lines(old, new);
    let mut out = String::with_capacity(old.len().max(new.len()));
    for change in td.iter_all_changes() {
        let keep = match change.tag() {
            ChangeTag::Equal => true,
            ChangeTag::Delete => {
                let sel = selected.contains(&LineKey::Old(change.old_index().unwrap_or(0)));
                // Applying a deletion drops the line; reverting it brings it back.
                sel == revert
            }
            ChangeTag::Insert => {
                let sel = selected.contains(&LineKey::New(change.new_index().unwrap_or(0)));
                sel != revert
            }
        };
        if keep {
            out.push_str(change.value());
        }
    }
    out
}

fn as_text(bytes: Option<&[u8]>, what: &str) -> Result<String> {
    let b = bytes.unwrap_or_default();
    if content::is_binary(b) {
        return Err(GitError::Invalid(format!("{what} is binary; stage the whole file instead")));
    }
    String::from_utf8(b.to_vec())
        .map_err(|_| GitError::Invalid(format!("{what} is not UTF-8 text; stage the whole file instead")))
}

/// Mode of the index entry, or of the work tree file when it is not in the index.
fn index_mode(git: &Git, path: &str) -> Result<String> {
    let out = git.read(&["ls-files", "-s", "--", path])?;
    if let Some(mode) = out.split_whitespace().next() {
        return Ok(mode.to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(git.root().join(path)) {
            if meta.permissions().mode() & 0o111 != 0 {
                return Ok("100755".into());
            }
        }
    }
    Ok("100644".into())
}

/// Replaces the staged content of `path` with `data`.
pub fn write_index(git: &Git, path: &str, data: &[u8]) -> Result<String> {
    let mode = index_mode(git, path)?;
    let out = git.exec(&["hash-object", "-w", "--stdin"], Some(data), Access::Write)?;
    if !out.success() {
        return Err(GitError::Invalid(out.combined()));
    }
    let sha = out.stdout_str().trim().to_string();
    let info = format!("{mode},{sha},{path}");
    git.write(&["update-index", "--add", "--cacheinfo", &info])
}

/// Applies `kind` to the selected lines of `path`.
///
/// `expect_old` / `expect_new` are the contents the selection was made on; if
/// either side changed since, nothing is written and an error asks for a refresh.
pub fn apply(
    git: &Git,
    kind: PartialKind,
    path: &str,
    keys: &[LineKey],
    expect_old: Option<&[u8]>,
    expect_new: Option<&[u8]>,
) -> Result<String> {
    if keys.is_empty() {
        return Err(GitError::Invalid("no lines selected".into()));
    }
    let (old_src, new_src) = match kind {
        PartialKind::Stage | PartialKind::Rollback => (Source::Index, Source::WorkTree),
        PartialKind::Unstage => (Source::Revision("HEAD".into()), Source::Index),
    };
    let old = content::read(git, &old_src, path)?;
    let new = content::read(git, &new_src, path)?;
    if old.as_deref() != expect_old || new.as_deref() != expect_new {
        return Err(GitError::Invalid(format!(
            "{path} changed since the diff was shown; refresh and try again"
        )));
    }
    if new.is_none() {
        return Err(GitError::Invalid(format!(
            "{path} is deleted; stage or roll back the whole file instead"
        )));
    }
    let old_t = as_text(old.as_deref(), path)?;
    let new_t = as_text(new.as_deref(), path)?;
    let selected: HashSet<LineKey> = keys.iter().copied().collect();
    let n = keys.len();
    match kind {
        PartialKind::Stage => {
            let staged = apply_selected(&old_t, &new_t, &selected);
            write_index(git, path, staged.as_bytes())?;
            Ok(format!("Staged {n} line(s) of {path}"))
        }
        PartialKind::Unstage => {
            let staged = revert_selected(&old_t, &new_t, &selected);
            write_index(git, path, staged.as_bytes())?;
            Ok(format!("Unstaged {n} line(s) of {path}"))
        }
        PartialKind::Rollback => {
            let text = revert_selected(&old_t, &new_t, &selected);
            std::fs::write(git.root().join(path), text)?;
            Ok(format!("Rolled back {n} line(s) of {path}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(k: &[LineKey]) -> HashSet<LineKey> {
        k.iter().copied().collect()
    }

    #[test]
    fn applies_and_reverts_subsets() {
        let old = "a\nb\nc\nd\n";
        let new = "a\nB\nc\nd\ne\n";
        // b→B is Old(1)+New(1); the appended e is New(4).
        assert_eq!(apply_selected(old, new, &keys(&[LineKey::New(4)])), "a\nb\nc\nd\ne\n");
        assert_eq!(apply_selected(old, new, &keys(&[LineKey::Old(1), LineKey::New(1)])), "a\nB\nc\nd\n");
        // Only the deletion half of a replacement: the line disappears.
        assert_eq!(apply_selected(old, new, &keys(&[LineKey::Old(1)])), "a\nc\nd\n");
        assert_eq!(revert_selected(old, new, &keys(&[LineKey::New(4)])), "a\nB\nc\nd\n");
        assert_eq!(revert_selected(old, new, &keys(&[LineKey::Old(1), LineKey::New(1)])), "a\nb\nc\nd\ne\n");
        assert_eq!(apply_selected(old, new, &HashSet::new()), old);
        assert_eq!(revert_selected(old, new, &HashSet::new()), new);
    }

    #[test]
    fn keeps_missing_final_newline() {
        assert_eq!(apply_selected("a", "a\nb", &keys(&[LineKey::Old(0), LineKey::New(0), LineKey::New(1)])), "a\nb");
    }
}
