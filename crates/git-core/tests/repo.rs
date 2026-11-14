use std::path::Path;
use std::process::Command;

use git_core::log::LogFilter;
use git_core::repo_state::RepoState;
use git_core::{conflicts, log, ops, refs, repo_state, stash, status, Git};

fn init(dir: &Path) -> Git {
    let ok = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let git = Git::open(dir).unwrap();
    git.write(&["config", "user.name", "Tester"]).unwrap();
    git.write(&["config", "user.email", "t@example.com"]).unwrap();
    git.write(&["config", "commit.gpgsign", "false"]).unwrap();
    git
}

fn write(git: &Git, path: &str, text: &str) {
    std::fs::write(git.root().join(path), text).unwrap();
}

fn commit_file(git: &Git, path: &str, text: &str, msg: &str) {
    write(git, path, text);
    ops::stage(git, &[path.to_string()]).unwrap();
    ops::commit(git, msg, false, false).unwrap();
}

#[test]
fn empty_repo_has_empty_log() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    assert!(log::read_log(&git, &LogFilter::default()).unwrap().is_empty());
    assert!(matches!(refs::read_head(&git).unwrap(), refs::Head::Unborn { .. }));
}

#[test]
fn log_refs_status_and_details() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "a.txt", "one\n", "first");
    commit_file(&git, "a.txt", "one\ntwo\n", "second commit");
    ops::create_branch(&git, "feat/x", "HEAD~1").unwrap();
    ops::create_tag(&git, "v1", "HEAD", Some("release")).unwrap();

    let commits = log::read_log(&git, &LogFilter::default()).unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "second commit");
    assert_eq!(commits[0].parents, vec![commits[1].hash.clone()]);

    let r = refs::read_refs(&git).unwrap();
    assert_eq!(r.head.branch_name(), Some("main"));
    assert_eq!(r.local.len(), 2);
    assert_eq!(r.tags[0].target, commits[0].hash, "annotated tag is peeled");
    let labels = r.labels_by_commit();
    assert_eq!(labels[&commits[1].hash][0].name, "feat/x");

    let d = log::read_details(&git, &commits[0].hash).unwrap();
    assert_eq!(d.changes.len(), 1);
    assert_eq!(d.changes[0].path, "a.txt");
    let root = log::read_details(&git, &commits[1].hash).unwrap();
    assert_eq!(root.changes[0].kind, log::ChangeKind::Added);

    let filtered = log::read_log(
        &git,
        &LogFilter {
            text: "SECOND".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(filtered.len(), 1);
    let by_hash = log::read_log(
        &git,
        &LogFilter {
            text: commits[1].hash[..7].to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_hash[0].hash, commits[1].hash);

    write(&git, "a.txt", "changed\n");
    write(&git, "new.txt", "n\n");
    let st = status::read_status(&git).unwrap();
    assert!(st.iter().any(|e| e.path == "a.txt" && e.is_unstaged()));
    assert!(st.iter().any(|e| e.path == "new.txt" && e.is_untracked()));

    ops::stage(&git, &["a.txt".into()]).unwrap();
    assert!(status::read_status(&git)
        .unwrap()
        .iter()
        .any(|e| e.path == "a.txt" && e.is_staged()));
    ops::unstage(&git, &["a.txt".into()]).unwrap();
    ops::rollback(&git, &["a.txt".into()]).unwrap();
    assert_eq!(std::fs::read_to_string(git.root().join("a.txt")).unwrap(), "one\ntwo\n");

    stash::push(&git, "wip", true, false).unwrap();
    let stashes = stash::list(&git).unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("wip"));
    stash::pop(&git, "stash@{0}", false).unwrap();
    assert!(git.root().join("new.txt").exists());

    assert_eq!(log::containing_branches(&git, &commits[1].hash).unwrap().len(), 2);
}

#[test]
fn merge_conflict_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "f.txt", "a\nb\nc\n", "base");
    ops::checkout_new_branch(&git, "other", "HEAD").unwrap();
    commit_file(&git, "f.txt", "a\nTHEIRS\nc\n", "theirs");
    ops::checkout(&git, "main").unwrap();
    commit_file(&git, "f.txt", "a\nOURS\nc\n", "ours");

    assert!(ops::merge(&git, "other", false, false).is_err());
    assert_eq!(repo_state::read_state(&git), RepoState::Merging);
    let st = status::read_status(&git).unwrap();
    assert!(st.iter().any(|e| e.path == "f.txt" && e.is_conflicted()));

    let unmerged = conflicts::list_unmerged(&git).unwrap();
    assert_eq!(unmerged.len(), 1);
    let cf = conflicts::load(&git, "f.txt").unwrap();
    assert_eq!(cf.base.as_deref(), Some(&b"a\nb\nc\n"[..]));
    assert_eq!(cf.yours.as_deref(), Some(&b"a\nOURS\nc\n"[..]));
    assert_eq!(cf.theirs.as_deref(), Some(&b"a\nTHEIRS\nc\n"[..]));
    assert_eq!(git_core::merge3::conflict_count(&cf.merge_chunks()), 1);

    conflicts::write_resolved(&git, "f.txt", b"a\nBOTH\nc\n").unwrap();
    assert!(conflicts::list_unmerged(&git).unwrap().is_empty());
    ops::merge_continue(&git).unwrap();
    assert_eq!(repo_state::read_state(&git), RepoState::Normal);
    let head = log::read_log(&git, &LogFilter::default()).unwrap();
    assert_eq!(head[0].parents.len(), 2);
}

