//! Drive the real app against a temporary repository and inspect the screen.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use gitty_core::Repo;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Popup, View};
use crate::config::Config;
use crate::ui;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "core.autocrlf", "false"]);
    git(p, &["config", "user.name", "Hang"]);
    git(p, &["config", "user.email", "hang@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    fs::create_dir_all(p.join("Assets/Scripts")).unwrap();
    let code: String = (1..=30).map(|i| format!("// line {i}\n")).collect();
    fs::write(p.join("Assets/Scripts/Player.cs"), &code).unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "Initial Unity project"]);
    git(p, &["switch", "-q", "-c", "feat/inventory"]);
    fs::write(p.join("Assets/Scripts/Inventory.cs"), "class Inventory {}\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "Add inventory"]);
    git(p, &["switch", "-q", "main"]);
    git(p, &["merge", "-q", "--no-ff", "-m", "Merge feat/inventory", "feat/inventory"]);
    // working changes: two hunks in Player.cs plus an untracked file with a Vietnamese name
    let edited = code.replace("// line 2\n", "// line 2 jump buffer\n").replace("// line 28\n", "// line 28 coyote time\n");
    fs::write(p.join("Assets/Scripts/Player.cs"), edited).unwrap();
    fs::write(p.join("Ghi chú thiết kế.md"), "# Ý tưởng\n").unwrap();
    dir
}

fn app_for(dir: &Path) -> App {
    let cfg = Config { symbols: "unicode".into(), color: "16".into(), ..Config::default() };
    let mut app = App::new(Repo::discover(dir).unwrap(), cfg, None);
    wait(&mut app, |a| a.loaded_once);
    app
}

