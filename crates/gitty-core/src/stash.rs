use crate::diff::FileDiff;
use crate::git::{Error, Result, display_command};
use crate::log::CommitFile;
use crate::oplog::{OpEntry, OpKind, StashRef, now};
use crate::repo::Repo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{0}`
    pub name: String,
    pub oid: String,
    pub time: i64,
    pub message: String,
}

impl Repo {
    pub fn stashes(&self) -> Result<Vec<Stash>> {
        let out = self.git.run_text(&["stash", "list", "--format=%gd%x1f%H%x1f%ct%x1f%gs"])?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.splitn(4, '\x1f').collect();
                (f.len() == 4).then(|| Stash {
                    name: f[0].to_string(),
                    oid: f[1].to_string(),
                    time: f[2].parse().unwrap_or(0),
                    message: f[3].to_string(),
                })
            })
            .collect())
    }

    fn stash_top(&self) -> Option<String> {
        self.git.run_text(&["rev-parse", "-q", "--verify", "refs/stash"]).ok().map(|s| s.trim().to_string())
    }

    pub fn stash_push(&self, message: &str, include_untracked: bool) -> Result<String> {
        let before = self.snapshot()?;
        let top_before = self.stash_top();
        let mut args = vec!["stash", "push"];
        if include_untracked {
            args.push("--include-untracked");
        }
        if !message.is_empty() {
            args.extend(["-m", message]);
        }
        self.git.run(&args)?;
        let top = self.stash_top();
        if top == top_before {
            return Err(Error::Msg("Nothing to stash — the working tree is clean.".into()));
        }
        let after = self.snapshot()?;
        let oid = top.unwrap_or_default();
        let msg = self.git.run_text(&["log", "-1", "--format=%gs", "-g", "refs/stash"]).unwrap_or_default().trim().to_string();
        let _ = self.oplog_push(OpEntry {
            time: now(),
            kind: OpKind::StashPush,
            description: format!("Stash \"{}\"", if message.is_empty() { "changes" } else { message }),
            command: display_command(&args),
            before,
            after,
            backups: Vec::new(),
            stash: Some(StashRef { oid, message: msg }),
        });
        Ok(display_command(&args))
    }

    /// Apply a stash; `pop` also removes it when the apply succeeds.
    pub fn stash_apply(&self, name: &str, pop: bool) -> Result<String> {
        let args = ["stash", if pop { "pop" } else { "apply" }, "--index", name];
        match self.git.run(&args) {
            Ok(_) => Ok(display_command(&args)),
            // --index fails when the staged state can't be restored; fall back
            Err(Error::Command { stderr, .. }) if stderr.contains("--index") || stderr.contains("Conflicts in index") => {
                let plain = ["stash", if pop { "pop" } else { "apply" }, name];
                self.git.run(&plain)?;
                Ok(display_command(&plain))
            }
            Err(e) => Err(e),
        }
    }

    pub fn stash_drop(&self, stash: &Stash) -> Result<String> {
        let before = self.snapshot()?;
        let args = ["stash", "drop", stash.name.as_str()];
        self.git.run(&args)?;
        let after = self.snapshot()?;
        let _ = self.oplog_push(OpEntry {
            time: now(),
            kind: OpKind::StashDrop,
            description: format!("Drop stash \"{}\"", stash.message),
            command: display_command(&args),
            before,
            after,
            backups: Vec::new(),
            stash: Some(StashRef { oid: stash.oid.clone(), message: stash.message.clone() }),
        });
        Ok(display_command(&args))
    }

    /// Files in a stash, including untracked files saved with it.
    pub fn stash_files(&self, oid: &str) -> Result<Vec<CommitFile>> {
        let base = format!("{oid}^1");
        let names = self.git.run_text(&["diff", "--no-color", "-M", "-z", "--name-status", &base, oid])?;
        let mut files = Vec::new();
        let mut it = names.split('\0').filter(|s| !s.is_empty());
        while let Some(st) = it.next() {
            let status = st.chars().next().unwrap_or('M');
            let (orig, path) = if matches!(status, 'R' | 'C') {
                (it.next().map(str::to_string), it.next().unwrap_or_default().to_string())
            } else {
                (None, it.next().unwrap_or_default().to_string())
            };
            files.push(CommitFile { status, path, orig, adds: None, dels: None });
        }
        let untracked = format!("{oid}^3");
        if self.git.succeeds(&["rev-parse", "-q", "--verify", &untracked]) {
            let out = self.git.run_text(&["ls-tree", "-r", "-z", "--name-only", &untracked])?;
            for p in out.split('\0').filter(|s| !s.is_empty()) {
                files.push(CommitFile { status: '?', path: p.to_string(), orig: None, adds: None, dels: None });
            }
        }
        Ok(files)
    }

    pub fn stash_file_diff(&self, oid: &str, file: &CommitFile) -> Result<FileDiff> {
        if file.status == '?' {
            return self.diff_commit_file(&format!("{oid}^3"), None, &file.path, None);
        }
        self.diff_commit_file(oid, Some(&format!("{oid}^1")), &file.path, file.orig.as_deref())
    }
}