#[test]
fn rebase_conflict_is_reversed() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "f.txt", "a\nb\nc\n", "base");
    ops::checkout_new_branch(&git, "topic", "HEAD").unwrap();
    commit_file(&git, "f.txt", "a\nMINE\nc\n", "mine");
    ops::checkout(&git, "main").unwrap();
    commit_file(&git, "f.txt", "a\nUPSTREAM\nc\n", "upstream");
    ops::checkout(&git, "topic").unwrap();

    assert!(ops::rebase(&git, "main").is_err());
    assert_eq!(repo_state::read_state(&git), RepoState::Rebasing);
    let cf = conflicts::load(&git, "f.txt").unwrap();
    assert!(cf.reversed);
    assert_eq!(cf.yours.as_deref(), Some(&b"a\nMINE\nc\n"[..]));
    assert_eq!(cf.theirs.as_deref(), Some(&b"a\nUPSTREAM\nc\n"[..]));

    conflicts::accept_side(&git, &["f.txt".into()], true).unwrap();
    assert_eq!(std::fs::read_to_string(git.root().join("f.txt")).unwrap(), "a\nMINE\nc\n");
    ops::rebase_continue(&git).unwrap();
    assert_eq!(repo_state::read_state(&git), RepoState::Normal);
}

#[test]
fn reset_revert_cherry_pick() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "a.txt", "1\n", "c1");
    ops::checkout_new_branch(&git, "side", "HEAD").unwrap();
    commit_file(&git, "b.txt", "b\n", "side change");
    let side = log::read_log(&git, &LogFilter::default()).unwrap()[0].hash.clone();
    ops::checkout(&git, "main").unwrap();
    ops::cherry_pick(&git, std::slice::from_ref(&side), true).unwrap();
    assert!(git.root().join("b.txt").exists());
    let head = log::read_log(
        &git,
        &LogFilter {
            revisions: vec!["HEAD".into()],
            ..Default::default()
        },
    )
    .unwrap();
    ops::revert(&git, std::slice::from_ref(&head[0].hash)).unwrap();
    assert!(!git.root().join("b.txt").exists());
    ops::reset(&git, "HEAD~2", ops::ResetMode::Hard).unwrap();
    let head = log::read_log(
        &git,
        &LogFilter {
            revisions: vec!["HEAD".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(head.len(), 1);
}

fn staged_text(git: &Git, path: &str) -> String {
    git.read(&["show", &format!(":{path}")]).unwrap()
}

#[test]
fn partial_stage_unstage_rollback() {
    use git_core::partial::{self, LineKey, PartialKind};
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "f.txt", "a\nb\nc\nd\n", "base");
    write(&git, "f.txt", "A\nb\nc\nd\ne\n");
    let read = |s: &str| std::fs::read(git.root().join(s)).ok();

    // Stage only the appended line.
    let index = git.read_bytes(&["show", ":f.txt"]).unwrap();
    partial::apply(&git, PartialKind::Stage, "f.txt", &[LineKey::New(4)], Some(&index), read("f.txt").as_deref()).unwrap();
    assert_eq!(staged_text(&git, "f.txt"), "a\nb\nc\nd\ne\n");
    let st = status::read_status(&git).unwrap();
    assert!(st[0].is_staged() && st[0].is_unstaged());

    // A stale expectation is refused.
    let stale = partial::apply(&git, PartialKind::Stage, "f.txt", &[LineKey::Old(0)], Some(&index), read("f.txt").as_deref());
    assert!(stale.is_err());

    // Unstage it again (HEAD ↔ index).
    let head = git.read_bytes(&["show", "HEAD:f.txt"]).unwrap();
    let index = git.read_bytes(&["show", ":f.txt"]).unwrap();
    partial::apply(&git, PartialKind::Unstage, "f.txt", &[LineKey::New(4)], Some(&head), Some(&index)).unwrap();
    assert_eq!(staged_text(&git, "f.txt"), "a\nb\nc\nd\n");

    // Roll back the first-line change in the work tree, keep the appended line.
    let index = git.read_bytes(&["show", ":f.txt"]).unwrap();
    partial::apply(
        &git,
        PartialKind::Rollback,
        "f.txt",
        &[LineKey::Old(0), LineKey::New(0)],
        Some(&index),
        read("f.txt").as_deref(),
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(git.root().join("f.txt")).unwrap(), "a\nb\nc\nd\ne\n");

    // Untracked file: stage part of it.
    write(&git, "new.txt", "one\ntwo\n");
    partial::apply(&git, PartialKind::Stage, "new.txt", &[LineKey::New(0)], None, read("new.txt").as_deref()).unwrap();
    assert_eq!(staged_text(&git, "new.txt"), "one\n");
}

#[test]
fn modify_delete_conflict_and_markers() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "f.txt", "a\n", "base");
    commit_file(&git, "g.txt", "a\nb\nc\n", "base g");
    ops::checkout_new_branch(&git, "other", "HEAD").unwrap();
    git.write(&["rm", "-q", "f.txt"]).unwrap();
    commit_file(&git, "g.txt", "a\nTHEIRS\nc\n", "delete f");
    ops::checkout(&git, "main").unwrap();
    commit_file(&git, "f.txt", "a\nchanged\n", "modify f");
    commit_file(&git, "g.txt", "a\nOURS\nc\n", "ours g");
    assert!(ops::merge(&git, "other", false, false).is_err());

    let st = status::read_status(&git).unwrap();
    let f = st.iter().find(|e| e.path == "f.txt").unwrap();
    assert_eq!(f.conflict_description(), "deleted by them");

    // g.txt still has markers: the checked variant refuses it.
    assert!(conflicts::has_markers(&git, "g.txt"));
    assert!(conflicts::mark_resolved_checked(&git, &["g.txt".into()]).is_err());
    write(&git, "g.txt", "a\nBOTH\nc\n");
    conflicts::mark_resolved_checked(&git, &["g.txt".into()]).unwrap();

    conflicts::resolve_deletion(&git, "f.txt", true).unwrap();
    assert_eq!(std::fs::read_to_string(git.root().join("f.txt")).unwrap(), "a\nchanged\n");
    assert!(conflicts::list_unmerged(&git).unwrap().is_empty());
    ops::merge_continue(&git).unwrap();
    assert_eq!(repo_state::read_state(&git), RepoState::Normal);
}