fn wait(app: &mut App, cond: impl Fn(&App) -> bool) {
    let start = Instant::now();
    while !cond(app) {
        app.tick();
        assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wait until queued operations are done and the follow-up refresh landed.
fn settle(app: &mut App) {
    // waiting on state, not a fixed sleep: slow CI runners (Windows) need longer
    wait(app, |a| a.busy == 0);
    std::thread::sleep(Duration::from_millis(100));
    for _ in 0..30 {
        app.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    wait(app, |a| a.is_idle());
}

fn key(app: &mut App, code: KeyCode) {
    app.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn chars(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

fn screen(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn changes_view_shows_files_and_diff() {
    let dir = setup();
    let mut app = app_for(dir.path());
    let s = screen(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Unstaged"));
    assert!(s.contains("Player.cs"));
    assert!(s.contains("Ghi chú thiết kế.md"));
    assert!(s.contains("jump buffer"));
    assert!(s.contains("1 Changes"));
}

#[test]
fn stage_a_single_hunk_from_the_diff() {
    let dir = setup();
    let mut app = app_for(dir.path());
    // Player.cs is selected first; go into the diff and stage the first hunk
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char(' '));
    settle(&mut app);
    let out = Command::new("git").current_dir(dir.path()).args(["diff", "--cached"]).output().unwrap();
    let cached = String::from_utf8_lossy(&out.stdout);
    assert!(cached.contains("jump buffer"), "{cached}");
    assert!(!cached.contains("coyote"), "{cached}");
    let s = screen(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Staged"));
}

#[test]
fn commit_then_undo() {
    let dir = setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('a')); // stage all
    settle(&mut app);
    key(&mut app, KeyCode::Char('c'));
    assert!(matches!(app.popup, Some(Popup::Commit(_))));
    chars(&mut app, "Sửa nhảy");
    let s = screen(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Commit 2 files to main"));
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(app.status.entries.is_empty(), "{:?}", app.status.entries);
    let log = Command::new("git").current_dir(dir.path()).args(["log", "-1", "--format=%s"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "Sửa nhảy");

    key(&mut app, KeyCode::Char('u'));
    settle(&mut app);
    let log = Command::new("git").current_dir(dir.path()).args(["log", "-1", "--format=%s"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "Merge feat/inventory");
    assert_eq!(app.status.entries.iter().filter(|e| e.is_staged()).count(), 2);
}

#[test]
fn history_view_draws_the_graph() {
    let dir = setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('2'));
    assert_eq!(app.view, View::History);
    wait(&mut app, |a| a.detail.is_some());
    let s = screen(&mut app, 130, 30);
    println!("{s}");
    assert!(s.contains("Merge feat/inventory"));
    assert!(s.contains("HEAD → main"));
    assert!(s.contains('╮') || s.contains('╯'));
}

#[test]
fn branches_view_and_new_branch() {
    let dir = setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('3'));
    let s = screen(&mut app, 120, 24);
    println!("{s}");
    assert!(s.contains("feat/inventory"));
    key(&mut app, KeyCode::Char('b'));
    chars(&mut app, "fix/jump");
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert_eq!(app.status.branch.head.as_deref(), Some("fix/jump"));
}

#[test]
fn ascii_mode_and_small_terminal() {
    let dir = setup();
    let cfg = Config { symbols: "ascii".into(), color: "none".into(), ..Config::default() };
    let mut app = App::new(Repo::discover(dir.path()).unwrap(), cfg, None);
    wait(&mut app, |a| a.loaded_once);
    key(&mut app, KeyCode::Char('2'));
    wait(&mut app, |a| a.detail.is_some());
    let s = screen(&mut app, 100, 20);
    println!("{s}");
    assert!(!s.contains('╭') && !s.contains('●'), "ascii mode leaked unicode");
    assert!(s.contains("+-"));
    let tiny = screen(&mut app, 40, 10);
    assert!(tiny.contains("needs at least"));
}

#[test]
fn help_popup_lists_keys() {
    let dir = setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('?'));
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("Stage / unstage"));
    key(&mut app, KeyCode::Esc);
    assert!(app.popup.is_none());
}

// ------------------------------------------------------------------ phase 2

fn unity_setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "core.autocrlf", "false"]);
    git(p, &["config", "user.name", "Hang"]);
    git(p, &["config", "user.email", "hang@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    let w = |rel: &str, text: &str| {
        let f = p.join(rel);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, text).unwrap();
    };
    w("ProjectSettings/ProjectVersion.txt", "m_EditorVersion: 6000.0.23f1\n");
    w("Packages/manifest.json", r#"{"dependencies":{"com.unity.render-pipelines.universal":"17.0.3"}}"#);
    w("UserSettings/Layouts/default.dwlt", "layout\n");
    w("Assets/Scenes/Keep.unity", "%YAML 1.1\n--- !u!1 &100\nGameObject:\n  m_Name: InventoryChest\n--- !u!4 &101\nTransform:\n  m_GameObject: {fileID: 100}\n  m_LocalPosition: {x: 12.5, y: 0, z: -3.25}\n");
    w("Assets/Scripts/Player.cs", "class Player {}\n");
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "Initial Unity project"]);
    for i in 1..=3 {
        w(&format!("Assets/Scripts/Item{i}.cs"), &format!("class Item{i} {{}}\n"));
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", &format!("Add item {i}")]);
    }
    dir
}

#[test]
fn insight_view_shows_project_health_and_sizes() {
    let dir = unity_setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('4'));
    wait(&mut app, |a| a.insight.is_some() && !a.blobs_loading);
    let s = screen(&mut app, 140, 40);
    println!("{s}");
    assert!(s.contains("Unity 6000.0.23f1"));
    assert!(s.contains("URP"));
    assert!(s.contains("UserSettings/ is committed"));
    assert!(s.contains("Size · repo root"));
    assert!(s.contains("Largest in history"));
    assert!(s.contains("Hot files"));

    // fix the UserSettings warning from the Health list
    let idx = app.health_items().iter().position(|h| h.title.starts_with("UserSettings/")).unwrap();
    app.health_sel = idx;
    key(&mut app, KeyCode::Enter);
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
    key(&mut app, KeyCode::Char('y'));
    settle(&mut app);
    assert!(fs::read_to_string(dir.path().join(".gitignore")).unwrap().contains("/UserSettings/"));
    wait(&mut app, |a| !a.insight_loading && !a.blobs_loading);
    assert!(!app.health_items().iter().any(|h| h.title.starts_with("UserSettings/ is committed")));
}

#[test]
fn unity_scene_change_is_summarized_in_the_diff() {
    let dir = unity_setup();
    let scene = dir.path().join("Assets/Scenes/Keep.unity");
    let text = fs::read_to_string(&scene).unwrap().replace("x: 12.5", "x: 14");
    fs::write(&scene, text).unwrap();
    let mut app = app_for(dir.path());
    let s = screen(&mut app, 140, 30);
    println!("{s}");
    assert!(s.contains("Unity changes (1)"));
    assert!(s.contains("InventoryChest › Transform.m_LocalPosition"));
}

