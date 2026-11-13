//! Process runner for the `git` executable.
//!
//! Port of git4idea's `GitHandler` / `GitImplBase`: every command is executed as
//! `git -c core.quotepath=false -c log.showSignature=false <cmd> <args>` with a
//! stable locale and with interactive prompts/editors disabled.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::error::{GitError, Result};

/// Output of a finished git process.
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl GitOutput {
    pub fn success(&self) -> bool {
        self.code == 0
    }

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Stdout and stderr combined, used for operation logs.
    pub fn combined(&self) -> String {
        let mut s = self.stdout_str();
        if !self.stderr.trim().is_empty() {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(&self.stderr);
        }
        s
    }
}

/// A git repository handle. Cheap to clone; safe to share between threads.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
    git_dir: PathBuf,
    executable: PathBuf,
    /// Serializes write commands, mirroring git4idea's per-repo write lock.
    write_lock: Arc<Mutex<()>>,
}

/// Whether a command only reads repository state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

impl Git {
    /// Opens the repository containing `path` (any directory inside the work tree).
    pub fn open(path: impl AsRef<Path>) -> Result<Git> {
        Self::open_with(path, PathBuf::from("git"))
    }

    pub fn open_with(path: impl AsRef<Path>, executable: PathBuf) -> Result<Git> {
        let path = path.as_ref();
        if !path.is_dir() {
            return Err(GitError::NotARepository(path.display().to_string()));
        }
        let probe = Git {
            root: path.to_path_buf(),
            git_dir: path.join(".git"),
            executable,
            write_lock: Arc::new(Mutex::new(())),
        };
        let out = probe.exec(
            &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
            None,
            Access::Read,
        )?;
        if !out.success() {
            return Err(GitError::NotARepository(path.display().to_string()));
        }
        let text = out.stdout_str();
        let mut lines = text.lines();
        let root = lines
            .next()
            .ok_or_else(|| GitError::NotARepository(path.display().to_string()))?;
        let git_dir = lines
            .next()
            .ok_or_else(|| GitError::NotARepository(path.display().to_string()))?;
        Ok(Git {
            root: PathBuf::from(root),
            git_dir: PathBuf::from(git_dir),
            executable: probe.executable,
            write_lock: probe.write_lock,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    fn command(&self, args: &[&str], access: Access) -> Command {
        let mut cmd = Command::new(&self.executable);
        cmd.current_dir(&self.root)
            .arg("-c")
            .arg("core.quotepath=false")
            .arg("-c")
            .arg("log.showSignature=false")
            .arg("-c")
            .arg("color.ui=false")
            .args(args)
            .env("LANGUAGE", "")
            .env("LC_ALL", "en_US.UTF-8")
            .env("GIT_TERMINAL_PROMPT", "0")
            // Never block on an interactive editor: continue/merge use the default message.
            .env("GIT_EDITOR", "true")
            .env("GIT_MERGE_AUTOEDIT", "no")
            .env("GIT_TRACE", "0")
            .env("GIT_TRACE_PACK_ACCESS", "")
            .env("GIT_TRACE_PACKET", "")
            .env("GIT_TRACE_PERFORMANCE", "0")
            .env("GIT_TRACE_SETUP", "0")
            .env("TAILGIT_GIT_EXECUTABLE", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if access == Access::Read {
            cmd.env("GIT_OPTIONAL_LOCKS", "0");
        }
        cmd
    }

    /// Runs git and returns the raw output, whatever the exit code.
    pub fn exec(&self, args: &[&str], stdin: Option<&[u8]>, access: Access) -> Result<GitOutput> {
        let _guard = match access {
            Access::Write => Some(self.write_lock.lock().unwrap_or_else(|e| e.into_inner())),
            Access::Read => None,
        };
        let mut cmd = self.command(args, access);
        if stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(GitError::Spawn)?;
        if let Some(input) = stdin {
            let mut pipe = child.stdin.take().expect("stdin piped");
            let input = input.to_vec();
            // Write on a separate thread so a large stdin cannot deadlock with stdout.
            std::thread::spawn(move || {
                let _ = pipe.write_all(&input);
            });
        }
        let out = child.wait_with_output().map_err(GitError::Spawn)?;
        Ok(GitOutput {
            code: out.status.code().unwrap_or(-1),
            stdout: out.stdout,
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    /// Runs a read command and returns stdout, failing on non-zero exit.
    pub fn read(&self, args: &[&str]) -> Result<String> {
        let out = self.exec(args, None, Access::Read)?;
        check(args, out).map(|o| o.stdout_str())
    }

    /// Runs a read command and returns raw stdout bytes, failing on non-zero exit.
    pub fn read_bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        let out = self.exec(args, None, Access::Read)?;
        check(args, out).map(|o| o.stdout)
    }

    /// Runs a write command; returns combined output on success.
    pub fn write(&self, args: &[&str]) -> Result<String> {
        let out = self.exec(args, None, Access::Write)?;
        check(args, out).map(|o| o.combined())
    }

    /// Runs a write command feeding `stdin`.
    pub fn write_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<String> {
        let out = self.exec(args, Some(stdin), Access::Write)?;
        check(args, out).map(|o| o.combined())
    }

    /// Runs a long write command (fetch/pull/push) and reports progress lines
    /// from stderr as they arrive. Progress lines are split on `\r` and `\n`.
    pub fn write_with_progress(
        &self,
        args: &[&str],
        mut on_progress: impl FnMut(&str),
    ) -> Result<String> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut child = self
            .command(args, Access::Write)
            .spawn()
            .map_err(GitError::Spawn)?;
        let mut stdout = child.stdout.take().expect("stdout piped");
        let stdout_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        });
        let mut stderr = child.stderr.take().expect("stderr piped");
        let mut all_err = Vec::new();
        let mut line = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stderr.read(&mut chunk).map_err(GitError::Spawn)?;
            if n == 0 {
                break;
            }
            for &b in &chunk[..n] {
                all_err.push(b);
                if b == b'\r' || b == b'\n' {
                    if !line.is_empty() {
                        on_progress(&String::from_utf8_lossy(&line));
                        line.clear();
                    }
                } else {
                    line.push(b);
                }
            }
        }
        if !line.is_empty() {
            on_progress(&String::from_utf8_lossy(&line));
        }
        let status = child.wait().map_err(GitError::Spawn)?;
        let out = GitOutput {
            code: status.code().unwrap_or(-1),
            stdout: stdout_reader.join().unwrap_or_default(),
            stderr: clean_progress(&String::from_utf8_lossy(&all_err)),
        };
        check(args, out).map(|o| o.combined())
    }
}

/// Keeps only the final state of `\r`-overwritten progress lines.
fn clean_progress(s: &str) -> String {
    s.split('\n')
        .map(|l| l.rsplit('\r').find(|p| !p.is_empty()).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn check(args: &[&str], out: GitOutput) -> Result<GitOutput> {
    if out.success() {
        Ok(out)
    } else {
        Err(GitError::Command {
            command: format!("git {}", args.join(" ")),
            code: out.code,
            output: out.combined(),
        })
    }
}
