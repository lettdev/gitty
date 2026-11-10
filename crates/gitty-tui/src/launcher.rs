//! The start screen: shown when gitty starts outside a repository (or in one
//! it can't open). Pick a repository, open a folder, clone, set up git or
//! trust a folder; the main app starts once a repository opens.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitty_core::{Error, Repo};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::config::Config;
use crate::keys::{Action, Chord, Keymap};
use crate::picker::{Outcome, Picker};
use crate::theme::{self, Symbols, Theme};

pub struct Launcher {
    pub picker: Picker,
    pub theme: Theme,
    pub sym: Symbols,
    pub keys: Keymap,
    started: Instant,
    pub quit: bool,
    pub opened: Option<Repo>,
}

impl Launcher {
    /// `start` is the folder gitty was started in; `err` is why it didn't open.
    pub fn new(cfg: &Config, start: &Path, err: Option<Error>, recent: Vec<PathBuf>) -> Self {
        let (keys, _) = Keymap::new(&cfg.keys);
        let base = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
        let roots: Vec<PathBuf> =
            if cfg.workspaces.is_empty() { vec![base.clone()] } else { cfg.workspaces.iter().map(|w| crate::config::expand(w)).collect() };
        let mut picker = Picker::new(roots, base, None, recent);
        if let Some(e) = err {
            picker.set_target(start, e);
        }
        picker.scan();
        Self {
            picker,
            theme: Theme::new(theme::detect_depth(&cfg.color)),
            sym: Symbols::detect(&cfg.symbols),
            keys,
            started: Instant::now(),
            quit: false,
            opened: None,
        }
    }

    pub fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    pub fn spinner(&self) -> &'static str {
        let frames = self.sym.spinner;
        frames[(self.started.elapsed().as_millis() / 100) as usize % frames.len()]
    }

    pub fn tick(&mut self) {
        if let Some(Outcome::Open(repo)) = self.picker.tick() {
            self.opened = Some(repo);
        }
    }

    pub fn on_event(&mut self, ev: Event) {
        let Event::Key(key) = ev else { return };
        if key.kind == KeyEventKind::Release {
            return;
        }
        if self.picker.prompt.is_none() {
            let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
            if ctrl_c || key.code == KeyCode::Esc || self.keys.find(&[Action::Quit], Chord::from_event(&key)).is_some() {
                self.quit = true;
                return;
            }
        }
        if let Outcome::Open(repo) = self.picker.on_key(&key, &self.keys) {
            self.opened = Some(repo);
        }
    }
}

/// Run the start screen until a repository opens (`Some`) or the user quits.
pub fn run(terminal: &mut DefaultTerminal, cfg: &Config, start: &Path, err: Option<Error>) -> std::io::Result<Option<Repo>> {
    let mut l = Launcher::new(cfg, start, err, crate::recent::load());
    loop {
        l.tick();
        if let Some(repo) = l.opened.take() {
            return Ok(Some(repo));
        }
        if l.quit {
            return Ok(None);
        }
        terminal.draw(|f| crate::ui::draw_start(f, &mut l))?;
        if event::poll(Duration::from_millis(50))? {
            l.on_event(event::read()?);
        }
    }
}
