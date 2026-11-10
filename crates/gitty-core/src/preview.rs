//! What a plain folder holds, before it becomes a repository: project type,
//! size, which folders the recommended rules would leave out, and the large
//! files that belong in Git LFS. Also sets the repository up with those rules.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use ::ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::git::Git;
use crate::insight::{LFS_THRESHOLD, ext_pattern};
use crate::project::{self, Project};

/// Stop counting after this many files; the numbers then say "at least".
const MAX_FILES: usize = 200_000;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TopEntry {
    pub name: String,
    pub is_dir: bool,
    pub files: u64,
    pub bytes: u64,
    /// Left out by the recommended rules or an existing .gitignore; names who.
    pub ignored_by: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FolderPreview {
    pub root: PathBuf,
    pub projects: Vec<Project>,
    /// "Unity", "Node.js + Rust", or `None` when nothing was recognised.
    pub kind: Option<String>,
    pub files: u64,
    pub bytes: u64,
    /// What git would track with the recommended rules applied.
    pub kept_files: u64,
    pub kept_bytes: u64,
    /// Top-level files and folders, largest first.
    pub top: Vec<TopEntry>,
    /// Kept files that belong in Git LFS (path, size), largest first.
    pub big: Vec<(String, u64)>,
    /// `.gitignore` lines the project types recommend that aren't there yet.
    pub ignore_rules: Vec<String>,
    /// `git lfs track` patterns for the large files.
    pub lfs_patterns: Vec<String>,
    pub lfs_available: bool,
    pub has_gitignore: bool,
    /// The scan stopped at its limit; sizes and counts are lower bounds.
    pub truncated: bool,
}

impl FolderPreview {
    pub fn is_empty(&self) -> bool {
        self.files == 0
    }

    pub fn kind_label(&self) -> Option<String> {
        self.kind.clone()
    }
}

/// List every file under `root` (paths with `/`), up to `MAX_FILES`.
fn walk(root: &Path) -> (Vec<(String, u64)>, bool) {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            // symlinks are not followed: they can loop or leave the folder
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            if meta.is_dir() {
                if entry.file_name() != ".git" {
                    stack.push(path);
                }
            } else if meta.is_file() {
                let Ok(rel) = path.strip_prefix(root) else { continue };
                files.push((rel.to_string_lossy().replace('\\', "/"), meta.len()));
                if files.len() >= MAX_FILES {
                    return (files, true);
                }
            }
        }
    }
    (files, false)
}

/// The anchored rules each detected project recommends ("/Library/").
fn recommended_rules(projects: &[Project]) -> Vec<(String, &'static str)> {
    let mut rules = Vec::new();
    for p in projects {
        for r in p.kind.ignore_rules() {
            let rule = format!("/{}{r}", p.dir);
            if !rules.iter().any(|(x, _)| *x == rule) {
                rules.push((rule, p.kind.name()));
            }
        }
    }
    rules
}

fn matcher(root: &Path, lines: &[String], existing: bool) -> Gitignore {
    let mut b = GitignoreBuilder::new(root);
    if existing {
        b.add(root.join(".gitignore"));
    }
    for l in lines {
        let _ = b.add_line(None, l);
    }
    b.build().unwrap_or_else(|_| Gitignore::empty())
}

