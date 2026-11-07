//! Operation log: every mutating gitty operation records the repository state
//! before and after, so the last operation can be undone.
//!
//! Stored as JSON lines in `<git-dir>/gitty/oplog.jsonl`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{Error, Result};
use crate::repo::{Repo, RepoState};

const KEEP_ENTRIES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum HeadState {
    Branch(String),
    Detached(String),
    /// A branch with no commits yet.
    Unborn(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub head: HeadState,
    /// Local branches and tags: full ref name → object id.
    pub refs: BTreeMap<String, String>,
    /// Tree written from the index; `None` when the index has conflicts.
    pub index_tree: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// Only the index changed (stage/unstage).
    Index,
    Commit,
    /// Branch refs changed without touching the worktree.
    Refs,
    /// HEAD moved to another branch/commit and the worktree followed.
    Checkout,
    /// The current branch moved and the worktree followed (pull/merge).
    Pull,
    /// Worktree files were discarded; their content is kept as blobs.
    Discard,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBackup {
    pub path: String,
    /// Blob holding the content before the operation; `None` = file did not exist.
    pub before: Option<String>,
    /// Content hash right after the operation, to detect later edits.
    pub after: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpEntry {
    pub time: i64,
    pub kind: OpKind,
    pub description: String,
    pub command: String,
    pub before: Snapshot,
    pub after: Snapshot,
    #[serde(default)]
    pub backups: Vec<FileBackup>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Repo {
    fn oplog_path(&self) -> PathBuf {
        self.git_dir().join("gitty").join("oplog.jsonl")
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let head = match self.git.run_text(&["symbolic-ref", "-q", "HEAD"]) {
            Ok(r) => {
                let r = r.trim();
                let name = r.strip_prefix("refs/heads/").unwrap_or(r).to_string();
                if self.head_is_unborn() { HeadState::Unborn(name) } else { HeadState::Branch(name) }
            }
            Err(_) => HeadState::Detached(self.git.run_text(&["rev-parse", "HEAD"])?.trim().to_string()),
        };
        let mut refs = BTreeMap::new();
        let out = self.git.run_text(&["for-each-ref", "--format=%(refname) %(objectname)", "refs/heads", "refs/tags"])?;
        for line in out.lines() {
            if let Some((r, o)) = line.rsplit_once(' ') {
                refs.insert(r.to_string(), o.to_string());
            }
        }
        let index_tree = self.git.run_text(&["write-tree"]).ok().map(|t| t.trim().to_string());
        Ok(Snapshot { head, refs, index_tree })
    }

    pub fn oplog_entries(&self) -> Vec<OpEntry> {
        let Ok(text) = fs::read_to_string(self.oplog_path()) else { return Vec::new() };
        text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    pub fn last_op(&self) -> Option<OpEntry> {
        self.oplog_entries().pop()
    }

    fn oplog_write(&self, entries: &[OpEntry]) -> Result<()> {
        let path = self.oplog_path();
        fs::create_dir_all(path.parent().expect("oplog has a parent"))?;
        let start = entries.len().saturating_sub(KEEP_ENTRIES);
        let mut text = String::new();
        for e in &entries[start..] {
            text.push_str(&serde_json::to_string(e)?);
            text.push('\n');
        }
        let tmp = path.with_extension("jsonl.tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn oplog_push(&self, entry: OpEntry) -> Result<()> {
        let path = self.oplog_path();
        fs::create_dir_all(path.parent().expect("oplog has a parent"))?;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(f, "{}", serde_json::to_string(&entry)?)?;
        drop(f);
        let entries = self.oplog_entries();
        if entries.len() > KEEP_ENTRIES * 2 {
            self.oplog_write(&entries)?;
        }
        Ok(())
    }

    /// Run a mutating operation and record it in the operation log.
    /// `f` returns the command line to display; it may push file backups.
    pub fn record(
        &self,
        kind: OpKind,
        description: impl Into<String>,
        f: impl FnOnce(&Repo, &mut Vec<FileBackup>) -> Result<String>,
    ) -> Result<String> {
        let before = self.snapshot()?;
        let mut backups = Vec::new();
        let result = f(self, &mut backups);
        if let Ok(after) = self.snapshot() {
            for b in &mut backups {
                b.after = self.hash_file(&b.path).unwrap_or(None);
            }
            let changed = after != before || backups.iter().any(|b| b.before != b.after);
            if changed {
                let command = result.as_ref().cloned().unwrap_or_default();
                let _ = self.oplog_push(OpEntry {
                    time: now(),
                    kind,
                    description: description.into(),
                    command,
                    before,
                    after,
                    backups,
                });
            }
        }
        result
    }

    /// Content hash of a worktree file, without applying filters (LFS, eol).
    pub(crate) fn hash_file(&self, path: &str) -> Result<Option<String>> {
        if !self.root().join(path).is_file() {
            return Ok(None);
        }
        Ok(Some(self.git.run_text(&["hash-object", "--no-filters", "--", path])?.trim().to_string()))
    }

    /// Store a file's current content as a blob so it can be restored.
    pub(crate) fn backup_path(&self, path: &str, out: &mut Vec<FileBackup>) -> Result<()> {
        let full = self.root().join(path);
        if full.is_dir() {
            for file in walk_files(&full) {
                if let Ok(rel) = file.strip_prefix(self.root()) {
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    self.backup_path(&rel, out)?;
                }
            }
            return Ok(());
        }
        let before = if full.is_file() {
            Some(self.git.run_text(&["hash-object", "-w", "--no-filters", "--", path])?.trim().to_string())
        } else {
            None
        };
        out.push(FileBackup { path: path.trim_end_matches('/').to_string(), before, after: None });
        Ok(())
    }

    fn restore_backup(&self, b: &FileBackup) -> Result<()> {
        let full = self.root().join(&b.path);
        match &b.before {
            Some(oid) => {
                let bytes = self.git.run(&["cat-file", "blob", oid])?;
                if let Some(parent) = full.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&full, bytes)?;
            }
            None => {
                if full.is_file() {
                    fs::remove_file(&full)?;
                }
            }
        }
        Ok(())
    }

    fn restore_refs(&self, target: &BTreeMap<String, String>) -> Result<()> {
        let current = self.snapshot()?.refs;
        let mut script = String::new();
        for (r, oid) in target {
            if current.get(r) != Some(oid) {
                script.push_str(&format!("update {r} {oid}\n"));
            }
        }
        for r in current.keys() {
            if !target.contains_key(r) {
                script.push_str(&format!("delete {r}\n"));
            }
        }
        if !script.is_empty() {
            self.git.run_input(&["update-ref", "--stdin"], Some(script.as_bytes()))?;
        }
        Ok(())
    }

    fn restore_head(&self, head: &HeadState) -> Result<()> {
        match head {
            HeadState::Branch(n) | HeadState::Unborn(n) => {
                self.git.run(&["symbolic-ref", "HEAD", &format!("refs/heads/{n}")])?;
            }
            HeadState::Detached(oid) => {
                self.git.run(&["update-ref", "--no-deref", "HEAD", oid])?;
            }
        }
        Ok(())
    }

    fn head_oid(snapshot: &Snapshot) -> Option<String> {
        match &snapshot.head {
            HeadState::Branch(n) => snapshot.refs.get(&format!("refs/heads/{n}")).cloned(),
            HeadState::Detached(o) => Some(o.clone()),
            HeadState::Unborn(_) => None,
        }
    }

    /// Undo the most recent recorded operation. Refuses when the repository
    /// changed since, so nothing made outside gitty is thrown away.
    pub fn undo_last(&self) -> Result<String> {
        let mut entries = self.oplog_entries();
        let Some(e) = entries.pop() else {
            return Err(Error::Msg("Nothing to undo.".into()));
        };
        let refuse = |why: &str| {
            Err(Error::Msg(format!(
                "Can't undo \"{}\": {why}\nUse `git reflog` to recover manually.",
                e.description
            )))
        };
        let now = self.snapshot()?;
        if now.head != e.after.head || now.refs != e.after.refs {
            return refuse("branches or HEAD changed after it.");
        }
        let checks_index = !matches!(e.kind, OpKind::Checkout | OpKind::Pull);
        if checks_index && e.after.index_tree.is_some() && now.index_tree != e.after.index_tree {
            return refuse("the staging area changed after it.");
        }
        for b in &e.backups {
            if self.hash_file(&b.path)? != b.after {
                return refuse(&format!("{} was edited after it.", b.path));
            }
        }

        match e.kind {
            OpKind::Checkout => {
                match &e.before.head {
                    HeadState::Branch(n) => self.git.run(&["switch", n]).map(|_| ())?,
                    HeadState::Detached(o) => self.git.run(&["switch", "--detach", o]).map(|_| ())?,
                    HeadState::Unborn(_) => return refuse("the previous branch had no commits."),
                }
                self.restore_refs(&e.before.refs)?;
            }
            OpKind::Pull => {
                match self.state() {
                    RepoState::Merging => self.git.run(&["merge", "--abort"]).map(|_| ())?,
                    RepoState::Rebasing => self.git.run(&["rebase", "--abort"]).map(|_| ())?,
                    _ => {}
                }
                if let Some(oid) = Self::head_oid(&e.before) {
                    self.git.run(&["reset", "--keep", &oid])?;
                }
                self.restore_refs(&e.before.refs)?;
            }
            OpKind::Index | OpKind::Commit | OpKind::Refs | OpKind::Discard => {
                self.restore_refs(&e.before.refs)?;
                self.restore_head(&e.before.head)?;
                if let Some(tree) = &e.before.index_tree {
                    // read-tree drops cached stat info, so only do it when needed
                    if e.after.index_tree.as_ref() != Some(tree) {
                        self.git.run(&["read-tree", tree])?;
                    }
                }
                for b in &e.backups {
                    self.restore_backup(b)?;
                }
            }
        }
        self.oplog_write(&entries)?;
        Ok(e.description)
    }
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                out.push(entry.path());
            }
        }
    }
    out
}
