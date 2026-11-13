//! Git CLI wrapper for TailGIT: runs the `git` executable and parses its output.
//! Modeled on IntelliJ's git4idea command layer.

pub mod conflicts;
pub mod content;
pub mod error;
pub mod log;
pub mod merge_context;
pub mod merge_model;
pub mod merge3;
pub mod ops;
pub mod partial;
pub mod refs;
pub mod repo_state;
pub mod runner;
pub mod stash;
pub mod status;

pub use error::{GitError, Result};
pub use runner::Git;
