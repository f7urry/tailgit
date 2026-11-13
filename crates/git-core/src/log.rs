//! Commit log loading. Port of git4idea's `GitLogUtil` / `GitLogParser` /
//! `GitLogProvider` filter mapping.

use crate::error::{GitError, Result};
use crate::runner::Git;

const RECORD_START: &str = "\u{1}\u{1}";
const ITEM_SEP: &str = "\u{2}\u{2}";
const RECORD_END: &str = "\u{3}\u{3}";
const RECORD_START_FMT: &str = "%x01%x01";
const ITEM_SEP_FMT: &str = "%x02%x02";
const RECORD_END_FMT: &str = "%x03%x03";

/// A commit as shown in the log table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    /// Committer timestamp (seconds since epoch).
    pub commit_time: i64,
    pub author_name: String,
    pub author_email: String,
    pub author_time: i64,
    pub subject: String,
}

impl Commit {
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash.len().min(8)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Unknown,
}

impl ChangeKind {
    pub fn from_code(c: char) -> ChangeKind {
        match c {
            'A' => ChangeKind::Added,
            'M' => ChangeKind::Modified,
            'D' => ChangeKind::Deleted,
            'R' => ChangeKind::Renamed,
            'C' => ChangeKind::Copied,
            'T' => ChangeKind::TypeChanged,
            'U' => ChangeKind::Unmerged,
            _ => ChangeKind::Unknown,
        }
    }

    pub fn letter(self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::Copied => "C",
            ChangeKind::TypeChanged => "T",
            ChangeKind::Unmerged => "U",
            ChangeKind::Unknown => "?",
        }
    }
}

/// A file changed by a commit (relative to its first parent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub kind: ChangeKind,
    pub path: String,
    pub old_path: Option<String>,
}

/// Full commit metadata for the details panel.
#[derive(Debug, Clone)]
pub struct CommitDetails {
    pub hash: String,
    pub parents: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    pub author_time: i64,
    pub committer_name: String,
    pub committer_email: String,
    pub commit_time: i64,
    pub subject: String,
    /// Full raw message (subject + body).
    pub message: String,
    pub changes: Vec<FileChange>,
}

/// Log filters, mapped to `git log` arguments like `GitLogProvider` does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFilter {
    /// Revisions/branches to start from. Empty means HEAD + all branches, remotes and tags.
    pub revisions: Vec<String>,
    /// Message text (`--grep`). Hash-like words additionally match commits by hash.
    pub text: String,
    pub regex: bool,
    pub match_case: bool,
    /// Authors (`--author`, one per user).
    pub users: Vec<String>,
    /// `--after=` value (anything git understands, e.g. `2026-01-31` or `2 weeks ago`).
    pub after: String,
    /// `--before=` value.
    pub before: String,
    /// Path filter; always passed last after `--`.
    pub paths: Vec<String>,
    pub max_count: Option<usize>,
    /// `--topo-order` instead of the default `--date-order`.
    pub topo_order: bool,
}

impl LogFilter {
    pub fn is_filtered(&self) -> bool {
        !self.text.trim().is_empty()
            || !self.users.is_empty()
            || !self.after.trim().is_empty()
            || !self.before.trim().is_empty()
            || !self.paths.is_empty()
    }

    /// Words of the text filter when every one of them looks like a hash prefix.
    pub fn hash_words(&self) -> Vec<String> {
        let words: Vec<&str> = self.text.split_whitespace().collect();
        if !words.is_empty() && words.iter().all(|w| is_hash_like(w)) {
            words.iter().map(|w| w.to_lowercase()).collect()
        } else {
            Vec::new()
        }
    }
}

