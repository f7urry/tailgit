//! File contents at revisions and text diffs between them.

use similar::{ChangeTag, TextDiff};

use crate::error::Result;
use crate::runner::{Access, Git};

/// Where to read a file version from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Nothing (file absent on this side).
    Empty,
    /// A blob at a revision (`<rev>:<path>`).
    Revision(String),
    /// The staged version (`:<path>`).
    Index,
    /// The file on disk.
    WorkTree,
}

/// Contents of a file version, `None` if it does not exist there.
pub fn read(git: &Git, source: &Source, path: &str) -> Result<Option<Vec<u8>>> {
    match source {
        Source::Empty => Ok(None),
        Source::WorkTree => {
            let full = git.root().join(path);
            if full.is_file() {
                Ok(Some(std::fs::read(full)?))
            } else {
                Ok(None)
            }
        }
        Source::Index | Source::Revision(_) => {
            let spec = match source {
                Source::Revision(rev) => format!("{rev}:{path}"),
                _ => format!(":{path}"),
            };
            let out = git.exec(&["cat-file", "-p", &spec], None, Access::Read)?;
            Ok(out.success().then_some(out.stdout))
        }
    }
}

/// Reads a conflict stage blob (`:1:` base, `:2:` ours, `:3:` theirs).
pub fn read_stage(git: &Git, stage: u8, path: &str) -> Result<Option<Vec<u8>>> {
    let spec = format!(":{stage}:{path}");
    let out = git.exec(&["cat-file", "-p", &spec], None, Access::Read)?;
    Ok(out.success().then_some(out.stdout))
}

pub fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTag {
    Equal,
    Delete,
    Insert,
}

/// A line in a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub tag: LineTag,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
    pub text: String,
}

/// A row in a side-by-side diff; either side may be absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideRow {
    pub left: Option<(usize, String)>,
    pub right: Option<(usize, String)>,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hunkish {
    Lines(Vec<DiffLine>),
    /// A separator for skipped unchanged lines.
    Skip(usize),
}

#[derive(Debug, Clone, Default)]
pub struct TextDiffResult {
    pub binary: bool,
    pub identical: bool,
    pub too_large: bool,
    /// Unified view, with context hunks.
    pub unified: Vec<Hunkish>,
    /// Full side-by-side view.
    pub side_by_side: Vec<SideRow>,
    pub added: usize,
    pub removed: usize,
}

const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;

/// Diffs two file versions line by line.
pub fn diff(old: Option<&[u8]>, new: Option<&[u8]>, context: usize) -> TextDiffResult {
    let old_b = old.unwrap_or_default();
    let new_b = new.unwrap_or_default();
    if is_binary(old_b) || is_binary(new_b) {
        return TextDiffResult {
            binary: true,
            identical: old_b == new_b,
            ..Default::default()
        };
    }
    if old_b.len() + new_b.len() > MAX_DIFF_BYTES {
        return TextDiffResult {
            too_large: true,
            identical: old_b == new_b,
            ..Default::default()
        };
    }
    let old_s = String::from_utf8_lossy(old_b);
    let new_s = String::from_utf8_lossy(new_b);
    let td = TextDiff::from_lines(old_s.as_ref(), new_s.as_ref());
    let mut res = TextDiffResult {
        identical: old_b == new_b,
        ..Default::default()
    };

    // Unified view with context.
    let mut last_new_end = 0usize;
    for group in td.grouped_ops(context) {
        let first = &group[0];
        let skipped = first.new_range().start.saturating_sub(last_new_end);
        if skipped > 0 {
            res.unified.push(Hunkish::Skip(skipped));
        }
        let mut lines = Vec::new();
        for op in &group {
            for change in td.iter_changes(op) {
                let tag = match change.tag() {
                    ChangeTag::Equal => LineTag::Equal,
                    ChangeTag::Delete => LineTag::Delete,
                    ChangeTag::Insert => LineTag::Insert,
                };
                lines.push(DiffLine {
                    tag,
                    old_no: change.old_index().map(|i| i + 1),
                    new_no: change.new_index().map(|i| i + 1),
                    text: trim_eol(change.value()),
                });
            }
            last_new_end = op.new_range().end;
        }
        res.unified.push(Hunkish::Lines(lines));
    }
    let total_new = td.new_slices().len();
    if !res.unified.is_empty() && last_new_end < total_new {
        res.unified.push(Hunkish::Skip(total_new - last_new_end));
    }

    // Side-by-side: pair deletions with insertions inside each replace block.
    for op in td.ops() {
        let changes: Vec<_> = td.iter_changes(op).collect();
        let dels: Vec<_> = changes.iter().filter(|c| c.tag() == ChangeTag::Delete).collect();
        let ins: Vec<_> = changes.iter().filter(|c| c.tag() == ChangeTag::Insert).collect();
        res.removed += dels.len();
        res.added += ins.len();
        if dels.is_empty() && ins.is_empty() {
            for c in &changes {
                res.side_by_side.push(SideRow {
                    left: Some((c.old_index().unwrap_or(0) + 1, trim_eol(c.value()))),
                    right: Some((c.new_index().unwrap_or(0) + 1, trim_eol(c.value()))),
                    changed: false,
                });
            }
        } else {
            for i in 0..dels.len().max(ins.len()) {
                res.side_by_side.push(SideRow {
                    left: dels
                        .get(i)
                        .map(|c| (c.old_index().unwrap_or(0) + 1, trim_eol(c.value()))),
                    right: ins
                        .get(i)
                        .map(|c| (c.new_index().unwrap_or(0) + 1, trim_eol(c.value()))),
                    changed: true,
                });
            }
        }
    }
    res
}

fn trim_eol(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_lines() {
        let d = diff(Some(b"a\nb\nc\n"), Some(b"a\nB\nc\nd\n"), 3);
        assert_eq!(d.added, 2);
        assert_eq!(d.removed, 1);
        assert!(!d.binary);
        assert_eq!(d.unified.len(), 1);
        let changed = d.side_by_side.iter().filter(|r| r.changed).count();
        assert_eq!(changed, 2);
    }

    #[test]
    fn detects_binary() {
        assert!(diff(Some(b"a\0b"), Some(b"x"), 3).binary);
    }
}
