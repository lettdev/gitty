//! Refresh when files change on disk, ignoring paths covered by .gitignore
//! (a Unity `Library/` folder or `node_modules/` would otherwise cause a
//! refresh storm).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use gitty_core::Repo;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::worker::Msg;

struct Filter {
    roots: Vec<PathBuf>,
    git_dirs: Vec<PathBuf>,
    ignore: Gitignore,
}

fn with_canonical(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![p.to_path_buf()];
    if let Ok(c) = p.canonicalize() {
        if c != p {
            v.push(c);
        }
    }
    v
}

impl Filter {
    fn relevant(&self, p: &Path) -> bool {
        for gd in &self.git_dirs {
            if let Ok(rel) = p.strip_prefix(gd) {
                let s = rel.to_string_lossy().replace('\\', "/");
                if s.ends_with(".lock") || s.starts_with("gitty/") {
                    return false;
                }
                return matches!(s.as_str(), "HEAD" | "index" | "MERGE_HEAD" | "ORIG_HEAD" | "packed-refs" | "FETCH_HEAD")
                    || s.starts_with("refs/");
            }
        }
        for root in &self.roots {
            if let Ok(rel) = p.strip_prefix(root) {
                if rel.as_os_str().is_empty() {
                    return true;
                }
                return !self.ignore.matched_path_or_any_parents(rel, p.is_dir()).is_ignore();
            }
        }
        false
    }
}

pub fn start(repo: &Repo, tx: Sender<Msg>) -> Option<RecommendedWatcher> {
    let root = repo.root().to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let mut builder = GitignoreBuilder::new(&root);
    builder.add(root.join(".gitignore"));
    builder.add(git_dir.join("info").join("exclude"));
    let filter = Filter {
        roots: with_canonical(&root),
        git_dirs: with_canonical(&git_dir),
        ignore: builder.build().unwrap_or_else(|_| Gitignore::empty()),
    };
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        if ev.paths.iter().any(|p| filter.relevant(p)) {
            let _ = tx.send(Msg::FsChanged);
        }
    })
    .ok()?;
    watcher.watch(&root, RecursiveMode::Recursive).ok()?;
    if !git_dir.starts_with(&root) {
        // linked worktree: its git dir lives elsewhere
        let _ = watcher.watch(&git_dir, RecursiveMode::Recursive);
    }
    Some(watcher)
}
