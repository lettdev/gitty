//! Find repositories under a few folders and summarize each one.

use std::path::{Path, PathBuf};

use crate::repo::Repo;
use crate::status;

const SKIP: &[&str] = &["node_modules", "target", "Library", "build", "dist", "vendor", "Pods", "DerivedData"];

#[derive(Clone, Debug, PartialEq, Eq)]
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
