//! Mutating operations. Each returns the git command line it ran, for display.

use std::fs;

use crate::git::{Result, display_command};
use crate::oplog::OpKind;
use crate::repo::Repo;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum DiscardTarget {
    /// Restore a tracked file's worktree content from the index.
    Worktree { path: String },
    /// Delete an untracked file or directory.
    Untracked { path: String },
    /// Reset a file to HEAD in both index and worktree.
    Staged { path: String, orig: Option<String>, index_status: char },
}

impl DiscardTarget {
    pub fn path(&self) -> &str {
        match self {
            DiscardTarget::Worktree { path } | DiscardTarget::Untracked { path } | DiscardTarget::Staged { path, .. } => path,
        }
    }
}

pub fn describe_paths(paths: &[String]) -> String {
    match paths {
        [one] => one.rsplit('/').find(|s| !s.is_empty()).unwrap_or(one).to_string(),
        many => format!("{} files", many.len()),
    }
}

impl Repo {
    fn exec(&self, args: &[&str]) -> Result<String> {
        self.git.run(args)?;
        Ok(display_command(args))
    }

    fn exec_input(&self, args: &[&str], input: &str) -> Result<String> {
        self.git.run_input(args, Some(input.as_bytes()))?;
        Ok(display_command(args))
    }

    pub fn stage(&self, paths: &[String]) -> Result<String> {
        let mut args = vec!["add", "-A", "--"];
        args.extend(paths.iter().map(String::as_str));
        self.record(OpKind::Index, format!("Stage {}", describe_paths(paths)), |r, _| r.exec(&args))
    }

    pub fn unstage(&self, paths: &[String]) -> Result<String> {
        let mut args = if self.head_is_unborn() {
            vec!["rm", "--cached", "-r", "-q", "--"]
        } else {
            vec!["restore", "--staged", "--"]
        };
        args.extend(paths.iter().map(String::as_str));
        self.record(OpKind::Index, format!("Unstage {}", describe_paths(paths)), |r, _| r.exec(&args))
    }

    pub fn stage_all(&self) -> Result<String> {
        self.record(OpKind::Index, "Stage all changes", |r, _| r.exec(&["add", "-A"]))
    }

    pub fn unstage_all(&self) -> Result<String> {
        self.record(OpKind::Index, "Unstage all changes", |r, _| {
            if r.head_is_unborn() { r.exec(&["rm", "--cached", "-r", "-q", "."]) } else { r.exec(&["reset", "-q"]) }
        })
    }

    /// Apply a partial patch to the index: forward to stage, reverse to unstage.
    pub fn apply_to_index(&self, patch: &str, reverse: bool, path: &str) -> Result<String> {
        let mut args = vec!["apply", "--cached", "--recount", "--whitespace=nowarn"];
        if reverse {
            args.push("--reverse");
        }
        args.push("-");
        let what = if reverse { "Unstage" } else { "Stage" };
        let name = describe_paths(&[path.to_string()]);
        self.record(OpKind::Index, format!("{what} lines in {name}"), |r, _| r.exec_input(&args, patch))
    }

    /// Throw away part of a file's unstaged changes (the file is backed up).
    pub fn discard_patch(&self, patch: &str, path: &str) -> Result<String> {
        let name = describe_paths(&[path.to_string()]);
        self.record(OpKind::Discard, format!("Discard lines in {name}"), |r, backups| {
            r.backup_path(path, backups)?;
            r.exec_input(&["apply", "--reverse", "--recount", "--whitespace=nowarn", "-"], patch)
        })
    }

    pub fn discard(&self, targets: &[DiscardTarget]) -> Result<String> {
        let paths: Vec<String> = targets.iter().map(|t| t.path().to_string()).collect();
        self.record(OpKind::Discard, format!("Discard {}", describe_paths(&paths)), |r, backups| {
            let mut commands = Vec::new();
            for t in targets {
                r.backup_path(t.path(), backups)?;
                match t {
                    DiscardTarget::Worktree { path } => {
                        commands.push(r.exec(&["restore", "--worktree", "--", path])?);
                    }
                    DiscardTarget::Untracked { path } => {
                        let full = r.root().join(path);
                        if full.is_dir() { fs::remove_dir_all(&full)? } else if full.exists() { fs::remove_file(&full)? }
                        commands.push(format!("rm -r {path}"));
                    }
                    DiscardTarget::Staged { path, orig, index_status } => match (index_status, orig) {
                        ('A', _) | ('R', _) | ('C', _) => {
                            commands.push(r.exec(&["rm", "--cached", "-f", "-q", "--", path])?);
                            let full = r.root().join(path);
                            if full.is_file() {
                                fs::remove_file(&full)?;
                            }
                            if let (Some(o), 'R') = (orig, index_status) {
                                r.backup_path(o, backups)?;
                                commands.push(r.exec(&["restore", "--source=HEAD", "--staged", "--worktree", "--", o])?);
                            }
                        }
                        _ => commands.push(r.exec(&["restore", "--source=HEAD", "--staged", "--worktree", "--", path])?),
                    },
                }
            }
            Ok(commands.join(" && "))
        })
    }