/// Look through `root` (not a repository yet).
pub fn preview(root: &Path) -> FolderPreview {
    let (files, truncated) = walk(root);
    let projects = project::detect(root, &files);
    let has_gitignore = root.join(".gitignore").is_file();
    let existing_text = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    let existing_lines: HashSet<&str> = existing_text.lines().map(str::trim).collect();
    let rules = recommended_rules(&projects);
    let ignore_rules: Vec<String> = rules
        .iter()
        .map(|(r, _)| r.clone())
        .filter(|r| !existing_lines.contains(r.as_str()) && !existing_lines.contains(r.trim_start_matches('/')))
        .collect();
    let ours = matcher(root, &rules.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(), false);
    let theirs = matcher(root, &[], has_gitignore);
    // who leaves a path out: the project rules (named by kind) or the existing .gitignore
    let left_out = |rel: &str, is_dir: bool| -> Option<String> {
        let m = ours.matched_path_or_any_parents(rel, is_dir);
        if m.is_ignore() {
            let glob = m.inner().map(|g| g.original().to_string()).unwrap_or_default();
            let kind = rules.iter().find(|(r, _)| *r == glob).map(|(_, k)| *k).unwrap_or("project");
            return Some(format!("{kind} rules"));
        }
        if has_gitignore && theirs.matched_path_or_any_parents(rel, is_dir).is_ignore() {
            return Some(".gitignore".into());
        }
        None
    };

    let mut top: BTreeMap<String, TopEntry> = BTreeMap::new();
    let (mut bytes, mut kept_files, mut kept_bytes) = (0u64, 0u64, 0u64);
    let mut kept: Vec<(String, u64)> = Vec::new();
    for (path, size) in &files {
        bytes += size;
        let (first, rest) = path.split_once('/').map(|(a, b)| (a, Some(b))).unwrap_or((path.as_str(), None));
        let e = top.entry(first.to_string()).or_insert_with(|| TopEntry {
            name: first.to_string(),
            is_dir: rest.is_some(),
            files: 0,
            bytes: 0,
            ignored_by: left_out(first, rest.is_some()),
        });
        e.files += 1;
        e.bytes += size;
        if e.ignored_by.is_none() && left_out(path, false).is_none() {
            kept_files += 1;
            kept_bytes += size;
            kept.push((path.clone(), *size));
        }
    }
    let engine: HashSet<&str> = projects.iter().flat_map(|p| p.kind.lfs_patterns().iter().copied()).collect();
    let mut big: Vec<(String, u64)> = kept
        .into_iter()
        .filter(|(p, s)| *s >= LFS_THRESHOLD || (*s >= 1024 * 1024 && ext_pattern(p).is_some_and(|e| engine.contains(e.as_str()))))
        .collect();
    big.sort_by_key(|b| std::cmp::Reverse(b.1));
    let mut lfs_patterns: Vec<String> = big.iter().filter_map(|(p, _)| ext_pattern(p)).collect();
    lfs_patterns.sort();
    lfs_patterns.dedup();
    let mut top: Vec<TopEntry> = top.into_values().collect();
    top.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    let mut names: Vec<&str> = projects.iter().map(|p| p.kind.name()).collect();
    names.dedup();
    FolderPreview {
        root: root.to_path_buf(),
        kind: (!names.is_empty()).then(|| names.join(" + ")),
        projects,
        files: files.len() as u64,
        bytes,
        kept_files,
        kept_bytes,
        top,
        big,
        ignore_rules,
        lfs_patterns,
        lfs_available: Git::new(root).succeeds(&["lfs", "version"]),
        has_gitignore,
        truncated,
    }
}

/// What to add when setting git up in a folder.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct InitOptions {
    /// Lines to append to .gitignore (missing ones only).
    pub ignore_rules: Vec<String>,
    /// Patterns to `git lfs track` (needs Git LFS).
    pub lfs_patterns: Vec<String>,
}

