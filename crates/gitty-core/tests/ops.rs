//! End-to-end tests against real temporary repositories.

use std::fs;
use std::path::Path;
use std::process::Command;

use gitty_core::diff::{LineKind, build_patch};
use gitty_core::log::LogQuery;
use gitty_core::ops::DiscardTarget;
use gitty_core::{Error, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).args(args).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn setup() -> (tempfile::TempDir, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "core.autocrlf", "false"]);
    git(p, &["config", "user.name", "Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
    fs::write(p.join("f.txt"), lines.join("\n") + "\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "initial"]);
    let repo = Repo::discover(p).unwrap();
    (dir, repo)
}

/// Change line 2 and line 18 so the diff has two hunks.
fn edit_two_places(p: &Path) {
    let mut lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
    lines[1] = "line 2 changed".into();
    lines[17] = "line 18 changed".into();
    fs::write(p.join("f.txt"), lines.join("\n") + "\n").unwrap();
}

#[test]
fn discover_finds_root_from_subdir() {
    let (dir, _) = setup();
    fs::create_dir(dir.path().join("sub")).unwrap();
    let r = Repo::discover(&dir.path().join("sub")).unwrap();
    assert_eq!(r.root().canonicalize().unwrap(), dir.path().canonicalize().unwrap());
}

#[test]
fn stage_one_hunk_only() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    let d = repo.diff_worktree("f.txt").unwrap();
    assert_eq!(d.hunks.len(), 2);
    let patch = build_patch(&d, |h, _| h == 0, false).unwrap();
    repo.apply_to_index(&patch, false, "f.txt").unwrap();
    let cached = git(dir.path(), &["diff", "--cached"]);
    assert!(cached.contains("+line 2 changed"));
    assert!(!cached.contains("line 18 changed"));
    let unstaged = git(dir.path(), &["diff"]);
    assert!(unstaged.contains("+line 18 changed"));
}

#[test]
fn stage_single_added_line_of_a_replacement() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    let d = repo.diff_worktree("f.txt").unwrap();
    // only the "+line 2 changed" line, not the matching deletion
    let add = d.hunks[0].lines.iter().position(|l| l.kind == LineKind::Add).unwrap();
    let patch = build_patch(&d, |h, l| h == 0 && l == add, false).unwrap();
    repo.apply_to_index(&patch, false, "f.txt").unwrap();
    let staged = git(dir.path(), &["show", ":f.txt"]);
    assert!(staged.contains("line 2\nline 2 changed\nline 3"), "{staged}");
}

#[test]
fn unstage_one_hunk() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    repo.stage(&["f.txt".into()]).unwrap();
    let d = repo.diff_index("f.txt", None).unwrap();
    let patch = build_patch(&d, |h, _| h == 1, true).unwrap();
    repo.apply_to_index(&patch, true, "f.txt").unwrap();
    let cached = git(dir.path(), &["diff", "--cached"]);
    assert!(cached.contains("line 2 changed"));
    assert!(!cached.contains("line 18 changed"));
}

#[test]
fn undo_stage() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    repo.stage(&["f.txt".into()]).unwrap();
    assert!(!git(dir.path(), &["diff", "--cached"]).is_empty());
    repo.undo_last().unwrap();
    assert!(git(dir.path(), &["diff", "--cached"]).is_empty());
    assert!(matches!(repo.undo_last(), Err(Error::Msg(_))));
}

#[test]
fn undo_commit_keeps_changes_staged() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    repo.stage(&["f.txt".into()]).unwrap();
    let before = git(dir.path(), &["rev-parse", "HEAD"]);
    repo.commit("second\n\nbody", false).unwrap();
    assert_ne!(before, git(dir.path(), &["rev-parse", "HEAD"]));
    let desc = repo.undo_last().unwrap();
    assert!(desc.contains("second"));
    assert_eq!(before, git(dir.path(), &["rev-parse", "HEAD"]));
    assert!(git(dir.path(), &["diff", "--cached"]).contains("line 2 changed"));
}

