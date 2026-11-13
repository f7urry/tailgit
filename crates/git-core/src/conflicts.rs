//! Merge conflict support. Port of git4idea's `GitMergeUtil` / `GitMergeProvider`.

use std::collections::BTreeMap;

use crate::content;
use crate::error::Result;
use crate::merge3::{self, Chunk};
use crate::repo_state::{self, RepoState};
use crate::runner::Git;

/// Which index stages exist for a conflicted path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnmergedPath {
    pub path: String,
    pub has_base: bool,
    pub has_ours: bool,
    pub has_theirs: bool,
}

/// `git ls-files --unmerged -z`: `mode blob stage\tpath\0`.
pub fn list_unmerged(git: &Git) -> Result<Vec<UnmergedPath>> {
    let text = git.read(&["ls-files", "--unmerged", "-z"])?;
    Ok(parse_unmerged(&text))
}

pub fn parse_unmerged(text: &str) -> Vec<UnmergedPath> {
    let mut map: BTreeMap<String, UnmergedPath> = BTreeMap::new();
    for rec in text.split('\0').filter(|s| !s.is_empty()) {
        let Some((meta, path)) = rec.split_once('\t') else {
            continue;
        };
        let stage = meta.split_whitespace().nth(2).unwrap_or("0");
        let e = map.entry(path.to_string()).or_insert_with(|| UnmergedPath {
            path: path.to_string(),
            ..Default::default()
        });
        match stage {
            "1" => e.has_base = true,
            "2" => e.has_ours = true,
            "3" => e.has_theirs = true,
            _ => {}
        }
    }
    map.into_values().collect()
}

/// The three versions of a conflicted file, oriented so that `yours` is always
/// the user's own change (stages 2/3 are swapped while rebasing, like IntelliJ).
#[derive(Debug, Clone)]
pub struct ConflictFile {
    pub path: String,
    pub base: Option<Vec<u8>>,
    pub yours: Option<Vec<u8>>,
    pub theirs: Option<Vec<u8>>,
    /// True when "yours" is index stage 3 (rebase).
    pub reversed: bool,
    pub yours_label: String,
    pub theirs_label: String,
}

impl ConflictFile {
    pub fn is_binary(&self) -> bool {
        [&self.base, &self.yours, &self.theirs]
            .iter()
            .any(|v| v.as_deref().is_some_and(content::is_binary))
    }

    /// Line-level three-way merge of the text versions.
    pub fn merge_chunks(&self) -> Vec<Chunk> {
        let s = |v: &Option<Vec<u8>>| {
            merge3::split_lines(&String::from_utf8_lossy(v.as_deref().unwrap_or_default()))
        };
        merge3::merge(&s(&self.base), &s(&self.yours), &s(&self.theirs))
    }
}

fn head_label(git: &Git, rev: &str) -> String {
    git.read(&["rev-parse", "--abbrev-ref", rev])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != rev)
        .or_else(|| {
            git.read(&["rev-parse", "--short", rev])
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| rev.to_string())
}

pub fn load(git: &Git, path: &str) -> Result<ConflictFile> {
    if !list_unmerged(git)?.iter().any(|u| u.path == path) {
        return Err(crate::error::GitError::Invalid(format!("{path} has no unresolved conflict")));
    }
    let state = repo_state::read_state(git);
    let reversed = state == RepoState::Rebasing;
    let mut base = content::read_stage(git, 1, path)?;
    // An incomplete base from the recursive/ort strategies has markers of its own:
    // use the file at the merge base instead (GitMergeUtil).
    if base.as_deref().is_some_and(|b| text_has_markers(&String::from_utf8_lossy(b))) {
        if let Some(theirs) = crate::merge_context::theirs_revision(git) {
            if let Ok(mb) = git.read(&["merge-base", "HEAD", &theirs]) {
                let rev = content::Source::Revision(mb.trim().to_string());
                if let Ok(Some(b)) = content::read(git, &rev, path) {
                    base = Some(b);
                }
            }
        }
    }
    let ours = content::read_stage(git, 2, path)?;
    let theirs = content::read_stage(git, 3, path)?;
    let (yours, theirs_v) = if reversed { (theirs, ours) } else { (ours, theirs) };
    let (yours_label, theirs_label) = match state {
        RepoState::Rebasing => (
            "Your commit (being rebased)".to_string(),
            format!("Upstream ({})", head_label(git, "HEAD")),
        ),
        RepoState::Merging => (
            format!("Yours ({})", head_label(git, "HEAD")),
            format!("Theirs ({})", head_label(git, "MERGE_HEAD")),
        ),
        RepoState::CherryPicking => (
            format!("Yours ({})", head_label(git, "HEAD")),
            format!("Cherry-picked ({})", head_label(git, "CHERRY_PICK_HEAD")),
        ),
        RepoState::Reverting => (
            format!("Yours ({})", head_label(git, "HEAD")),
            "Revert".to_string(),
        ),
        RepoState::Normal => ("Yours".to_string(), "Theirs".to_string()),
    };
    Ok(ConflictFile {
        path: path.to_string(),
        base,
        yours,
        theirs: theirs_v,
        reversed,
        yours_label,
        theirs_label,
    })
}

