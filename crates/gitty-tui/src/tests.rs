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
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wait until queued operations are done and the follow-up refresh landed.
fn settle(app: &mut App) {
    wait(app, |a| a.busy == 0);
    std::thread::sleep(Duration::from_millis(100));
    for _ in 0..30 {
        app.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
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
