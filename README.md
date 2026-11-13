# TailGIT

A desktop Git client in Rust (egui), modeled on IntelliJ IDEA's Git tool window
(`plugins/git4idea` + `platform/vcs-log` of IntelliJ Community). It shells out to the
`git` CLI like git4idea does, so hooks, credential helpers and SSH config work as usual.

```bash
cargo run --release -p tailgit-app -- /path/to/repo
```

## Layout

| Crate | What it is | Ported from |
|---|---|---|
| `crates/git-core` | git runner, log/refs/status/stash parsers, write ops, conflict loading, diff3 merge | `GitHandler`, `GitLogParser`, `GitIndexStatusUtil`, `GitImpl`, `GitMergeUtil` |
| `crates/log-graph` | commit graph layout (layout-index DFS, per-row lanes, long-edge arrows, lane colors) | `GraphLayoutBuilder`, `PrintElementGeneratorImpl`, `GraphColorManagerImpl` |
| `crates/tailgit-app` | the egui application (`tailgit` binary) | Git Log / Commit / Stash tool windows |

## Features

- **Log**: commit graph; ref badges; filters for text/hash (regex, match case), branch, user, date and paths; loads 50k commits per page; keyboard navigation.
- **Branches**: Local/Remote/Tags tree with `/` folders, favorites, ahead/behind counts and search. The context menu has checkout, new branch, rebase onto, merge, reset, set/unset upstream, rename, delete (local and remote) and push tag.
- **Commit details**: changed files, full message, parents, refs, containing branches. Diffs are unified or side-by-side.
- **Commit / staging area**: Staged / Changes / Unversioned / Conflicts lists, stage/unstage, rollback, amend, commit and push. In the unified diff each change has Stage / Unstage / Rollback actions, and single lines can be selected and staged on their own. The new index content is written directly (`hash-object` + `update-index`), and a selection made on outdated contents is refused.
- **Sync**: fetch, pull (merge / rebase / ff-only), and a push dialog with upstream, force-with-lease and tags options. Progress is streamed.
- **History ops**: cherry-pick, revert, reset (soft/mixed/hard/keep), checkout revision, tags.
- **Stash**: create (untracked / keep index), apply, pop, drop, unstash as branch.
- **Conflicts** (following IntelliJ's `MultipleFileMergeDialog` and three-way merge viewer):
  - The **Conflicts** dialog says what is being merged ("Merging branch **feature** into branch **main**", or the rebase, cherry-pick or revert being applied). It lists files as Name | Yours (branch) | Theirs (branch) with Modified/Deleted per side. Selection works with click, Shift and Cmd, plus Group by directory. Accept Yours, Accept Theirs and Merge… (Enter or double-click) act on the selection.
  - The dialog closes itself once nothing is unmerged. A rebase then asks "Continue Rebase", and a cherry-pick or revert opens the Commit tab with the original message. Closing it early warns "There are pending unresolved conflicts."
  - The **merge window** shows left (yours) | Result | right (theirs), titled like IntelliJ ("Changes from main", "Rebasing abc123 from feature", …). The result starts as the base text.
    - Each change has `≫`/`≪` (accept; Ctrl or Cmd+click resolves a conflict at once; accepting the second side appends) and `×` (ignore). Conflicts that can be merged automatically get `✨ Resolve`.
    - The toolbar has "Apply non-conflicting changes: Left / All / Right", Resolve Simple Conflicts, Revert Conflict Resolution, next/previous difference, and an "Automatically apply non-conflicting changes" option (off by default).
    - The result is editable. It finishes with Cancel / Accept Left / Accept Right / Apply Changes, each with IntelliJ's confirmation texts.
  - Yours/theirs are swapped during a rebase. Conflict markers are detected with IntelliJ's rule, and an incomplete base that contains markers is replaced by the merge-base version.
- **Auto-refresh**: refreshes on `.git` changes and on window focus. Recent repos and favorites are kept in the user config dir.

Credentials are not prompted for in the app: fetch/pull/push rely on your credential helper or ssh-agent.

## Tests

```bash
cargo test --workspace
```
