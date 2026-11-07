use std::path::{Path, PathBuf};

use crate::git::{Error, Git, Result};
use crate::status::{self, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum RepoState {
    Clean,
    Merging,
    Rebasing,
    CherryPicking,
    Reverting,
    Bisecting,
}

impl RepoState {
    pub fn label(self) -> Option<&'static str> {
        match self {
            RepoState::Clean => None,
            RepoState::Merging => Some("MERGING"),
            RepoState::Rebasing => Some("REBASING"),
            RepoState::CherryPicking => Some("CHERRY-PICKING"),
            RepoState::Reverting => Some("REVERTING"),
            RepoState::Bisecting => Some("BISECTING"),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Repo {
    pub(crate) git: Git,
    root: PathBuf,
    git_dir: PathBuf,
    name: String,
}

impl Repo {
    /// Find the repository containing `start`.
    pub fn discover(start: &Path) -> Result<Self> {
        let probe = Git::new(start);
        let out = match probe.run_text(&["rev-parse", "--show-toplevel", "--absolute-git-dir"]) {
            Ok(o) => o,
            Err(Error::Command { stderr, .. }) => return Err(Error::NotARepo(stderr)),
            Err(e) => return Err(e),
        };
        let mut lines = out.lines().map(str::trim);
        let root = PathBuf::from(lines.next().unwrap_or_default());
        let git_dir = PathBuf::from(lines.next().unwrap_or_default());
        if root.as_os_str().is_empty() || git_dir.as_os_str().is_empty() {
            return Err(Error::NotARepo(start.display().to_string()));
        }
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        Ok(Self { git: Git::new(&root), root, git_dir, name })
    }

    pub fn git(&self) -> &Git {
        &self.git
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn status(&self) -> Result<Status> {
        let out = self
            .git
            .run(&["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=normal"])?;
        Ok(status::parse(&out))
    }

    pub fn head_is_unborn(&self) -> bool {
        !self.git.succeeds(&["rev-parse", "-q", "--verify", "HEAD"])
    }

    pub fn state(&self) -> RepoState {
        let d = &self.git_dir;
        if d.join("rebase-merge").exists() || d.join("rebase-apply").exists() {
            RepoState::Rebasing
        } else if d.join("MERGE_HEAD").exists() {
            RepoState::Merging
        } else if d.join("CHERRY_PICK_HEAD").exists() {
            RepoState::CherryPicking
        } else if d.join("REVERT_HEAD").exists() {
            RepoState::Reverting
        } else if d.join("BISECT_LOG").exists() {
            RepoState::Bisecting
        } else {
            RepoState::Clean
        }
    }

    /// The prepared message of an in-progress merge, if any.
    pub fn merge_message(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.git_dir.join("MERGE_MSG")).ok()?;
        let msg: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        Some(msg.join("\n").trim().to_string()).filter(|m| !m.is_empty())
    }

    pub fn last_commit_message(&self) -> Result<String> {
        Ok(self.git.run_text(&["log", "-1", "--format=%B"])?.trim_end().to_string())
    }

    /// Changes whenever any ref moves; used to decide when to reload history.
    pub fn refs_fingerprint(&self) -> Result<String> {
        self.git.run_text(&["for-each-ref", "--format=%(objectname) %(refname)"])
    }
}
