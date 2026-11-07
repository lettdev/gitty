//! Insight, stash, rebase and conflict resolution against real repositories.

use std::fs;
use std::path::Path;
use std::process::Command;

use gitty_core::conflict::{self, Pick};
use gitty_core::insight::{Fix, Severity};
use gitty_core::project::ProjectKind;
use gitty_core::rebase::RebaseAction;
use gitty_core::{Repo, RepoState};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).args(args).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn init() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "core.autocrlf", "false"]);
    git(p, &["config", "user.name", "Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    dir
}

fn write(p: &Path, rel: &str, text: &str) {
    let f = p.join(rel);
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(f, text).unwrap();
}

fn commit(p: &Path, msg: &str) {
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", msg]);
}

fn unity_repo() -> tempfile::TempDir {
    let d = init();
    let p = d.path();
    write(p, "ProjectSettings/ProjectVersion.txt", "m_EditorVersion: 6000.0.23f1\n");
    write(p, "ProjectSettings/EditorSettings.asset", "EditorSettings:\n  m_SerializationMode: 1\n  m_ExternalVersionControlSupport: Visible Meta Files\n");
    write(p, "Packages/manifest.json", r#"{"dependencies":{"com.unity.render-pipelines.universal":"17.0.3"}}"#);
    write(p, "Assets/Scripts/Player.cs", "class Player {}\n");
    write(p, "UserSettings/Layouts/default.dwlt", "layout\n");
    write(p, "Assets/.DS_Store", "junk");
    fs::write(p.join("Assets/cliff.psd"), vec![7u8; 2 * 1024 * 1024]).unwrap();
    commit(p, "Initial Unity project");
    fs::create_dir_all(p.join("Library/Artifacts")).unwrap();
    fs::write(p.join("Library/Artifacts/cache.bin"), vec![1u8; 4096]).unwrap();
    d
}

#[test]
fn insight_on_a_unity_project() {
    let d = unity_repo();
    let repo = Repo::discover(d.path()).unwrap();
    let ins = repo.insight().unwrap();
    assert_eq!(ins.projects[0].kind, ProjectKind::Unity);
    assert_eq!(ins.languages[0].name, "C#");
    assert!(ins.size.worktree > 2 * 1024 * 1024);
    let titles: Vec<&str> = ins.health.iter().map(|h| h.title.as_str()).collect();
    assert!(titles.iter().any(|t| t.starts_with("UserSettings/ is committed")), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("Library/ is not ignored")), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with(".DS_Store is committed")), "{titles:?}");
    assert!(titles.contains(&"Unity assets are saved as binary"), "{titles:?}");
    // the 2 MB PSD is an engine art file over 1 MB that is not in LFS
    assert!(titles.iter().any(|t| t.contains("not in LFS") || t.contains("LFS is not installed")), "{titles:?}");
    assert_eq!(ins.health[0].severity, Severity::Warn);
    assert_eq!(ins.hot.iter().find(|h| h.path == "Assets/Scripts/Player.cs").map(|h| h.commits), Some(1));
    assert!(ins.tree.find("Assets/").is_some());
}

#[test]
fn ignore_fix_untracks_and_undo_restores() {
    let d = unity_repo();
    let repo = Repo::discover(d.path()).unwrap();
    let ins = repo.insight().unwrap();
    let fix = ins.health.iter().find(|h| h.title.starts_with("UserSettings/")).and_then(|h| h.fix.clone()).unwrap();
    let Fix::Ignore { pattern, untrack } = fix else { panic!("expected an ignore fix") };
    repo.apply_ignore(&pattern, &untrack).unwrap();
    assert!(fs::read_to_string(d.path().join(".gitignore")).unwrap().contains("/UserSettings/"));
    assert!(!git(d.path(), &["ls-files", "--cached"]).contains("UserSettings"));
    assert!(d.path().join("UserSettings/Layouts/default.dwlt").exists(), "files stay on disk");
    repo.undo_last().unwrap();
    assert!(!d.path().join(".gitignore").exists());
    assert!(git(d.path(), &["ls-files", "--cached"]).contains("UserSettings"));
}

#[test]
fn largest_blobs_finds_deleted_files_and_their_commit() {
    let d = init();
    let p = d.path();
    write(p, "a.txt", "small\n");
    commit(p, "first");
    fs::write(p.join("big.bin"), vec![3u8; 300_000]).unwrap();
    commit(p, "add big file");
    let added = git(p, &["rev-parse", "HEAD"]).trim().to_string();
    fs::remove_file(p.join("big.bin")).unwrap();
    commit(p, "remove big file");
    let repo = Repo::discover(p).unwrap();
    let blobs = repo.largest_blobs(5, true).unwrap();
    assert_eq!(blobs[0].path, "big.bin");
    assert_eq!(blobs[0].size, 300_000);
    assert_eq!(blobs[0].commit.as_deref(), Some(added.as_str()));
    // second call is served from the cache
    assert_eq!(repo.largest_blobs(5, true).unwrap(), blobs);
}

