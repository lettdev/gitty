//! Choosing a repository: recent and nearby repositories, opening any folder,
//! cloning, setting up git in a plain folder and trusting a folder git
//! refuses. Shared by the start screen and the Repositories view (5).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use gitty_core::preview::{self, FolderPreview, InitOptions};
use gitty_core::workspace::{self, RepoSummary};
use gitty_core::{Error, Repo};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::keys::{self, Action, Chord, Keymap};
use crate::widgets::LineInput;

/// A folder that couldn't be opened as it is, and why.
pub enum Target {
    /// No repository here yet; `i` sets one up. `preview` arrives from a
    /// background scan of the folder.
    NotARepo { path: PathBuf, warning: Option<String>, preview: Option<Box<FolderPreview>> },
    /// A repository git refuses because the folder belongs to another
    /// account; `t` trusts `trust` (the repository root).
    Untrusted { path: PathBuf, trust: String },
    /// Anything else, shown as git reported it.
    Failed { path: PathBuf, error: String },
}

pub enum Prompt {
    OpenPath(LineInput),
    CloneUrl(LineInput),
    ConfirmInit(InitChoice),
    ConfirmTrust,
}

/// The options shown when setting git up in a folder.
pub struct InitChoice {
    /// Add the recommended .gitignore rules.
    pub ignore: bool,
    /// Track the large files with Git LFS.
    pub lfs: bool,
    /// Which option the cursor is on (0 = ignore rules, 1 = LFS).
    pub row: usize,
}

impl InitChoice {
    fn for_preview(p: Option<&FolderPreview>) -> Self {
        Self {
            ignore: p.is_some_and(|p| !p.ignore_rules.is_empty()),
            lfs: p.is_some_and(|p| p.lfs_available && !p.lfs_patterns.is_empty()),
            row: 0,
        }
    }
}

pub enum Outcome {
    Nothing,
    Open(Repo),
}

pub struct Row {
    pub summary: RepoSummary,
    pub recent: bool,
}

enum Msg {
    Scanned(Vec<Row>),
    CloneProgress(String),
    Preview(PathBuf, Box<FolderPreview>),
    Cloned(Result<PathBuf, String>),
}

pub struct Picker {
    pub rows: Vec<Row>,
    pub sel: usize,
    pub off: usize,
    pub loading: bool,
    /// Folders scanned for nearby repositories.
    pub roots: Vec<PathBuf>,
    /// Where relative paths and clones go.
    pub base: PathBuf,
    /// The repository that is open, if any (marked in the list).
    pub current: Option<PathBuf>,
    pub target: Option<Target>,
    pub prompt: Option<Prompt>,
    pub message: Option<(String, bool)>,
    /// Clone progress while a clone runs.
    pub busy: Option<String>,
    /// Which row each drawn line shows (section headers are `None`), for mouse clicks.
    pub line_rows: Vec<Option<usize>>,
    recent: Vec<PathBuf>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Picker {
    pub fn new(roots: Vec<PathBuf>, base: PathBuf, current: Option<PathBuf>, recent: Vec<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rows: Vec::new(),
            sel: 0,
            off: 0,
            loading: false,
            roots,
            base,
            current: current.and_then(|c| c.canonicalize().ok()),
            target: None,
            prompt: None,
            message: None,
            busy: None,
            line_rows: Vec::new(),
            recent,
            tx,
            rx,
        }
    }