/// `git init` in `path`, then add the chosen ignore rules and LFS patterns.
/// Nothing is staged or committed.
pub fn init_with(path: &Path, opts: &InitOptions) -> crate::Result<PathBuf> {
    let root = crate::workspace::init_repo(path)?;
    if !opts.ignore_rules.is_empty() {
        let file = root.join(".gitignore");
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        let have: HashSet<String> = text.lines().map(|l| l.trim().to_string()).collect();
        let add: Vec<&String> = opts.ignore_rules.iter().filter(|r| !have.contains(r.as_str())).collect();
        if !add.is_empty() {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str("# Generated and cache folders (added by gitty)\n");
            for r in add {
                text.push_str(r);
                text.push('\n');
            }
            std::fs::write(&file, text)?;
        }
    }
    if !opts.lfs_patterns.is_empty() {
        let git = Git::new(&root);
        git.run(&["lfs", "install", "--local"])?;
        let mut args = vec!["lfs", "track"];
        args.extend(opts.lfs_patterns.iter().map(String::as_str));
        git.run(&args)?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, bytes: usize) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; bytes]).unwrap();
    }

    fn unity_folder() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(r, "ProjectSettings/ProjectVersion.txt", 0);
        std::fs::write(r.join("ProjectSettings/ProjectVersion.txt"), "m_EditorVersion: 6000.0.1f1\n").unwrap();
        write(r, "Packages/manifest.json", 20);
        write(r, "Assets/Scripts/Player.cs", 300);
        write(r, "Assets/Art/hero.psd", 2 * 1024 * 1024);
        write(r, "Library/ArtifactDB", 500_000);
        write(r, "Library/cache/a.bin", 500_000);
        write(r, "Temp/x.tmp", 1000);
        d
    }

    #[test]
    fn previews_a_unity_folder() {
        let d = unity_folder();
        let p = preview(d.path());
        assert_eq!(p.kind_label().as_deref(), Some("Unity"));
        assert_eq!(p.files, 7);
        let lib = p.top.iter().find(|t| t.name == "Library").unwrap();
        assert!(lib.is_dir && lib.ignored_by.as_deref() == Some("Unity rules"), "{lib:?}");
        assert_eq!(lib.files, 2);
        assert!(p.top.iter().find(|t| t.name == "Assets").unwrap().ignored_by.is_none());
        // Library and Temp are left out of what git would track
        assert_eq!(p.kept_files, 4);
        assert!(p.kept_bytes < p.bytes - 1_000_000);
        assert!(p.ignore_rules.contains(&"/Library/".to_string()));
        assert_eq!(p.big.len(), 1, "the 2 MB .psd is an LFS candidate in a Unity project");
        assert_eq!(p.lfs_patterns, vec!["*.psd".to_string()]);
        assert!(!p.truncated);
    }

    #[test]
    fn existing_gitignore_is_respected_and_not_duplicated() {
        let d = unity_folder();
        std::fs::write(d.path().join(".gitignore"), "/Library/\nnotes.txt\n").unwrap();
        write(d.path(), "notes.txt", 10);
        let p = preview(d.path());
        assert!(p.has_gitignore);
        assert!(!p.ignore_rules.contains(&"/Library/".to_string()), "already there");
        assert!(p.ignore_rules.contains(&"/Temp/".to_string()));
        assert_eq!(p.top.iter().find(|t| t.name == "notes.txt").unwrap().ignored_by.as_deref(), Some(".gitignore"));

        init_with(d.path(), &InitOptions { ignore_rules: p.ignore_rules.clone(), lfs_patterns: vec![] }).unwrap();
        let text = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert!(text.starts_with("/Library/\nnotes.txt\n"), "existing lines kept: {text}");
        assert_eq!(text.matches("/Library/").count(), 1);
        assert!(text.contains("/Temp/"));
        assert!(d.path().join(".git").exists());
        let status = std::process::Command::new("git").current_dir(d.path()).args(["status", "--porcelain"]).output().unwrap();
        let status = String::from_utf8_lossy(&status.stdout);
        assert!(!status.contains("Library") && !status.contains("Temp"), "{status}");
        assert!(status.contains("?? Assets/"), "nothing staged: {status}");
    }

    #[test]
    fn empty_and_unknown_folders() {
        let d = tempfile::tempdir().unwrap();
        let p = preview(d.path());
        assert!(p.is_empty() && p.kind_label().is_none() && p.ignore_rules.is_empty());
        write(d.path(), "notes/todo.txt", 5);
        let p = preview(d.path());
        assert!(!p.is_empty() && p.kind_label().is_none());
        assert_eq!(p.top[0].name, "notes");
    }
}