#[test]
fn stash_push_drop_and_undo() {
    let d = init();
    let p = d.path();
    write(p, "a.txt", "one\n");
    commit(p, "first");
    write(p, "a.txt", "two\n");
    write(p, "new.txt", "untracked\n");
    let repo = Repo::discover(p).unwrap();
    repo.stash_push("WIP lighting", true).unwrap();
    assert_eq!(fs::read_to_string(p.join("a.txt")).unwrap(), "one\n");
    assert!(!p.join("new.txt").exists());
    let stashes = repo.stashes().unwrap();
    assert_eq!(stashes.len(), 1);
    let files = repo.stash_files(&stashes[0].oid).unwrap();
    assert!(files.iter().any(|f| f.path == "a.txt" && f.status == 'M'));
    assert!(files.iter().any(|f| f.path == "new.txt" && f.status == '?'));
    let fd = repo.stash_file_diff(&stashes[0].oid, &files[0]).unwrap();
    assert!(!fd.hunks.is_empty());

    repo.stash_drop(&stashes[0]).unwrap();
    assert!(repo.stashes().unwrap().is_empty());
    repo.undo_last().unwrap();
    assert_eq!(repo.stashes().unwrap().len(), 1, "drop undone");

    repo.undo_last().unwrap(); // undo the push: changes come back
    assert_eq!(fs::read_to_string(p.join("a.txt")).unwrap(), "two\n");
    assert!(p.join("new.txt").exists());
    assert!(repo.stashes().unwrap().is_empty());
    assert!(repo.stash_push("", false).is_ok());
    git(p, &["stash", "pop", "-q"]);
    git(p, &["stash", "push", "-q", "-u"]);
    assert!(repo.stash_push("", false).is_err(), "nothing left to stash");
}

#[test]
fn rebase_reword_squash_drop_then_undo() {
    let d = init();
    let p = d.path();
    write(p, "f.txt", "0\n");
    commit(p, "base");
    let base = git(p, &["rev-parse", "HEAD"]).trim().to_string();
    for i in 1..=4 {
        write(p, &format!("f{i}.txt"), &format!("{i}\n"));
        commit(p, &format!("change {i}"));
    }
    let head_before = git(p, &["rev-parse", "HEAD"]);
    let repo = Repo::discover(p).unwrap();
    let mut steps = repo.rebase_plan(Some(&base)).unwrap();
    assert_eq!(steps.iter().map(|s| s.subject.as_str()).collect::<Vec<_>>(), ["change 1", "change 2", "change 3", "change 4"]);
    steps[0].action = RebaseAction::Reword;
    steps[0].message = Some("Thêm tính năng 1".into());
    steps[1].action = RebaseAction::Fixup;
    steps[2].action = RebaseAction::Drop;
    repo.run_rebase(Some(&base), &steps).unwrap();
    let log = git(p, &["log", "--format=%s", &format!("{base}..HEAD")]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["change 4", "Thêm tính năng 1"]);
    assert!(p.join("f2.txt").exists(), "fixup keeps the change");
    assert!(!p.join("f3.txt").exists(), "drop removes the change");

    repo.undo_last().unwrap();
    assert_eq!(git(p, &["rev-parse", "HEAD"]), head_before);
}

#[test]
fn rebase_conflict_continue_and_undo() {
    let d = init();
    let p = d.path();
    write(p, "f.txt", "a\n");
    commit(p, "base");
    let base = git(p, &["rev-parse", "HEAD"]).trim().to_string();
    write(p, "f.txt", "b\n");
    commit(p, "to b");
    write(p, "f.txt", "c\n");
    commit(p, "to c");
    let head_before = git(p, &["rev-parse", "HEAD"]);
    let repo = Repo::discover(p).unwrap();
    let mut steps = repo.rebase_plan(Some(&base)).unwrap();
    steps.swap(0, 1); // reordering two edits of the same line conflicts
    let err = repo.run_rebase(Some(&base), &steps).unwrap_err().to_string();
    assert!(err.contains("conflict"), "{err}");
    assert_eq!(repo.state(), RepoState::Rebasing);

    let cf = repo.read_conflict("f.txt").unwrap();
    assert_eq!(cf.regions.len(), 1);
    let text = conflict::resolve(&cf, 0, Pick::Theirs).unwrap();
    repo.write_resolution("f.txt", &text, "Take theirs").unwrap();
    git(p, &["add", "f.txt"]);
    // the second pick conflicts again; resolve it the same way
    loop {
        match repo.continue_operation() {
            Ok(_) if repo.state() == RepoState::Clean => break,
            Ok(_) => {}
            Err(_) => {
                let cf = repo.read_conflict("f.txt").unwrap();
                let text = conflict::resolve(&cf, 0, Pick::Theirs).unwrap();
                fs::write(p.join("f.txt"), text).unwrap();
                git(p, &["add", "f.txt"]);
            }
        }
    }
    assert_eq!(repo.state(), RepoState::Clean);
    assert_ne!(git(p, &["rev-parse", "HEAD"]), head_before);
    repo.undo_last().unwrap();
    assert_eq!(git(p, &["rev-parse", "HEAD"]), head_before);
}