pub fn is_hash_like(s: &str) -> bool {
    (4..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn pretty_format(fields: &[&str]) -> String {
    let body = fields
        .iter()
        .map(|f| format!("%{f}"))
        .collect::<Vec<_>>()
        .join(ITEM_SEP_FMT);
    format!("--pretty=format:{RECORD_START_FMT}{body}{RECORD_END_FMT}")
}

const LOG_FIELDS: [&str; 7] = ["H", "P", "ct", "an", "ae", "at", "s"];

/// Builds the `git log` argument list for a filter (without the leading `log`).
pub fn log_args(filter: &LogFilter) -> Vec<String> {
    let mut args = vec![
        pretty_format(&LOG_FIELDS),
        "--encoding=UTF-8".to_string(),
        if filter.topo_order { "--topo-order" } else { "--date-order" }.to_string(),
    ];
    if let Some(n) = filter.max_count {
        args.push(format!("--max-count={n}"));
    }
    if filter.revisions.is_empty() {
        args.extend(
            ["HEAD", "--branches", "--remotes", "--tags"]
                .iter()
                .map(|s| s.to_string()),
        );
    } else {
        args.extend(filter.revisions.iter().cloned());
    }
    if !filter.after.trim().is_empty() {
        args.push(format!("--after={}", filter.after.trim()));
    }
    if !filter.before.trim().is_empty() {
        args.push(format!("--before={}", filter.before.trim()));
    }
    for user in &filter.users {
        let user = user.trim();
        if !user.is_empty() {
            if filter.regex {
                args.push(format!("--author={user}"));
            } else {
                args.push(format!("--author={}", regex_escape(user)));
            }
        }
    }
    if !filter.users.is_empty() {
        // Author patterns are always regexes in git; make the plain-text form behave literally.
        args.push("--regexp-ignore-case".to_string());
    }
    let text = filter.text.trim();
    if !text.is_empty() && filter.hash_words().is_empty() {
        args.push(format!("--grep={text}"));
        if filter.regex {
            args.push("--extended-regexp".to_string());
        } else {
            args.push("--fixed-strings".to_string());
        }
        if !filter.match_case && !args.iter().any(|a| a == "--regexp-ignore-case") {
            args.push("--regexp-ignore-case".to_string());
        }
    }
    if !filter.paths.is_empty() {
        args.push("--full-history".to_string());
        args.push("--simplify-merges".to_string());
        args.push("--".to_string());
        args.extend(filter.paths.iter().cloned());
    }
    args
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Loads the commit log for the given filter.
pub fn read_log(git: &Git, filter: &LogFilter) -> Result<Vec<Commit>> {
    let hash_words = filter.hash_words();
    if !hash_words.is_empty() {
        return read_by_hash(git, &hash_words);
    }
    let args = log_args(filter);
    let mut argv: Vec<&str> = vec!["log"];
    argv.extend(args.iter().map(|s| s.as_str()));
    let out = git.exec(&argv, None, crate::runner::Access::Read)?;
    if !out.success() {
        // An unborn HEAD (fresh repository) is not an error: the log is just empty.
        if out.stderr.contains("does not have any commits")
            || out.stderr.contains("bad default revision 'HEAD'")
            || out.stderr.contains("ambiguous argument 'HEAD'")
        {
            return Ok(Vec::new());
        }
        return Err(GitError::Command {
            command: "git log".into(),
            code: out.code,
            output: out.combined(),
        });
    }
    parse_log(&out.stdout_str())
}

/// Commits whose hash starts with any of the given prefixes.
fn read_by_hash(git: &Git, words: &[String]) -> Result<Vec<Commit>> {
    let mut hashes = Vec::new();
    for w in words {
        if let Ok(h) = git.read(&["rev-parse", "--verify", "--quiet", &format!("{w}^{{commit}}")]) {
            let h = h.trim().to_string();
            if !h.is_empty() && !hashes.contains(&h) {
                hashes.push(h);
            }
        }
    }
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let fmt = pretty_format(&LOG_FIELDS);
    let mut argv = vec!["log", fmt.as_str(), "--encoding=UTF-8", "--no-walk=unsorted"];
    argv.extend(hashes.iter().map(|s| s.as_str()));
    parse_log(&git.read(&argv)?)
}

fn records(text: &str) -> impl Iterator<Item = &str> {
    text.split(RECORD_START).filter_map(|chunk| {
        let end = chunk.find(RECORD_END)?;
        Some(&chunk[..end])
    })
}

/// Parses output produced with [`log_args`].
pub fn parse_log(text: &str) -> Result<Vec<Commit>> {
    let mut commits = Vec::new();
    for rec in records(text) {
        let f: Vec<&str> = rec.split(ITEM_SEP).collect();
        if f.len() != LOG_FIELDS.len() {
            return Err(GitError::Parse(format!("unexpected log record: {rec:?}")));
        }
        commits.push(Commit {
            hash: f[0].to_string(),
            parents: f[1].split_whitespace().map(str::to_string).collect(),
            commit_time: f[2].parse().unwrap_or(0),
            author_name: f[3].to_string(),
            author_email: f[4].to_string(),
            author_time: f[5].parse().unwrap_or(0),
            subject: f[6].to_string(),
        });
    }
    Ok(commits)
}

const DETAIL_FIELDS: [&str; 10] = ["H", "P", "an", "ae", "at", "cn", "ce", "ct", "s", "B"];

/// Loads full metadata and the changed files of one commit.
pub fn read_details(git: &Git, hash: &str) -> Result<CommitDetails> {
    let fmt = pretty_format(&DETAIL_FIELDS);
    let text = git.read(&["log", &fmt, "--encoding=UTF-8", "--no-walk=unsorted", hash])?;
    let rec = records(&text)
        .next()
        .ok_or_else(|| GitError::Parse(format!("no commit {hash}")))?;
    let f: Vec<&str> = rec.split(ITEM_SEP).collect();
    if f.len() != DETAIL_FIELDS.len() {
        return Err(GitError::Parse(format!("unexpected details record: {rec:?}")));
    }
    let parents: Vec<String> = f[1].split_whitespace().map(str::to_string).collect();
    let changes = read_changes(git, hash, parents.first().map(|s| s.as_str()))?;
    Ok(CommitDetails {
        hash: f[0].to_string(),
        parents,
        author_name: f[2].to_string(),
        author_email: f[3].to_string(),
        author_time: f[4].parse().unwrap_or(0),
        committer_name: f[5].to_string(),
        committer_email: f[6].to_string(),
        commit_time: f[7].parse().unwrap_or(0),
        subject: f[8].to_string(),
        message: f[9].trim_end().to_string(),
        changes,
    })
}

/// Files that differ between revision `rev` and the working tree (tracked files).
pub fn read_worktree_changes(git: &Git, rev: &str) -> Result<Vec<FileChange>> {
    let text = git.read(&["diff", "-z", "-M", "--name-status", rev, "--"])?;
    Ok(parse_name_status_z(&text))
}

/// Files changed by `hash` relative to `parent` (or to the empty tree for root commits).
pub fn read_changes(git: &Git, hash: &str, parent: Option<&str>) -> Result<Vec<FileChange>> {
    let text = match parent {
        Some(p) => git.read(&["diff-tree", "-r", "-z", "-M", "--name-status", p, hash])?,
        None => git.read(&[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "-r",
            "-z",
            "-M",
            "--name-status",
            hash,
        ])?,
    };
    Ok(parse_name_status_z(&text))
}

/// Parses `--name-status -z` output: `STATUS\0path\0` or `R100\0old\0new\0`.
pub fn parse_name_status_z(text: &str) -> Vec<FileChange> {
    let mut parts = text.split('\0').filter(|s| !s.is_empty());
    let mut changes = Vec::new();
    while let Some(status) = parts.next() {
        let kind = ChangeKind::from_code(status.chars().next().unwrap_or('?'));
        match kind {
            ChangeKind::Renamed | ChangeKind::Copied => {
                let (Some(old), Some(new)) = (parts.next(), parts.next()) else {
                    break;
                };
                changes.push(FileChange {
                    kind,
                    path: new.to_string(),
                    old_path: Some(old.to_string()),
                });
            }
            _ => {
                let Some(path) = parts.next() else { break };
                changes.push(FileChange {
                    kind,
                    path: path.to_string(),
                    old_path: None,
                });
            }
        }
    }
    changes
}

/// Local and remote branches that contain `hash` (`git branch -a --contains`).
pub fn containing_branches(git: &Git, hash: &str) -> Result<Vec<String>> {
    let text = git.read(&[
        "branch",
        "-a",
        "--no-color",
        "--format=%(refname:short)",
        "--contains",
        hash,
    ])?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.ends_with("/HEAD") && !l.starts_with('('))
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records() {
        let text = "\u{1}\u{1}aaa\u{2}\u{2}bbb ccc\u{2}\u{2}100\u{2}\u{2}Ann\u{2}\u{2}a@x\u{2}\u{2}90\u{2}\u{2}Subj\u{3}\u{3}\n\
                    \u{1}\u{1}bbb\u{2}\u{2}\u{2}\u{2}50\u{2}\u{2}Bob\u{2}\u{2}b@x\u{2}\u{2}50\u{2}\u{2}root\u{3}\u{3}";
        let c = parse_log(text).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].parents, vec!["bbb", "ccc"]);
        assert!(c[1].parents.is_empty());
        assert_eq!(c[1].subject, "root");
    }

    #[test]
    fn name_status() {
        let c = parse_name_status_z("M\0a.txt\0R100\0old.rs\0new.rs\0A\0b c\0");
        assert_eq!(c.len(), 3);
        assert_eq!(c[1].kind, ChangeKind::Renamed);
        assert_eq!(c[1].old_path.as_deref(), Some("old.rs"));
        assert_eq!(c[2].path, "b c");
    }

    #[test]
    fn filter_args() {
        let f = LogFilter {
            revisions: vec!["main".into()],
            text: "fix bug".into(),
            users: vec!["ann".into()],
            after: "2026-01-01".into(),
            paths: vec!["src".into()],
            ..Default::default()
        };
        let a = log_args(&f);
        assert!(a.contains(&"main".to_string()));
        assert!(a.contains(&"--grep=fix bug".to_string()));
        assert!(a.contains(&"--fixed-strings".to_string()));
        assert!(a.contains(&"--author=ann".to_string()));
        assert!(a.contains(&"--after=2026-01-01".to_string()));
        assert_eq!(a[a.len() - 2], "--");
        assert_eq!(a.last().unwrap(), "src");
    }

    #[test]
    fn hash_detection() {
        let f = LogFilter {
            text: "a1b2c3d 0f0f".into(),
            ..Default::default()
        };
        assert_eq!(f.hash_words().len(), 2);
        let f = LogFilter {
            text: "fix a1b2c3d".into(),
            ..Default::default()
        };
        assert!(f.hash_words().is_empty());
    }
}
