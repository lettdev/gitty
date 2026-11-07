use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("git was not found in PATH")]
    GitNotFound,
    #[error("not a git repository: {0}")]
    NotARepo(String),
    #[error("`{command}` failed:\n{stderr}")]
    Command { command: String, stderr: String },
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Render arguments the way a user would type them, for display only.
pub fn display_command<S: AsRef<str>>(args: &[S]) -> String {
    let mut out = String::from("git");
    for arg in args {
        let a = arg.as_ref();
        out.push(' ');
        if a.is_empty() || a.chars().any(|c| c.is_whitespace() || c == '"' || c == '\'') {
            out.push('"');
            out.push_str(&a.replace('"', "\\\""));
            out.push('"');
        } else {
            out.push_str(a);
        }
    }
    out
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Git {
    workdir: PathBuf,
}

impl Git {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self { workdir: workdir.into() }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// A `git` command preconfigured for non-interactive, parseable use.
    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.workdir)
            .args(["-c", "core.quotepath=false", "-c", "color.ui=never"])
            .args(args)
            // Never block on a prompt we cannot show; the UI offers an
            // interactive retry instead.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", ":")
            .env("GIT_MERGE_AUTOEDIT", "no")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null());
        cmd
    }

    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
        self.run_input(args, None)
    }

    pub fn run_text(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.run(args)?).into_owned())
    }

    pub fn succeeds(&self, args: &[&str]) -> bool {
        self.command(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn run_input(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        let mut cmd = self.command(args);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(spawn_error)?;
        let writer = input.map(|data| {
            let mut stdin = child.stdin.take().expect("stdin was piped");
            let data = data.to_vec();
            // Write from a separate thread so a large output cannot deadlock us.
            std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            })
        });
        let out = child.wait_with_output()?;
        if let Some(w) = writer {
            let _ = w.join();
        }
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(Error::Command {
                command: display_command(args),
                stderr: error_text(&out.stderr, &out.stdout),
            })
        }
    }

    /// Run a long command (fetch/pull/push), reporting each progress line.
    /// The child is detached from the controlling terminal so ssh cannot
    /// scribble a password prompt over the UI.
    pub fn run_streaming(&self, args: &[&str], mut on_line: impl FnMut(&str)) -> Result<String> {
        let mut cmd = self.command(args);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        detach_from_tty(&mut cmd);
        let mut child = cmd.spawn().map_err(spawn_error)?;
        let mut stdout = child.stdout.take().expect("stdout was piped");
        let out_thread = std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = stdout.read_to_end(&mut v);
            v
        });
        let mut stderr = child.stderr.take().expect("stderr was piped");
        let mut lines: Vec<String> = Vec::new();
        let mut current: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        let mut flush = |current: &mut Vec<u8>, lines: &mut Vec<String>, final_line: bool| {
            let s = String::from_utf8_lossy(current).trim().to_string();
            current.clear();
            if !s.is_empty() {
                on_line(&s);
                if final_line {
                    lines.push(s);
                }
            }
        };
        loop {
            let n = match stderr.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            for &b in &buf[..n] {
                match b {
                    b'\n' => flush(&mut current, &mut lines, true),
                    b'\r' => flush(&mut current, &mut lines, false),
                    _ => current.push(b),
                }
            }
        }
        flush(&mut current, &mut lines, true);
        let status = child.wait()?;
        let stdout = out_thread.join().unwrap_or_default();
        let mut text = lines.join("\n");
        let stdout = String::from_utf8_lossy(&stdout);
        if !stdout.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(stdout.trim());
        }
        if status.success() {
            Ok(text)
        } else {
            Err(Error::Command { command: display_command(args), stderr: text })
        }
    }
}

fn error_text(stderr: &[u8], stdout: &[u8]) -> String {
    let e = String::from_utf8_lossy(stderr).trim().to_string();
    if e.is_empty() { String::from_utf8_lossy(stdout).trim().to_string() } else { e }
}

fn spawn_error(e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::NotFound { Error::GitNotFound } else { e.into() }
}

#[cfg(unix)]
fn detach_from_tty(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn detach_from_tty(_cmd: &mut Command) {}
