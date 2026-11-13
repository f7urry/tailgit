use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to start git: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("not a git repository: {0}")]
    NotARepository(String),
    #[error("{command} failed (exit {code}):\n{output}")]
    Command {
        command: String,
        code: i32,
        output: String,
    },
    #[error("cannot parse git output: {0}")]
    Parse(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

impl GitError {
    /// The git output for command failures, otherwise the display text.
    pub fn output(&self) -> String {
        match self {
            GitError::Command { output, .. } => output.clone(),
            other => other.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, GitError>;
