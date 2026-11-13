//! Repository state detection (normal / merging / rebasing / cherry-picking / reverting),
//! like git4idea's `GitRepositoryReader.readState`.

use crate::runner::Git;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoState {
    Normal,
    Merging,
    Rebasing,
    CherryPicking,
    Reverting,
}

impl RepoState {
    pub fn label(self) -> &'static str {
        match self {
            RepoState::Normal => "",
            RepoState::Merging => "Merging",
            RepoState::Rebasing => "Rebasing",
            RepoState::CherryPicking => "Cherry-picking",
            RepoState::Reverting => "Reverting",
        }
    }
}

pub fn read_state(git: &Git) -> RepoState {
    let d = git.git_dir();
    if d.join("rebase-merge").is_dir() || d.join("rebase-apply").is_dir() {
        RepoState::Rebasing
    } else if d.join("MERGE_HEAD").is_file() {
        RepoState::Merging
    } else if d.join("CHERRY_PICK_HEAD").is_file() {
        RepoState::CherryPicking
    } else if d.join("REVERT_HEAD").is_file() {
        RepoState::Reverting
    } else {
        RepoState::Normal
    }
}