#[test]
fn merge_context_wording() {
    use git_core::merge_context::{self, to_plain, SideStatus};
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "f.txt", "a\nb\nc\n", "base");
    commit_file(&git, "gone.txt", "x\n", "add gone");
    ops::checkout_new_branch(&git, "feature", "HEAD").unwrap();
    commit_file(&git, "f.txt", "a\nTHEIRS\nc\n", "theirs");
    git.write(&["rm", "-q", "gone.txt"]).unwrap();
    ops::commit(&git, "delete gone", false, false).unwrap();
    ops::checkout(&git, "main").unwrap();
    commit_file(&git, "f.txt", "a\nOURS\nc\n", "ours");
    commit_file(&git, "gone.txt", "x\ny\n", "modify gone");
    assert!(ops::merge(&git, "feature", false, false).is_err());

    let unmerged = conflicts::list_unmerged(&git).unwrap();
    let ctx = merge_context::read(&git, unmerged.len());
    assert_eq!(to_plain(&ctx.description), "Merging branch feature into branch main");
    assert_eq!(ctx.yours_column, "Yours (main)");
    assert_eq!(ctx.theirs_column, "Theirs (feature)");
    assert_eq!(to_plain(&ctx.right_title), "Changes from feature");
    let gone = unmerged.iter().find(|u| u.path == "gone.txt").unwrap();
    assert_eq!(ctx.statuses(gone), (SideStatus::Modified, SideStatus::Deleted));
    ops::merge_abort(&git).unwrap();

    // Rebase: yours/theirs swap, wording names the rebased branch and onto branch.
    ops::checkout(&git, "feature").unwrap();
    assert!(ops::rebase(&git, "main").is_err());
    let unmerged = conflicts::list_unmerged(&git).unwrap();
    let ctx = merge_context::read(&git, unmerged.len());
    assert!(ctx.reversed);
    let d = to_plain(&ctx.description);
    assert!(d.starts_with("Rebasing branch feature onto branch main. Current commit "), "{d}");
    assert_eq!(ctx.yours_column, "Yours (feature)");
    assert_eq!(ctx.theirs_column, "Theirs (main)");
    assert!(to_plain(&ctx.right_title).starts_with("Already rebased commits and commits from main"));
}