#[test]
fn stash_from_keys_then_pop_from_branches() {
    let dir = setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('s'));
    chars(&mut app, "WIP jump");
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(app.status.entries.is_empty(), "everything stashed incl. new files");
    assert_eq!(app.stashes.len(), 1);
    key(&mut app, KeyCode::Char('3'));
    key(&mut app, KeyCode::Char('G')); // last row = the stash
    assert!(app.selected_stash().is_some());
    app.tick();
    std::thread::sleep(Duration::from_millis(120));
    app.tick();
    let s = screen(&mut app, 120, 24);
    println!("{s}");
    assert!(s.contains("Stash · stash@{0}"));
    assert!(s.contains("Player.cs"));
    key(&mut app, KeyCode::Char('o'));
    settle(&mut app);
    assert!(app.stashes.is_empty());
    assert_eq!(app.status.entries.len(), 2);
}

#[test]
fn interactive_rebase_from_history() {
    let dir = unity_setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('2'));
    key(&mut app, KeyCode::Char('a')); // current branch only
    settle(&mut app);
    // select "Add item 1" (third from top) and rebase from there
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char('r'));
    assert!(matches!(app.popup, Some(Popup::Rebase(_))), "rebase editor opens");
    let s = screen(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Interactive rebase · 3 commits"));
    key(&mut app, KeyCode::Char('j')); // "Add item 2"
    key(&mut app, KeyCode::Char('f')); // fixup into item 1
    key(&mut app, KeyCode::Char('k')); // "Add item 3"
    key(&mut app, KeyCode::Char('r'));
    for _ in 0..10 {
        key(&mut app, KeyCode::Backspace);
    }
    chars(&mut app, "Thêm 3");
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Enter); // review
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
    key(&mut app, KeyCode::Char('y'));
    settle(&mut app);
    let out = Command::new("git").current_dir(dir.path()).args(["log", "--format=%s"]).output().unwrap();
    let log = String::from_utf8_lossy(&out.stdout);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["Thêm 3", "Add item 1", "Initial Unity project"], "{log}");
    key(&mut app, KeyCode::Char('u'));
    settle(&mut app);
    let out = Command::new("git").current_dir(dir.path()).args(["log", "-1", "--format=%s"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Add item 3");
}

#[test]
fn resolve_merge_conflict_in_the_ui() {
    let dir = setup();
    let p = dir.path();
    git(p, &["stash", "-u", "-q"]);
    git(p, &["switch", "-q", "feat/inventory"]);
    fs::write(p.join("Assets/Scripts/Player.cs"), "// theirs\n").unwrap();
    git(p, &["commit", "-q", "-am", "theirs"]);
    git(p, &["switch", "-q", "main"]);
    fs::write(p.join("Assets/Scripts/Player.cs"), "// ours\n").unwrap();
    git(p, &["commit", "-q", "-am", "ours"]);
    let _ = Command::new("git").current_dir(p).args(["merge", "feat/inventory"]).output();
    let mut app = app_for(p);
    assert_eq!(app.state, gitty_core::RepoState::Merging);
    let s = screen(&mut app, 120, 24);
    println!("{s}");
    assert!(s.contains("Conflicts"));
    assert!(s.contains("Conflict 1/1"));
    assert!(s.contains("MERGING"));
    key(&mut app, KeyCode::Enter); // into the resolver
    key(&mut app, KeyCode::Char('b')); // keep both
    settle(&mut app);
    assert_eq!(fs::read_to_string(p.join("Assets/Scripts/Player.cs")).unwrap(), "// ours\n// theirs\n");
    key(&mut app, KeyCode::Char(' ')); // mark resolved
    settle(&mut app);
    key(&mut app, KeyCode::Char('m'));
    assert!(matches!(app.popup, Some(Popup::OpMenu)));
    key(&mut app, KeyCode::Char('c'));
    settle(&mut app);
    assert_eq!(app.state, gitty_core::RepoState::Clean);
    let out = Command::new("git").current_dir(p).args(["log", "-1", "--format=%p"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).split_whitespace().count(), 2, "merge commit created");
}

#[test]
fn repos_view_lists_sibling_repositories() {
    let parent = tempfile::tempdir().unwrap();
    for name in ["alpha", "beta"] {
        let p = parent.path().join(name);
        fs::create_dir_all(&p).unwrap();
        git(&p, &["init", "-q", "-b", "main"]);
        git(&p, &["config", "core.autocrlf", "false"]);
    }
    fs::write(parent.path().join("beta/x.txt"), "x").unwrap();
    let mut app = app_for(&parent.path().join("alpha"));
    key(&mut app, KeyCode::Char('5'));
    wait(&mut app, |a| a.repos.len() == 2);
    let s = screen(&mut app, 140, 20);
    println!("{s}");
    assert!(s.contains("alpha") && s.contains("beta"));
    assert!(s.contains("1 changed"));
    let beta = app.repos.iter().position(|r| r.name == "beta").unwrap();
    app.repo_sel = beta;
    key(&mut app, KeyCode::Enter);
    let target = app.take_switch().expect("switch requested");
    assert!(target.ends_with("beta"));
}

// ------------------------------------------------------------------ extras

#[test]
fn edit_message_of_an_older_commit() {
    let dir = unity_setup();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('2'));
    key(&mut app, KeyCode::Char('a'));
    settle(&mut app);
    key(&mut app, KeyCode::Char('j')); // "Add item 2"
    key(&mut app, KeyCode::Char('e'));
    let s = screen(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Edit message of"));
    for _ in 0..12 {
        key(&mut app, KeyCode::Backspace);
    }
    chars(&mut app, "Thêm vật phẩm 2");
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    let out = Command::new("git").current_dir(dir.path()).args(["log", "--format=%s"]).output().unwrap();
    let log = String::from_utf8_lossy(&out.stdout);
    assert_eq!(log.lines().collect::<Vec<_>>(), ["Add item 3", "Thêm vật phẩm 2", "Add item 1", "Initial Unity project"]);
}

#[test]
fn ignore_a_file_by_extension_from_changes() {
    let dir = setup();
    let mut app = app_for(dir.path());
    // the untracked markdown note is the second unstaged row
    key(&mut app, KeyCode::Char('j'));
    assert!(app.selected_entry().unwrap().1.path.ends_with(".md"));
    key(&mut app, KeyCode::Char('i'));
    let s = screen(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("every .md file"));
    key(&mut app, KeyCode::Char('2'));
    settle(&mut app);
    assert!(fs::read_to_string(dir.path().join(".gitignore")).unwrap().contains("*.md"));
    assert!(!app.status.entries.iter().any(|e| e.path.ends_with(".md")));
}

#[test]
fn ignore_rules_popup_add_check_delete() {
    let dir = setup();
    fs::write(dir.path().join(".gitignore"), "# build output\n/Library/\n").unwrap();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('I'));
    assert!(matches!(app.popup, Some(Popup::IgnoreRules(_))));
    key(&mut app, KeyCode::Char('a'));
    chars(&mut app, "*.tmp");
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    key(&mut app, KeyCode::Char('/'));
    chars(&mut app, "cache.tmp");
    key(&mut app, KeyCode::Enter);
    let s = screen(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Ignore rules"));
    assert!(s.contains("/Library/"));
    assert!(s.contains("is ignored by `*.tmp`"), "{s}");
    // select the *.tmp rule (last row of .gitignore) and delete it
    key(&mut app, KeyCode::Char('G'));
    key(&mut app, KeyCode::Char('d'));
    settle(&mut app);
    assert!(!fs::read_to_string(dir.path().join(".gitignore")).unwrap().contains("*.tmp"));
}

#[test]
fn insight_composition_by_type() {
    let dir = unity_setup();
    fs::write(dir.path().join("Assets/cliff.psd"), vec![1u8; 50_000]).unwrap();
    let mut app = app_for(dir.path());
    key(&mut app, KeyCode::Char('4'));
    wait(&mut app, |a| a.insight.is_some());
    let s = screen(&mut app, 140, 44);
    println!("{s}");
    assert!(s.contains("Composition"));
    assert!(s.contains("Images & textures"));
    assert!(s.contains(".psd"));
}
