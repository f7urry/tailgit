# TailGIT

A fast, native desktop Git client written in Rust with [egui](https://github.com/emilk/egui).

TailGIT follows the design of IntelliJ IDEA's Git tool window (`plugins/git4idea` and
`platform/vcs-log` in IntelliJ Community), with a macOS-style interface. It runs the `git`
CLI for every operation, the same way git4idea does, so your hooks, credential helpers,
SSH config and `.gitconfig` all work as they do in the terminal.

## Getting started

You need a Rust toolchain (edition 2021) and `git` on your `PATH`.

```bash
cargo run --release -p tailgit-app -- /path/to/repo
```

The path is optional. Without it, TailGIT opens a welcome screen where you can pick a
folder, type a path, or reopen a recent repository.

### macOS app bundle

```bash
scripts/bundle-macos.sh
```

This builds `TailGIT.app` with its icon, signs it ad hoc, and installs it to `~/Applications`,
replacing any earlier copy.

## Features

### Commit log

- Commit graph with per-lane colors. Long edges are shortened to arrows at both ends,
  as in IntelliJ.
- Branch and tag labels. You can put them above, below, left or right of the subject,
  in their own column, or hide them. Three styles are available: colorful, accent color,
  or border only.
- Columns: Subject, Branches, Author, Hash and Date. Drag a header to reorder, drag a
  divider to resize, and show or hide Author, Hash and Date. The layout is saved.
- Filters: text or hash (with regex and match case), branch, author, date range, paths,
  and "Hide merge commits".
- Sort by date or topologically.
- Loads 50,000 commits per page, with **Load more** for the rest.
- Keyboard navigation (arrows, Page Up/Down, Home/End, Cmd+A) and multi-select.
- Commit context menu: copy hash or subject, checkout revision, new branch, new tag,
  cherry-pick, revert, drop commit (with **Undo** in the status bar), reset the current
  branch to here, rebase onto here, and merge. Most of these also work on several commits
  at once.

### Branches

- Sidebar tree with HEAD, Local, Remote (one group per remote) and Tags. Names are split
  into folders on `/`.
- The current branch and favorites are listed first. Each branch shows ahead/behind counts.
  Search is built in.
- Context menu: checkout, new branch, rebase onto, merge, reset, set or unset upstream,
  rename, delete (local and remote), and push tag.

### Commit details and diffs

- Changed files, full message, parents, refs and the branches that contain the commit.
- Diffs are unified or side by side, with adjustable font size and optional `-`/`+` marks.

### Local changes and staging

- Staged, Changes, Unversioned and Conflicts lists, with stage, unstage, rollback, amend,
  and commit or commit and push.
- In the unified diff, each change block has **Stage**, **Unstage** and **Rollback**.
  You can also select single lines and stage them on their own.
- Partial staging writes the new index content directly (`hash-object` + `update-index`),
  so no patch has to apply. A selection made on outdated contents is refused.

### Sync

- Fetch; pull with merge, rebase or fast-forward only; and a push dialog with upstream,
  force-with-lease and tags options.
- The toolbar shows how many commits there are to pull and to push. Progress streams into
  the **Output** panel.

### History operations

Cherry-pick, revert, drop, reset (soft, mixed, hard or keep), checkout revision and tags.

### Stash

Create a stash (optionally with untracked files or keeping the index), then apply (with or
without restoring the index), pop, drop, or unstash it as a branch. You can browse the
changed files of each stash.

### Conflict resolution

This follows IntelliJ's `MultipleFileMergeDialog` and three-way merge viewer.

- A banner shows when a merge, rebase, cherry-pick or revert is in progress, with
  **Resolve…**, **Continue** and **Abort** buttons.
- The **Conflicts** dialog describes the operation ("Merging branch **feature** into branch
  **main**", or the rebase, cherry-pick or revert being applied). It lists files as
  Name | Yours | Theirs, marking each side as Modified or Deleted, and can group them by
  directory.
  - Click, Shift-click and Cmd-click select files. **Accept Yours**, **Accept Theirs** and
    **Merge…** (Enter or double-click) act on the selection.
  - The dialog closes once nothing is unmerged. After a rebase it offers **Continue Rebase**.
    After a cherry-pick or revert it opens the Commit tab with the original message.
- The **merge window** opens in its own native window as left (yours) | Result | right
  (theirs). Its titles follow IntelliJ, for example "Changes from main" or "Rebasing abc123
  from feature".
  - The result starts as the base text. Each change has `≫`/`≪` to accept it and `×` to
    ignore it. Accepting the second side of a conflict appends it, and Ctrl- or Cmd-click
    resolves a conflict in one step. Conflicts that can be merged automatically get
    `✨ Resolve`.
  - The toolbar has "Apply non-conflicting changes: Left / All / Right", Resolve Simple
    Conflicts, Revert Conflict Resolution, next/previous difference, and an "Automatically
    apply non-conflicting changes" option (off by default).
  - The result is editable, with light syntax highlighting. Finish with **Accept Left**,
    **Accept Right**, **Save and Close** (save without marking the file resolved) or
    **Apply Changes**. Closing the window asks before discarding changes.
- Yours and theirs are swapped during a rebase, as in IntelliJ. Conflict markers are detected
  with IntelliJ's rule, and an incomplete base that contains markers is replaced by the
  merge-base version.

### Interface

- Light, dark, or follow the system.
- Interface font: Inter (bundled), the system font, or any installed family, with
  adjustable size, hinting and sub-pixel positioning.
- The sidebar and details pane can be toggled from the status bar.
- Native macOS menu bar.
- Settings open with **Cmd+,**.

### Auto-refresh

The repository reloads when anything under `.git` changes and when the window gets focus.

## Configuration

Preferences, recent repositories, favorite branches and the log column layout are saved to
`settings.json` in the user config directory:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/dev.tailgit.tailgit/settings.json` |
| Linux | `~/.config/tailgit/settings.json` |
| Windows | `%APPDATA%\tailgit\tailgit\config\settings.json` |

TailGIT never asks for credentials. Fetch, pull and push rely on your Git credential helper
or ssh-agent.

## Project layout

| Crate | Purpose | Ported from (IntelliJ) |
|---|---|---|
| [`crates/git-core`](crates/git-core) | Runs `git`; parses log, refs, status and stash output; write operations; conflict loading; diff3 and the three-way merge model; partial staging | `GitHandler`, `GitLogParser`, `GitRepositoryReader`, `GitIndexStatusUtil`, `GitImpl`, `GitMergeUtil`, `MergeConflictModel` |
| [`crates/log-graph`](crates/log-graph) | Commit graph layout: layout-index DFS, per-row lanes, long-edge arrows, lane colors | `GraphLayoutBuilder`, `PrintElementGeneratorImpl`, `GraphColorManagerImpl` |
| [`crates/tailgit-app`](crates/tailgit-app) | The egui application (`tailgit` binary) | Git Log, Commit and Stash tool windows |

All git calls run on background threads and report back to the UI, so the window stays
responsive during long operations.

## Development

```bash
cargo test --workspace
```

## License

Apache-2.0. The bundled Inter font is under the SIL Open Font License
(`crates/tailgit-app/assets/Inter-OFL.txt`).