#[test]
fn discard_hunk_then_undo_restores_file() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    let original = fs::read_to_string(dir.path().join("f.txt")).unwrap();
    let d = repo.diff_worktree("f.txt").unwrap();
    let patch = build_patch(&d, |h, _| h == 1, true).unwrap();
    repo.discard_patch(&patch, "f.txt").unwrap();
    let now = fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert!(now.contains("line 2 changed") && !now.contains("line 18 changed"));
    repo.undo_last().unwrap();
    assert_eq!(original, fs::read_to_string(dir.path().join("f.txt")).unwrap());
}

#[test]
fn discard_untracked_dir_then_undo() {
    let (dir, repo) = setup();
    fs::create_dir_all(dir.path().join("Thiết kế/sub")).unwrap();
    fs::write(dir.path().join("Thiết kế/sub/a.txt"), "xin chào\n").unwrap();
    let st = repo.status().unwrap();
    let e = st.entries.iter().find(|e| e.is_untracked()).unwrap();
    repo.discard(&[DiscardTarget::Untracked { path: e.path.clone() }]).unwrap();
    assert!(!dir.path().join("Thiết kế").exists());
    repo.undo_last().unwrap();
    assert_eq!(fs::read_to_string(dir.path().join("Thiết kế/sub/a.txt")).unwrap(), "xin chào\n");
}

#[test]
fn undo_refuses_after_outside_change() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    let d = repo.diff_worktree("f.txt").unwrap();
    let patch = build_patch(&d, |h, _| h == 0, true).unwrap();
    repo.discard_patch(&patch, "f.txt").unwrap();
    fs::write(dir.path().join("f.txt"), "edited by hand\n").unwrap();
    let err = repo.undo_last().unwrap_err().to_string();
    assert!(err.contains("edited after"), "{err}");
}

#[test]
fn branch_create_switch_and_undo() {
    let (dir, repo) = setup();
    repo.create_branch("feat/x", None).unwrap();
    assert_eq!(git(dir.path(), &["branch", "--show-current"]).trim(), "feat/x");
    repo.undo_last().unwrap();
    assert_eq!(git(dir.path(), &["branch", "--show-current"]).trim(), "main");
    assert!(!repo.branches().unwrap().iter().any(|b| b.name == "feat/x"));
}

#[test]
fn rename_and_delete_undo() {
    let (dir, repo) = setup();
    git(dir.path(), &["branch", "old"]);
    repo.rename_branch("old", "new").unwrap();
    repo.undo_last().unwrap();
    assert!(repo.branches().unwrap().iter().any(|b| b.name == "old"));
    repo.delete_branch("old", false).unwrap();
    repo.undo_last().unwrap();
    assert!(repo.branches().unwrap().iter().any(|b| b.name == "old"));
}

#[test]
fn log_and_detail() {
    let (dir, repo) = setup();
    edit_two_places(dir.path());
    repo.stage_all().unwrap();
    repo.commit("second", false).unwrap();
    let log = repo.log(&LogQuery { revs: vec!["HEAD".into()], skip: 0, limit: 10 }).unwrap();
    assert_eq!(log.len(), 2);
    assert!(log[0].is_head());
    let d = repo.commit_detail(&log[0].hash).unwrap();
    assert_eq!(d.files.len(), 1);
    assert_eq!(d.files[0].adds, Some(2));
    let root = repo.commit_detail(&log[1].hash).unwrap();
    assert_eq!(root.files[0].status, 'A');
    let fd = repo.diff_commit_file(&log[1].hash, None, "f.txt", None).unwrap();
    assert_eq!(fd.hunks.len(), 1);
}

#[test]
fn unborn_repo_works() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "core.autocrlf", "false"]);
    git(dir.path(), &["config", "user.name", "T"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    let repo = Repo::discover(dir.path()).unwrap();
    assert!(repo.log(&LogQuery { revs: vec![], skip: 0, limit: 10 }).unwrap().is_empty());
    repo.stage(&["a.txt".into()]).unwrap();
    repo.unstage(&["a.txt".into()]).unwrap();
    repo.stage_all().unwrap();
    repo.commit("first", false).unwrap();
    repo.undo_last().unwrap();
    assert!(repo.head_is_unborn());
    assert!(git(dir.path(), &["diff", "--cached", "--name-only"]).contains("a.txt"));
}
