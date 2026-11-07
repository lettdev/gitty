mod app;
mod config;
mod keys;
mod theme;
mod ui;
mod util;
mod watcher;
mod widgets;
mod worker;

#[cfg(test)]
mod tests;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use gitty_core::git::display_command;
use gitty_core::{Error, Repo};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture,
};
use ratatui::crossterm::execute;

const HELP: &str = "gitty — a friendly git client for the terminal

Usage: gitty [PATH]

Opens the repository containing PATH (default: the current directory).

Options:
  -h, --help       Show this help
  -V, --version    Show the version

Press ? inside gitty to see every key.";

fn enable_extras(mouse: bool) {
    let mut out = io::stdout();
    // Not every terminal supports these; failures are harmless.
    let _ = execute!(out, EnableBracketedPaste);
    let _ = execute!(out, EnableFocusChange);
    if mouse {
        let _ = execute!(out, EnableMouseCapture);
    }
}

fn disable_extras(mouse: bool) {
    let mut out = io::stdout();
    if mouse {
        let _ = execute!(out, DisableMouseCapture);
    }
    let _ = execute!(out, DisableFocusChange);
    let _ = execute!(out, DisableBracketedPaste);
}

fn main() -> Result<()> {
    let mut path: Option<PathBuf> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("gitty {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            a if a.starts_with('-') => {
                eprintln!("gitty: unknown option {a}\n\n{HELP}");
                std::process::exit(2);
            }
            a => path = Some(PathBuf::from(a)),
        }
    }
    let start = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    let repo = match Repo::discover(&start) {
        Ok(r) => r,
        Err(Error::NotARepo(_)) => {
            eprintln!("gitty: {} is not inside a git repository.\nRun `git init` there first, or pass a path: gitty ~/code/project", start.display());
            std::process::exit(1);
        }
        Err(Error::GitNotFound) => {
            eprintln!("gitty: git was not found. Install it from https://git-scm.com and try again.");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("gitty: {e}");
            std::process::exit(1);
        }
    };
    let (cfg, cfg_error) = config::load();
    let mouse = cfg.mouse;

    let mut terminal = ratatui::init();
    enable_extras(mouse);
    let result = run(&mut terminal, repo, cfg, cfg_error);
    disable_extras(mouse);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, repo: Repo, cfg: config::Config, cfg_error: Option<String>) -> Result<()> {
    let mouse = cfg.mouse;
    let mut app = app::App::new(repo, cfg, cfg_error);
    let _watcher = watcher::start(&app.repo, app.sender());
    loop {
        if app.take_dirty() {
            terminal.draw(|f| ui::draw(f, &mut app))?;
        }
        if event::poll(Duration::from_millis(50))? {
            loop {
                app.on_event(event::read()?);
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        app.tick();
        if app.should_quit() {
            return Ok(());
        }
        if let Some(s) = app.take_suspend() {
            let repo = app.repo.clone();
            let outcome = match s.record {
                Some((kind, desc)) => repo.record(kind, desc, |r, _| {
                    run_interactive(terminal, r.root(), &s.args, mouse).map_err(|e| Error::Msg(e.to_string()))
                }),
                None => run_interactive(terminal, repo.root(), &s.args, mouse).map_err(|e| Error::Msg(e.to_string())),
            };
            app.after_suspend(outcome.map_err(|e| e.to_string()));
        }
    }
}

/// Leave the UI, run git with the real terminal (editors, password prompts),
/// then come back. Returns the command line for display.
fn run_interactive(terminal: &mut DefaultTerminal, root: &Path, args: &[String], mouse: bool) -> Result<String> {
    disable_extras(mouse);
    ratatui::restore();
    let shown = display_command(args);
    println!("\n$ {shown}\n");
    let status = std::process::Command::new("git").args(args).current_dir(root).status();
    match &status {
        Ok(s) if s.success() => println!("\n✓ done. Press Enter to return to gitty…"),
        Ok(s) => println!("\n✗ git exited with {}. Press Enter to return to gitty…", s.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into())),
        Err(e) => println!("\n✗ could not run git: {e}. Press Enter to return to gitty…"),
    }
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    *terminal = ratatui::init();
    enable_extras(mouse);
    terminal.clear()?;
    match status {
        Ok(s) if s.success() => Ok(shown),
        Ok(_) => Err(anyhow::anyhow!("`{shown}` failed")),
        Err(e) => Err(e.into()),
    }
}