/// Accepts one whole side for the given paths and marks them resolved.
/// `yours` refers to the user's side (accounting for rebase reversal).
pub fn accept_side(git: &Git, paths: &[String], yours: bool) -> Result<String> {
    let reversed = repo_state::read_state(git) == RepoState::Rebasing;
    let use_ours_stage = yours != reversed;
    let mut log = String::new();
    for p in paths {
        let unmerged = list_unmerged(git)?;
        let entry = unmerged.iter().find(|u| &u.path == p);
        let side_exists = entry.is_none_or(|e| if use_ours_stage { e.has_ours } else { e.has_theirs });
        if side_exists {
            let flag = if use_ours_stage { "--ours" } else { "--theirs" };
            log.push_str(&git.write(&["checkout", flag, "--", p])?);
            log.push_str(&git.write(&["add", "-f", "--", p])?);
        } else {
            // The chosen side deleted the file.
            log.push_str(&git.write(&["rm", "-q", "--", p])?);
        }
    }
    Ok(log)
}

/// Writes merged content and stages it.
pub fn write_resolved(git: &Git, path: &str, content: &[u8]) -> Result<String> {
    std::fs::write(git.root().join(path), content)?;
    mark_resolved(git, &[path.to_string()])
}

/// "Resolve All Simple Conflicts": applies every non-conflicting change and the conflicts that
/// resolve automatically; files that end up fully resolved are written and staged.
pub fn resolve_simple(git: &Git, paths: &[String]) -> Result<String> {
    use crate::merge_model::{ApplyFrom, MergeModel};
    let text = |v: &Option<Vec<u8>>| String::from_utf8(v.clone().unwrap_or_default()).ok();
    let mut done = 0;
    for p in paths {
        let f = load(git, p)?;
        if f.is_binary() || f.yours.is_none() || f.theirs.is_none() {
            continue;
        }
        let (Some(b), Some(l), Some(r)) = (text(&f.base), text(&f.yours), text(&f.theirs)) else { continue };
        let mut m = MergeModel::new(&b, &l, &r, false);
        m.apply_non_conflicting(ApplyFrom::All);
        m.resolve_simple();
        if m.unresolved() == (0, 0) {
            write_resolved(git, p, m.result_text().as_bytes())?;
            done += 1;
        }
    }
    Ok(format!("Resolved {done} of {} file(s)", paths.len()))
}

/// Writes the (possibly partial) merge result to the working tree without staging it,
/// so the file stays unmerged ("Save and Close").
pub fn write_result(git: &Git, path: &str, content: &[u8]) -> Result<String> {
    std::fs::write(git.root().join(path), content)?;
    Ok(String::new())
}

pub fn mark_resolved(git: &Git, paths: &[String]) -> Result<String> {
    let mut args = vec!["add", "--ignore-errors", "-A", "-f", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    git.write(&args)
}

/// Like [`mark_resolved`], but refuses files that still contain conflict markers.
pub fn mark_resolved_checked(git: &Git, paths: &[String]) -> Result<String> {
    let with_markers: Vec<&str> = paths
        .iter()
        .filter(|p| has_markers(git, p))
        .map(String::as_str)
        .collect();
    if !with_markers.is_empty() {
        return Err(crate::error::GitError::Invalid(format!(
            "still contains conflict markers: {}",
            with_markers.join(", ")
        )));
    }
    mark_resolved(git, paths)
}

/// True if the work tree file has a complete `<<<<<<<` / `=======` / `>>>>>>>` block.
pub fn has_markers(git: &Git, path: &str) -> bool {
    let Ok(data) = std::fs::read(git.root().join(path)) else {
        return false;
    };
    text_has_markers(&String::from_utf8_lossy(&data))
}

/// IntelliJ's rule (`GitMergeUtil`): the text contains `<<<<<<<`, `\n=======` and `\n>>>>>>>`.
pub fn text_has_markers(text: &str) -> bool {
    text.contains("<<<<<<<") && text.contains("\n=======") && text.contains("\n>>>>>>>")
}

/// Resolves a modify/delete conflict (`UD`, `DU`, `AU`, `UA`): `keep` keeps the
/// version that still exists, otherwise the file is deleted.
pub fn resolve_deletion(git: &Git, path: &str, keep: bool) -> Result<String> {
    if !keep {
        return git.write(&["rm", "-q", "-f", "--", path]);
    }
    let entry = list_unmerged(git)?.into_iter().find(|u| u.path == path);
    let flag = match entry {
        Some(e) if e.has_ours => "--ours",
        Some(e) if e.has_theirs => "--theirs",
        _ => return mark_resolved(git, &[path.to_string()]),
    };
    let mut log = git.write(&["checkout", flag, "--", path])?;
    log.push_str(&mark_resolved(git, &[path.to_string()])?);
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unmerged() {
        let t = "100644 aaa 1\tf.txt\x00100644 bbb 2\tf.txt\x00100644 ccc 3\tf.txt\x00100644 ddd 2\tg.txt\x00";
        let u = parse_unmerged(t);
        assert_eq!(u.len(), 2);
        assert!(u[0].has_base && u[0].has_ours && u[0].has_theirs);
        assert!(!u[1].has_base && u[1].has_ours && !u[1].has_theirs);
    }

    #[test]
    fn detects_markers() {
        assert!(text_has_markers("a\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b\n"));
        assert!(!text_has_markers("a\n=======\nb\n"));
        assert!(!text_has_markers("<<<<<<< only\n"));
    }
}