    /// Look for repositories again (recent ones first).
    pub fn scan(&mut self) {
        if self.loading {
            return;
        }
        self.loading = true;
        let (roots, recent, tx) = (self.roots.clone(), self.recent.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            let mut seen: Vec<PathBuf> = Vec::new();
            let mut rows: Vec<Row> = Vec::new();
            for p in recent.iter().filter(|p| p.is_dir()) {
                let c = canon(p);
                if !seen.contains(&c) {
                    seen.push(c);
                    rows.push(Row { summary: workspace::summarize(p), recent: true });
                }
            }
            let found: Vec<PathBuf> = workspace::find_repos(&roots, 2).into_iter().filter(|p| !seen.contains(&canon(p))).collect();
            let chunks: Vec<Vec<PathBuf>> = found.chunks(found.len().div_ceil(8).max(1)).map(|c| c.to_vec()).collect();
            let handles: Vec<_> = chunks
                .into_iter()
                .map(|chunk| std::thread::spawn(move || chunk.iter().map(|p| workspace::summarize(p)).collect::<Vec<_>>()))
                .collect();
            let mut nearby: Vec<RepoSummary> = handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect();
            nearby.sort_by(|a, b| b.last_commit.cmp(&a.last_commit).then(a.name.cmp(&b.name)));
            rows.extend(nearby.into_iter().map(|summary| Row { summary, recent: false }));
            let _ = tx.send(Msg::Scanned(rows));
        });
    }

    /// Pick up background results; true when something changed.
    pub fn tick(&mut self) -> Option<Outcome> {
        let mut out = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Scanned(rows) => {
                    self.loading = false;
                    self.rows = rows;
                    self.sel = self.rows.iter().position(|r| !self.is_current(&r.summary.path)).unwrap_or(0);
                    out.get_or_insert(Outcome::Nothing);
                }
                Msg::Preview(at, p) => {
                    if let Some(Target::NotARepo { path, preview, .. }) = &mut self.target {
                        if *path == at {
                            *preview = Some(p);
                        }
                    }
                    // options start from what the scan found
                    let choice = InitChoice::for_preview(self.target_preview());
                    if let Some(Prompt::ConfirmInit(c)) = &mut self.prompt {
                        *c = choice;
                    }
                    out.get_or_insert(Outcome::Nothing);
                }
                Msg::CloneProgress(line) => {
                    self.busy = Some(line);
                    out.get_or_insert(Outcome::Nothing);
                }
                Msg::Cloned(result) => {
                    self.busy = None;
                    out = Some(match result {
                        Ok(path) => self.try_open(&path),
                        Err(e) => {
                            self.message = Some((e, true));
                            Outcome::Nothing
                        }
                    });
                }
            }
        }
        out
    }

    pub fn is_current(&self, path: &Path) -> bool {
        self.current.is_some() && path.canonicalize().ok() == self.current
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.sel)
    }

    /// Select the row drawn on `line` of the list (after a mouse click).
    pub fn click_line(&mut self, line: usize) {
        if let Some(Some(i)) = self.line_rows.get(line) {
            self.sel = *i;
        }
    }

    /// Open `path`, or explain why it can't be opened and what would fix it.
    pub fn try_open(&mut self, path: &Path) -> Outcome {
        self.message = None;
        match Repo::discover(path) {
            Ok(repo) => {
                self.target = None;
                Outcome::Open(repo)
            }
            Err(e) => {
                self.set_target(path, e);
                Outcome::Nothing
            }
        }
    }

    /// The folder this screen was started in (or asked to open) couldn't be opened.
    pub fn set_target(&mut self, path: &Path, err: Error) {
        self.target = Some(target_for(path, err));
        if let Some(Target::NotARepo { path, .. }) = &self.target {
            let (path, tx) = (path.clone(), self.tx.clone());
            std::thread::spawn(move || {
                let p = preview::preview(&path);
                let _ = tx.send(Msg::Preview(path, Box::new(p)));
            });
        }
    }

    pub fn target_preview(&self) -> Option<&FolderPreview> {
        match &self.target {
            Some(Target::NotARepo { preview, .. }) => preview.as_deref(),
            _ => None,
        }
    }

    pub fn on_key(&mut self, key: &KeyEvent, keys: &Keymap) -> Outcome {
        if self.busy.is_some() {
            return Outcome::Nothing;
        }
        if let Some(prompt) = self.prompt.take() {
            return self.on_prompt_key(prompt, key);
        }
        let chord = Chord::from_event(key);
        let Some(action) = keys.find(keys::REPOS, chord) else { return Outcome::Nothing };
        let page = 10;
        let len = self.rows.len();
        match action {
            Action::Up => self.sel = self.sel.saturating_sub(1),
            Action::Down => self.sel = (self.sel + 1).min(len.saturating_sub(1)),
            Action::PageUp => self.sel = self.sel.saturating_sub(page),
            Action::PageDown => self.sel = (self.sel + page).min(len.saturating_sub(1)),
            Action::Top => self.sel = 0,
            Action::Bottom => self.sel = len.saturating_sub(1),
            Action::Open => {
                if let Some(path) = self.selected().map(|r| r.summary.path.clone()) {
                    if self.is_current(&path) {
                        self.message = Some(("That repository is already open.".into(), false));
                    } else {
                        return self.try_open(&path);
                    }
                }
            }
            Action::OpenFolder => {
                let mut input = LineInput::default();
                input.insert_str(&display_dir(&self.base));
                self.prompt = Some(Prompt::OpenPath(input));
            }
            Action::CloneRepo => self.prompt = Some(Prompt::CloneUrl(LineInput::default())),
            Action::InitHere if matches!(self.target, Some(Target::NotARepo { .. })) => {
                self.prompt = Some(Prompt::ConfirmInit(InitChoice::for_preview(self.target_preview())))
            }
            Action::TrustFolder if matches!(self.target, Some(Target::Untrusted { .. })) => self.prompt = Some(Prompt::ConfirmTrust),
            Action::Refresh => self.scan(),
            _ => {}
        }
        Outcome::Nothing
    }

    fn on_prompt_key(&mut self, prompt: Prompt, key: &KeyEvent) -> Outcome {
        match prompt {
            Prompt::OpenPath(mut input) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let path = resolve(&self.base, input.text.trim());
                    if !path.is_dir() {
                        self.message = Some((format!("{} isn't a folder.", path.display()), true));
                        self.prompt = Some(Prompt::OpenPath(input));
                    } else {
                        return self.try_open(&path);
                    }
                }
                _ => {
                    input.handle(key);
                    self.prompt = Some(Prompt::OpenPath(input));
                }
            },
            Prompt::CloneUrl(mut input) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter if !input.text.trim().is_empty() => self.start_clone(input.text.trim().to_string()),
                _ => {
                    input.handle(key);
                    self.prompt = Some(Prompt::CloneUrl(input));
                }
            },
            Prompt::ConfirmInit(mut c) => {
                let Some(Target::NotARepo { path, warning, preview }) = &self.target else { return Outcome::Nothing };
                let (has_rules, can_lfs) = match preview {
                    Some(p) => (!p.ignore_rules.is_empty(), p.lfs_available && !p.lfs_patterns.is_empty()),
                    None => (false, false),
                };
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => c.row = 0,
                    KeyCode::Down | KeyCode::Char('j') => c.row = 1,
                    KeyCode::Char(' ') if c.row == 0 && has_rules => c.ignore = !c.ignore,
                    KeyCode::Char(' ') if c.row == 1 && can_lfs => c.lfs = !c.lfs,
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => return Outcome::Nothing,
                    // with a warning, only an explicit y goes ahead
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter if key.code != KeyCode::Enter || warning.is_none() => {
                        let opts = InitOptions {
                            ignore_rules: if c.ignore { preview.as_ref().map(|p| p.ignore_rules.clone()).unwrap_or_default() } else { vec![] },
                            lfs_patterns: if c.lfs { preview.as_ref().map(|p| p.lfs_patterns.clone()).unwrap_or_default() } else { vec![] },
                        };
                        let path = path.clone();
                        return match preview::init_with(&path, &opts) {
                            Ok(_) => self.try_open(&path),
                            Err(e) => {
                                self.message = Some((format!("Couldn't set up git: {e}"), true));
                                Outcome::Nothing
                            }
                        };
                    }
                    _ => {}
                }
                self.prompt = Some(Prompt::ConfirmInit(c));
            }
            Prompt::ConfirmTrust => {
                let Some(Target::Untrusted { path, trust }) = &self.target else { return Outcome::Nothing };
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        let (path, trust) = (path.clone(), trust.clone());
                        return match gitty_core::repo::trust_folder(&trust) {
                            Ok(()) => self.try_open(&path),
                            Err(e) => {
                                self.message = Some((format!("Couldn't trust the folder: {e}"), true));
                                Outcome::Nothing
                            }
                        };
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {}
                    _ => self.prompt = Some(Prompt::ConfirmTrust),
                }
            }
        }
        Outcome::Nothing
    }

    /// Where a clone of `url` would land.
    pub fn clone_target(&self, url: &str) -> PathBuf {
        self.base.join(workspace::clone_dir_name(url))
    }

    fn start_clone(&mut self, url: String) {
        let name = workspace::clone_dir_name(&url);
        let (base, tx) = (self.base.clone(), self.tx.clone());
        self.message = None;
        self.busy = Some(format!("Cloning into {}…", base.join(&name).display()));
        std::thread::spawn(move || {
            let progress = tx.clone();
            let result = workspace::clone_repo(&url, &base, &name, |line| {
                let _ = progress.send(Msg::CloneProgress(line.trim().to_string()));
            });
            let _ = tx.send(Msg::Cloned(result.map_err(|e| e.to_string())));
        });
    }
}

pub fn target_for(path: &Path, err: Error) -> Target {
    let path = path.to_path_buf();
    match err {
        Error::NotARepo(_) => Target::NotARepo { warning: workspace::init_warning(&path), path, preview: None },
        Error::UntrustedRepo { path: trust } => Target::Untrusted { path, trust },
        e => Target::Failed { path, error: e.to_string() },
    }
}

/// `~` and relative paths, against `base`.
fn resolve(base: &Path, text: &str) -> PathBuf {
    let p = crate::config::expand(text);
    if p.is_absolute() { p } else { base.join(p) }
}

fn display_dir(p: &Path) -> String {
    let mut s = p.display().to_string();
    if !s.ends_with(std::path::MAIN_SEPARATOR) {
        s.push(std::path::MAIN_SEPARATOR);
    }
    s
}
