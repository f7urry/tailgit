//! Branches, tags and HEAD. Port of git4idea's `GitRepositoryReader`
//! (for-each-ref mode) and the `GitRefManager` branch-layout ordering.

use crate::error::Result;
use crate::runner::Git;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// On a branch (short name) pointing at a commit.
    Branch { name: String, hash: String },
    /// On a branch that has no commits yet.
    Unborn { name: String },
    /// Detached at a commit.
    Detached { hash: String },
}

impl Head {
    pub fn branch_name(&self) -> Option<&str> {
        match self {
            Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
            Head::Detached { .. } => None,
        }
    }

    pub fn hash(&self) -> Option<&str> {
        match self {
            Head::Branch { hash, .. } | Head::Detached { hash } => Some(hash),
            Head::Unborn { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short name, e.g. `feat/x` or `origin/main`.
    pub name: String,
    /// Full ref name, e.g. `refs/heads/feat/x`.
    pub full_name: String,
    pub target: String,
    pub is_current: bool,
    /// Upstream short name (local branches only).
    pub upstream: Option<String>,
    /// Commits ahead of upstream.
    pub ahead: u32,
    /// Commits behind upstream.
    pub behind: u32,
    /// Upstream configured but gone on the remote.
    pub upstream_gone: bool,
}

impl Branch {
    /// For a remote branch `origin/feat/x`, returns (`origin`, `feat/x`).
    pub fn split_remote<'a>(&'a self, remotes: &[String]) -> Option<(&'a str, &'a str)> {
        let mut best: Option<(&str, &str)> = None;
        for r in remotes {
            if let Some(rest) = self.name.strip_prefix(r.as_str()).and_then(|s| s.strip_prefix('/')) {
                if best.is_none_or(|(b, _)| r.len() > b.len()) {
                    best = Some((&self.name[..r.len()], rest));
                }
            }
        }
        best
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    /// The commit the tag points to (peeled for annotated tags).
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refs {
    pub head: Head,
    pub local: Vec<Branch>,
    pub remote: Vec<Branch>,
    pub tags: Vec<Tag>,
    pub remotes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// A ref label attached to a commit in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLabel {
    pub kind: RefKind,
    pub name: String,
    pub is_current: bool,
}

pub fn read_head(git: &Git) -> Result<Head> {
    let sym = git.exec(&["symbolic-ref", "-q", "HEAD"], None, crate::runner::Access::Read)?;
    let hash = git
        .exec(&["rev-parse", "--verify", "-q", "HEAD"], None, crate::runner::Access::Read)?
        .stdout_str()
        .trim()
        .to_string();
    if sym.success() {
        let full = sym.stdout_str().trim().to_string();
        let name = full.strip_prefix("refs/heads/").unwrap_or(&full).to_string();
        if hash.is_empty() {
            Ok(Head::Unborn { name })
        } else {
            Ok(Head::Branch { name, hash })
        }
    } else {
        Ok(Head::Detached { hash })
    }
}

pub fn read_refs(git: &Git) -> Result<Refs> {
    let head = read_head(git)?;
    let text = git.read(&[
        "for-each-ref",
        "--no-color",
        "--format=%(refname)%09%(objectname)%09%(HEAD)%09%(upstream:short)%09%(upstream:track,nobracket)%09%(*objectname)",
        "refs/heads",
        "refs/remotes",
        "refs/tags",
    ])?;
    let remotes: Vec<String> = git
        .read(&["remote"])?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let mut refs = parse_for_each_ref(&text);
    refs.0.sort_by(|a, b| a.name.cmp(&b.name));
    refs.1.sort_by(|a, b| a.name.cmp(&b.name));
    refs.2.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Refs {
        head,
        local: refs.0,
        remote: refs.1,
        tags: refs.2,
        remotes,
    })
}

/// Parses the tab-separated `for-each-ref` output used by [`read_refs`].
pub fn parse_for_each_ref(text: &str) -> (Vec<Branch>, Vec<Branch>, Vec<Tag>) {
    let mut local = Vec::new();
    let mut remote = Vec::new();
    let mut tags = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let full = f[0];
        if let Some(name) = full.strip_prefix("refs/tags/") {
            let target = if f[5].is_empty() { f[1] } else { f[5] };
            tags.push(Tag {
                name: name.to_string(),
                target: target.to_string(),
            });
            continue;
        }
        let (name, is_local) = if let Some(n) = full.strip_prefix("refs/heads/") {
            (n, true)
        } else if let Some(n) = full.strip_prefix("refs/remotes/") {
            if n.ends_with("/HEAD") {
                continue;
            }
            (n, false)
        } else {
            continue;
        };
        let (ahead, behind, gone) = parse_track(f[4]);
        let b = Branch {
            name: name.to_string(),
            full_name: full.to_string(),
            target: f[1].to_string(),
            is_current: f[2] == "*",
            upstream: (!f[3].is_empty()).then(|| f[3].to_string()),
            ahead,
            behind,
            upstream_gone: gone,
        };
        if is_local {
            local.push(b);
        } else {
            remote.push(b);
        }
    }
    (local, remote, tags)
}

/// Parses `ahead 1, behind 2` / `gone` from `%(upstream:track,nobracket)`.
fn parse_track(s: &str) -> (u32, u32, bool) {
    let mut ahead = 0;
    let mut behind = 0;
    if s.trim() == "gone" {
        return (0, 0, true);
    }
    for part in s.split(',') {
        let mut w = part.split_whitespace();
        match (w.next(), w.next().and_then(|n| n.parse().ok())) {
            (Some("ahead"), Some(n)) => ahead = n,
            (Some("behind"), Some(n)) => behind = n,
            _ => {}
        }
    }
    (ahead, behind, false)
}

impl Refs {
    /// Map of commit hash -> labels, for decorating log rows.
    pub fn labels_by_commit(&self) -> std::collections::HashMap<String, Vec<RefLabel>> {
        let mut map: std::collections::HashMap<String, Vec<RefLabel>> = Default::default();
        if let Head::Detached { hash } = &self.head {
            map.entry(hash.clone()).or_default().push(RefLabel {
                kind: RefKind::Head,
                name: "HEAD".into(),
                is_current: true,
            });
        }
        for b in &self.local {
            map.entry(b.target.clone()).or_default().push(RefLabel {
                kind: RefKind::LocalBranch,
                name: b.name.clone(),
                is_current: b.is_current,
            });
        }
        for b in &self.remote {
            map.entry(b.target.clone()).or_default().push(RefLabel {
                kind: RefKind::RemoteBranch,
                name: b.name.clone(),
                is_current: false,
            });
        }
        for t in &self.tags {
            map.entry(t.target.clone()).or_default().push(RefLabel {
                kind: RefKind::Tag,
                name: t.name.clone(),
                is_current: false,
            });
        }
        for labels in map.values_mut() {
            labels.sort_by(|a, b| {
                b.is_current
                    .cmp(&a.is_current)
                    .then(a.kind.cmp(&b.kind))
                    .then(a.name.cmp(&b.name))
            });
        }
        map
    }

    pub fn current_branch(&self) -> Option<&Branch> {
        self.local.iter().find(|b| b.is_current)
    }

    pub fn find_local(&self, name: &str) -> Option<&Branch> {
        self.local.iter().find(|b| b.name == name)
    }
}

/// Layout priority of a ref, lower first (leftmost lane). Based on
/// `GitRefManager.GitBranchLayoutComparator`, but keeps the main line and the current
/// branch to the left: origin/main|master, main|master, current branch, other remote
/// branches, other local branches, tags, detached HEAD.
pub fn layout_priority(label: &RefLabel) -> u8 {
    let main = |n: &str| n == "main" || n == "master";
    match label.kind {
        RefKind::RemoteBranch if label.name == "origin/main" || label.name == "origin/master" => 0,
        RefKind::LocalBranch if main(&label.name) => 1,
        RefKind::LocalBranch if label.is_current => 2,
        RefKind::RemoteBranch => 3,
        RefKind::LocalBranch => 4,
        RefKind::Tag => 5,
        RefKind::Head => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs() {
        let text = "refs/heads/main\taaa\t*\torigin/main\tahead 2, behind 1\t\n\
                    refs/heads/feat/x\tbbb\t \t\t\t\n\
                    refs/remotes/origin/HEAD\taaa\t \t\t\t\n\
                    refs/remotes/origin/main\tccc\t \t\t\t\n\
                    refs/tags/v1\ttagobj\t \t\t\tddd\n";
        let (l, r, t) = parse_for_each_ref(text);
        assert_eq!(l.len(), 2);
        assert!(l[0].is_current);
        assert_eq!((l[0].ahead, l[0].behind), (2, 1));
        assert_eq!(l[0].upstream.as_deref(), Some("origin/main"));
        assert_eq!(r.len(), 1);
        assert_eq!(t[0].target, "ddd");
    }

    #[test]
    fn splits_remote() {
        let b = Branch {
            name: "origin/feat/x".into(),
            full_name: String::new(),
            target: String::new(),
            is_current: false,
            upstream: None,
            ahead: 0,
            behind: 0,
            upstream_gone: false,
        };
        assert_eq!(b.split_remote(&["origin".into()]), Some(("origin", "feat/x")));
    }
}