#[test]
fn merge_conflict_take_side_and_show_base() {
    let d = init();
    let p = d.path();
    write(p, "f.txt", "base\n");
    commit(p, "base");
    git(p, &["switch", "-q", "-c", "feat"]);
    write(p, "f.txt", "theirs\n");
    commit(p, "feat");
    git(p, &["switch", "-q", "main"]);
    write(p, "f.txt", "ours\n");
    commit(p, "main");
    let _ = Command::new("git").current_dir(p).args(["merge", "-q", "feat"]).output();
    let repo = Repo::discover(p).unwrap();
    assert_eq!(repo.state(), RepoState::Merging);
    repo.show_conflict_base("f.txt").unwrap();
    let cf = repo.read_conflict("f.txt").unwrap();
    assert_eq!(cf.regions[0].base.as_deref(), Some(&["base\n".to_string()][..]));
    repo.take_side("f.txt", true).unwrap();
    assert_eq!(fs::read_to_string(p.join("f.txt")).unwrap(), "ours\n");
    repo.undo_last().unwrap();
    assert!(fs::read_to_string(p.join("f.txt")).unwrap().contains("|||||||"));
    repo.abort_operation().unwrap();
    assert_eq!(repo.state(), RepoState::Clean);
}

#[test]
fn reword_head_and_older_commit() {
    let d = init();
    let p = d.path();
    for i in 1..=3 {
        write(p, &format!("f{i}.txt"), "x\n");
        commit(p, &format!("commit {i}"));
    }
    write(p, "staged.txt", "keep me staged\n");
    git(p, &["add", "staged.txt"]);
    let repo = Repo::discover(p).unwrap();
    let head = git(p, &["rev-parse", "HEAD"]).trim().to_string();
    repo.reword_commit(&head, "Sửa message mới nhất").unwrap();
    assert_eq!(git(p, &["log", "-1", "--format=%s"]).trim(), "Sửa message mới nhất");
    assert!(git(p, &["diff", "--cached", "--name-only"]).contains("staged.txt"), "staged changes untouched");
    assert!(!git(p, &["show", "--name-only", "--format=", "HEAD"]).contains("staged.txt"));

    let first = git(p, &["rev-list", "--reverse", "HEAD"]).lines().next().unwrap().to_string();
    repo.reword_commit(&first, "Đầu tiên").unwrap();
    let log = git(p, &["log", "--format=%s"]);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["Sửa message mới nhất", "commit 2", "Đầu tiên"]);
    assert!(git(p, &["diff", "--cached", "--name-only"]).contains("staged.txt"), "autostash restored the index");
    repo.undo_last().unwrap();
    assert_eq!(git(p, &["log", "--format=%s"]).lines().last(), Some("commit 1"));
}

#[test]
fn ignore_sources_why_and_edit() {
    let d = init();
    let p = d.path();
    write(p, ".gitignore", "# build\n/Library/\n*.tmp\n");
    write(p, "Assets/.gitignore", "local.txt\n");
    write(p, "a.txt", "a\n");
    commit(p, "init");
    let repo = Repo::discover(p).unwrap();
    let src = repo.ignore_sources().unwrap();
    let root = src.iter().find(|s| s.label == ".gitignore").unwrap();
    assert_eq!(root.rules.len(), 3);
    assert!(root.rules[0].comment);
    assert!(src.iter().any(|s| s.rel.as_deref() == Some("Assets/.gitignore")));

    let why = repo.why_ignored("x.tmp").unwrap().unwrap();
    assert_eq!((why.source.as_str(), why.line, why.pattern.as_str()), (".gitignore", 3, "*.tmp"));
    assert!(repo.why_ignored("a.txt").unwrap().is_none());

    repo.add_ignore_rule(".gitignore", "*.log", &[]).unwrap();
    assert!(repo.why_ignored("debug.log").unwrap().is_some());
    repo.remove_ignore_rule(".gitignore", 3, "*.tmp").unwrap();
    assert!(repo.why_ignored("x.tmp").unwrap().is_none());
    assert!(repo.remove_ignore_rule(".gitignore", 1, "nope").is_err());
    repo.undo_last().unwrap();
    assert!(repo.why_ignored("x.tmp").unwrap().is_some());
}