    pub fn commit(&self, message: &str, amend: bool) -> Result<String> {
        let mut args = vec!["commit", "-F", "-"];
        if amend {
            args.push("--amend");
        }
        let summary = message.lines().next().unwrap_or_default().to_string();
        let what = if amend { "Amend commit" } else { "Commit" };
        self.record(OpKind::Commit, format!("{what} \"{summary}\""), |r, _| {
            r.git.run_input(&args, Some(message.as_bytes()))?;
            let mut shown = vec!["commit", "-m", summary.as_str()];
            if amend {
                shown.push("--amend");
            }
            Ok(display_command(&shown))
        })
    }

    pub fn switch(&self, name: &str) -> Result<String> {
        self.record(OpKind::Checkout, format!("Switch to {name}"), |r, _| r.exec(&["switch", name]))
    }

    /// Create a local branch tracking `remote_branch` (e.g. `origin/feat`) and switch to it.
    pub fn switch_track(&self, remote_branch: &str) -> Result<String> {
        self.record(OpKind::Checkout, format!("Switch to {remote_branch}"), |r, _| {
            r.exec(&["switch", "--track", remote_branch])
        })
    }

    pub fn create_branch(&self, name: &str, start: Option<&str>) -> Result<String> {
        let mut args = vec!["switch", "-c", name];
        if let Some(s) = start {
            args.push(s);
        }
        self.record(OpKind::Checkout, format!("Create branch {name}"), |r, _| r.exec(&args))
    }

    pub fn delete_branch(&self, name: &str, force: bool) -> Result<String> {
        let flag = if force { "-D" } else { "-d" };
        self.record(OpKind::Refs, format!("Delete branch {name}"), |r, _| r.exec(&["branch", flag, name]))
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<String> {
        self.record(OpKind::Refs, format!("Rename branch {old} → {new}"), |r, _| r.exec(&["branch", "-m", old, new]))
    }

    /// Add a pattern to the root .gitignore and stop tracking matching paths.
    pub fn apply_ignore(&self, pattern: &str, untrack: &[String]) -> Result<String> {
        self.add_ignore_rule(".gitignore", pattern, untrack)
    }

    /// `git lfs track` the patterns and convert the current versions of `convert`.
    pub fn lfs_track(&self, patterns: &[String], convert: &[String]) -> Result<String> {
        self.record(OpKind::Edit, format!("Track {} with LFS", patterns.join(" ")), |r, backups| {
            r.backup_path(".gitattributes", backups)?;
            let mut args = vec!["lfs", "track"];
            args.extend(patterns.iter().map(String::as_str));
            let mut commands = vec![r.exec(&args)?];
            commands.push(r.exec(&["add", ".gitattributes"])?);
            if !convert.is_empty() {
                let mut add = vec!["add", "--renormalize", "--"];
                add.extend(convert.iter().map(String::as_str));
                r.git.run(&add)?;
                commands.push(format!("git add --renormalize -- ({} files)", convert.len()));
            }
            Ok(commands.join(" && "))
        })
    }

    pub fn fetch(&self, on_line: impl FnMut(&str)) -> Result<String> {
        let args = ["fetch", "--all", "--prune", "--progress"];
        self.git.run_streaming(&args, on_line)?;
        Ok(display_command(&args))
    }

    pub fn pull(&self, on_line: impl FnMut(&str)) -> Result<String> {
        self.pull_tracked(on_line)
    }

    /// Push the current branch; `set_upstream` = `(remote, branch)` when it has none yet.
    pub fn push(&self, set_upstream: Option<(&str, &str)>, on_line: impl FnMut(&str)) -> Result<String> {
        let mut args = vec!["push", "--progress"];
        if let Some((remote, branch)) = set_upstream {
            args.extend(["-u", remote, branch]);
        }
        self.git.run_streaming(&args, on_line)?;
        Ok(display_command(&args))
    }
}
