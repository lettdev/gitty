//! Find repositories under a few folders and summarize each one.

use std::path::{Path, PathBuf};

use crate::repo::Repo;
use crate::status;

const SKIP: &[&str] = &["node_modules", "target", "Library", "build", "dist", "vendor", "Pods", "DerivedData"];

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RepoSummary {
    pub path: PathBuf,
    pub name: String,
    pub branch: Option<String>,
    pub changes: usize,
    pub conflicts: usize,
    pub upstream: bool,
    pub ahead: u32,
    pub behind: u32,
    pub last_commit: Option<i64>,
    pub error: Option<String>,
}

/// The folder name `git clone` would pick for `url`.
pub fn clone_dir_name(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let name = trimmed.rsplit(['/', ':', '\\']).next().unwrap_or("repo");
    if name.is_empty() { "repo".into() } else { name.to_string() }
}

/// Clone `url` into `parent/name`, reporting progress. Returns the new folder.
pub fn clone_repo(url: &str, parent: &Path, name: &str, on_line: impl FnMut(&str)) -> crate::Result<PathBuf> {
    let target = parent.join(name);
    if target.exists() && std::fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Err(crate::Error::Msg(format!("{} already exists and isn't empty.", target.display())));
    }
    std::fs::create_dir_all(parent)?;
    let git = crate::git::Git::new(parent);
    git.run_streaming(&["clone", "--progress", "--", url.trim(), name], on_line)?;
    Ok(target)
}

/// Create a new repository (with an initial `main` branch) in `path`.
pub fn init_repo(path: &Path) -> crate::Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    if path.join(".git").exists() {
        return Err(crate::Error::Msg(format!("{} is already a git repository.", path.display())));
    }
    crate::git::Git::new(path).run(&["init", "-b", "main"])?;
    Ok(path.to_path_buf())
}

/// A reason to think twice before running `git init` in `path`: the drive
/// root, the home folder, or a folder already inside another repository.
pub fn init_warning(path: &Path) -> Option<String> {
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if p.parent().is_none() {
        return Some("This is the top of the drive. Tracking it would put every file on the disk under git.".into());
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if home.and_then(|h| h.canonicalize().ok()).is_some_and(|h| h == p) {
        return Some("This is your home folder. Tracking it would put all your personal files under git — usually you want a project folder inside it.".into());
    }
    if let Ok(outer) = crate::Repo::discover(&p) {
        let outer_root = outer.root().canonicalize().unwrap_or_else(|_| outer.root().to_path_buf());
        if outer_root != p {
            return Some(format!(
                "This folder is already inside the repository at {}. A new repository here would be nested in it.",
                outer.root().display()
            ));
        }
    }
    None
}

/// Repositories under `roots`, at most `max_depth` folders deep.
/// Does not descend into a repository once found.
pub fn find_repos(roots: &[PathBuf], max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        if dir.join(".git").exists() {
            out.push(dir);
            continue;
        }
        if depth >= max_depth {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn summarize(path: &Path) -> RepoSummary {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    let mut s = RepoSummary {
        path: path.to_path_buf(),
        name,
        branch: None,
        changes: 0,
        conflicts: 0,
        upstream: false,
        ahead: 0,
        behind: 0,
        last_commit: None,
        error: None,
    };
    let repo = match Repo::discover(path) {
        Ok(r) => r,
        Err(e) => {
            s.error = Some(e.to_string());
            return s;
        }
    };
    match repo.git.run(&["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=normal"]) {
        Ok(out) => {
            let st = status::parse(&out);
            s.branch = st.branch.head.clone().or_else(|| st.branch.oid.as_ref().map(|o| format!("@{}", &o[..o.len().min(7)])));
            s.conflicts = st.entries.iter().filter(|e| e.is_conflict()).count();
            s.changes = st.entries.len();
            s.upstream = st.branch.upstream.is_some();
            s.ahead = st.branch.ahead;
            s.behind = st.branch.behind;
        }
        Err(e) => s.error = Some(e.to_string()),
    }
    s.last_commit = repo.git.run_text(&["log", "-1", "--format=%ct"]).ok().and_then(|t| t.trim().parse().ok());
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_names() {
        assert_eq!(clone_dir_name("https://github.com/me/gitty.git"), "gitty");
        assert_eq!(clone_dir_name("git@github.com:me/My-Game.git/"), "My-Game");
        assert_eq!(clone_dir_name("/local/path/repo"), "repo");
    }

    #[test]
    fn clone_a_local_repo() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let run = |args: &[&str]| assert!(std::process::Command::new("git").current_dir(&src).args(args).status().unwrap().success());
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(src.join("a.txt"), "a").unwrap();
        run(&["add", "."]);
        run(&["-c", "user.name=T", "-c", "user.email=t@e.com", "commit", "-q", "-m", "x"]);
        let mut lines = 0;
        let out = clone_repo(src.to_str().unwrap(), &d.path().join("dest"), "copy", |_| lines += 1).unwrap();
        assert!(out.join("a.txt").exists());
        assert!(clone_repo(src.to_str().unwrap(), &d.path().join("dest"), "copy", |_| {}).is_err(), "won't clone over a folder");
    }

    #[test]
    fn init_creates_a_repo_once() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("New Game");
        init_repo(&p).unwrap();
        assert!(p.join(".git").exists());
        assert!(init_repo(&p).is_err());
    }

    #[test]
    fn init_warnings() {
        let d = tempfile::tempdir().unwrap();
        let plain = d.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(init_warning(&plain), None);
        assert!(init_warning(Path::new("/")).is_some());
        init_repo(&plain).unwrap();
        let inner = plain.join("sub");
        std::fs::create_dir_all(&inner).unwrap();
        assert!(init_warning(&inner).unwrap().contains("inside the repository"));
    }

    #[test]
    fn finds_nested_repos_but_not_inside_repos() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        for p in ["a/.git", "group/b/.git", "a/inner/.git", "node_modules/c/.git", "x/y/z/deep/.git"] {
            std::fs::create_dir_all(r.join(p)).unwrap();
        }
        let found = find_repos(&[r.to_path_buf()], 2);
        let names: Vec<String> = found.iter().map(|p| p.strip_prefix(r).unwrap().display().to_string().replace('\\', "/")).collect();
        assert_eq!(names, vec!["a", "group/b"]);
    }
}