fn subjects(git: &Git) -> Vec<String> {
    git.read(&["log", "--format=%s"]).unwrap().lines().map(str::to_string).collect()
}

#[test]
fn drop_commit_removes_middle_commit() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "a.txt", "a\n", "A");
    commit_file(&git, "b.txt", "b\n", "B");
    commit_file(&git, "c.txt", "c\n", "C");
    let b = ops::rev_parse(&git, "HEAD~1").unwrap();
    ops::drop_commit(&git, &b).unwrap();
    assert_eq!(subjects(&git), ["C", "A"]);
    assert!(!git.root().join("b.txt").exists());
    assert!(git.root().join("c.txt").exists());
}

#[test]
fn drop_commit_keeps_uncommitted_changes() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "a.txt", "a\n", "A");
    commit_file(&git, "b.txt", "b\n", "B");
    commit_file(&git, "c.txt", "c\n", "C");
    write(&git, "a.txt", "a changed\n");
    let b = ops::rev_parse(&git, "HEAD~1").unwrap();
    ops::drop_commit(&git, &b).unwrap();
    assert_eq!(subjects(&git), ["C", "A"]);
    assert_eq!(std::fs::read_to_string(git.root().join("a.txt")).unwrap(), "a changed\n");
}

#[test]
fn drop_commit_rejects_root_and_foreign_commits() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "a.txt", "a\n", "A");
    commit_file(&git, "b.txt", "b\n", "B");
    let root = ops::rev_parse(&git, "HEAD~1").unwrap();
    assert!(ops::drop_commit(&git, &root).is_err());
    git.write(&["checkout", "-q", "-b", "side", "HEAD~1"]).unwrap();
    commit_file(&git, "s.txt", "s\n", "S");
    let side = ops::rev_parse(&git, "HEAD").unwrap();
    git.write(&["checkout", "-q", "main"]).unwrap();
    assert!(ops::drop_commit(&git, &side).is_err());
}

#[test]
fn undo_drop_restores_head_only_if_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let git = init(dir.path());
    commit_file(&git, "a.txt", "a\n", "A");
    commit_file(&git, "b.txt", "b\n", "B");
    commit_file(&git, "c.txt", "c\n", "C");
    let old = ops::rev_parse(&git, "HEAD").unwrap();
    let b = ops::rev_parse(&git, "HEAD~1").unwrap();
    ops::drop_commit(&git, &b).unwrap();
    let new = ops::rev_parse(&git, "HEAD").unwrap();
    assert_eq!(ops::rev_parse(&git, "ORIG_HEAD").unwrap(), old);
    commit_file(&git, "d.txt", "d\n", "D");
    assert!(ops::undo_drop(&git, &old, &new).is_err());
    git.write(&["reset", "--hard", &new]).unwrap();
    ops::undo_drop(&git, &old, &new).unwrap();
    assert_eq!(ops::rev_parse(&git, "HEAD").unwrap(), old);
}
