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
        Err(Error::NotARepo(_)) => match offer_init(&start) {
            Some(r) => r,
            None => std::process::exit(1),
        },
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

    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!(
            "gitty: needs an interactive terminal.\nRun it directly in Terminal, iTerm2, Windows Terminal or PowerShell — not through a pipe or a non-interactive shell."
        );
        std::process::exit(1);
    }

    let mut terminal = match ratatui::try_init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("gitty: could not open the terminal: {e}");
            std::process::exit(1);
        }
    };
    enable_extras(mouse);
    let result = run(&mut terminal, repo, cfg, cfg_error);
    disable_extras(mouse);
    ratatui::restore();
    result
}

/// `start` is a plain folder: offer to `git init` it (when we can ask), and
/// return the new repository on yes.
fn offer_init(start: &Path) -> Option<Repo> {
    use std::io::IsTerminal;
    let shown = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    eprintln!("gitty: {} isn't a git repository yet.", shown.display());
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        eprintln!("Run `git init` there first, or pass a path: gitty ~/code/project");
        return None;
    }
    let warning = gitty_core::workspace::init_warning(start);
    if let Some(w) = &warning {
        eprintln!("\nCareful: {w}");
    }
    // default to no when there's a warning, yes otherwise
    let hint = if warning.is_some() { "[y/N]" } else { "[Y/n]" };
    eprint!("\nInitialize a repository here (adds a .git folder, first branch main)? {hint} ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return None;
    }
    let yes = match answer.trim().to_lowercase().as_str() {
        "y" | "yes" => true,
        "" => warning.is_none(),
        _ => false,
    };
    if !yes {
        eprintln!("Nothing changed. Open a repository with: gitty ~/code/project");
        return None;
    }
    match gitty_core::workspace::init_repo(start).and_then(|_| Repo::discover(start)) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("gitty: couldn't create the repository: {e}");
            None
        }
    }
}

fn run(terminal: &mut DefaultTerminal, repo: Repo, cfg: config::Config, cfg_error: Option<String>) -> Result<()> {
    let mouse = cfg.mouse;
    let mut app = app::App::new(repo, cfg.clone(), cfg_error);
    let mut _watcher = watcher::start(&app.repo, app.sender());
    loop {
        if let Some(path) = app.take_switch() {
            match Repo::discover(&path) {
                Ok(next) => {
                    _watcher = None;
                    app = app::App::new(next, cfg.clone(), None);
                    _watcher = watcher::start(&app.repo, app.sender());
                }
                Err(Error::NotARepo(_)) => app.after_suspend(Err(format!(
                    "{} isn't a git repository any more — run gitty there to set one up.",
                    path.display()
                ))),
                Err(e) => app.after_suspend(Err(format!("Can't open {}: {e}", path.display()))),
            }
        }
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
            let outcome = match (&s.edit, s.record) {
                (Some(file), Some((_, desc))) => repo.record(gitty_core::oplog::OpKind::Edit, desc, |r, backups| {
                    r.backup_path(file, backups)?;
                    run_editor(terminal, r.root(), file, mouse).map_err(|e| Error::Msg(e.to_string()))
                }),
                (Some(file), None) => run_editor(terminal, repo.root(), file, mouse).map_err(|e| Error::Msg(e.to_string())),
                (None, Some((kind, desc))) => repo.record(kind, desc, |r, _| {
                    run_interactive(terminal, r.root(), &s.args, mouse).map_err(|e| Error::Msg(e.to_string()))
                }),
                (None, None) => run_interactive(terminal, repo.root(), &s.args, mouse).map_err(|e| Error::Msg(e.to_string())),
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

/// The editor git would use (core.editor, GIT_EDITOR, VISUAL, EDITOR).
fn editor_command(root: &Path) -> String {
    let out = std::process::Command::new("git").args(["var", "GIT_EDITOR"]).current_dir(root).output();
    match out {
        Ok(o) if o.status.success() => {
            let e = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if e.is_empty() || e == ":" { default_editor() } else { e }
        }
        _ => default_editor(),
    }
}

fn default_editor() -> String {
    if cfg!(windows) { "notepad".into() } else { "vi".into() }
}

fn run_editor(terminal: &mut DefaultTerminal, root: &Path, file: &str, mouse: bool) -> Result<String> {
    let editor = editor_command(root);
    disable_extras(mouse);
    ratatui::restore();
    let status = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", &format!("{editor} \"{file}\"")]).current_dir(root).status()
    } else {
        std::process::Command::new("sh").arg("-c").arg(format!("{editor} \"$1\"")).arg("gitty").arg(file).current_dir(root).status()
    };
    *terminal = ratatui::init();
    enable_extras(mouse);
    terminal.clear()?;
    match status {
        Ok(s) if s.success() => Ok(format!("{editor} {file}")),
        Ok(_) => Err(anyhow::anyhow!("{editor} exited with an error")),
        Err(e) => Err(anyhow::anyhow!("could not start {editor}: {e}")),
    }
}
