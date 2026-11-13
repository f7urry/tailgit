//! Working tree status. Port of git4idea's `GitIndexStatusUtil`
//! (`git status --porcelain -z`).

use crate::error::Result;
use crate::runner::Git;

/// One entry of `git status --porcelain`, with the raw XY codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    /// Original path for renames/copies.
    pub orig_path: Option<String>,
    /// Index (staged) status code `X`.
    pub index: char,
    /// Work tree status code `Y`.
    pub worktree: char,
}

impl StatusEntry {
    pub fn is_untracked(&self) -> bool {
        self.index == '?' && self.worktree == '?'
    }

    pub fn is_ignored(&self) -> bool {
        self.index == '!' && self.worktree == '!'
    }

    /// Unmerged XY codes: `DD AU UD UA DU AA UU`.
    pub fn is_conflicted(&self) -> bool {
        matches!(
            (self.index, self.worktree),
            ('D', 'D') | ('A', 'U') | ('U', 'D') | ('U', 'A') | ('D', 'U') | ('A', 'A') | ('U', 'U')
        )
    }

    pub fn is_staged(&self) -> bool {
        !self.is_conflicted() && !matches!(self.index, ' ' | '?' | '!')
    }

    pub fn is_unstaged(&self) -> bool {
        !self.is_conflicted() && !matches!(self.worktree, ' ' | '?' | '!')
    }

    /// Human readable description of a conflict kind.
    pub fn conflict_description(&self) -> &'static str {
        match (self.index, self.worktree) {
            ('D', 'D') => "both deleted",
            ('A', 'U') => "added by us",
            ('U', 'D') => "deleted by them",
            ('U', 'A') => "added by them",
            ('D', 'U') => "deleted by us",
            ('A', 'A') => "both added",
            ('U', 'U') => "both modified",
            _ => "",
        }
    }
}

pub fn read_status(git: &Git) -> Result<Vec<StatusEntry>> {
    let text = git.read(&["status", "--porcelain", "-z", "--untracked-files=all"])?;
    Ok(parse_porcelain_z(&text))
}

/// Parses `git status --porcelain -z`. For renames and copies the entry is
/// `XY new\0old\0`.
pub fn parse_porcelain_z(text: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut parts = text.split('\0');
    while let Some(item) = parts.next() {
        if item.len() < 4 {
            continue;
        }
        let mut chars = item.chars();
        let x = chars.next().unwrap_or(' ');
        let y = chars.next().unwrap_or(' ');
        let path = item[3..].to_string();
        let orig_path = if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            parts.next().map(str::to_string)
        } else {
            None
        };
        entries.push(StatusEntry {
            path,
            orig_path,
            index: x,
            worktree: y,
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status() {
        let s = parse_porcelain_z("M  a.txt\0 M b.txt\0R  new.rs\0old.rs\0?? u.txt\0UU c.txt\0");
        assert_eq!(s.len(), 5);
        assert!(s[0].is_staged() && !s[0].is_unstaged());
        assert!(s[1].is_unstaged() && !s[1].is_staged());
        assert_eq!(s[2].orig_path.as_deref(), Some("old.rs"));
        assert!(s[3].is_untracked());
        assert!(s[4].is_conflicted() && !s[4].is_staged());
    }
}
