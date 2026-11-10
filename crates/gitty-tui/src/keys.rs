//! Actions, their default keys, and user overrides from the config file.

use std::collections::HashMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    pub fn from_event(ev: &KeyEvent) -> Self {
        let mut mods = ev.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let code = match ev.code {
            KeyCode::BackTab => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            KeyCode::Char(c) => {
                // the character already carries the case
                mods.remove(KeyModifiers::SHIFT);
                if mods.contains(KeyModifiers::CONTROL) { KeyCode::Char(c.to_ascii_lowercase()) } else { KeyCode::Char(c) }
            }
            other => other,
        };
        Self { code, mods }
    }

    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        if spec.is_empty() {
            return None;
        }
        let (mod_part, key) = if spec == "+" {
            ("", "+")
        } else if let Some(m) = spec.strip_suffix("++") {
            (m, "+")
        } else {
            match spec.rsplit_once('+') {
                Some((m, k)) => (m, k),
                None => ("", spec),
            }
        };
        let mut mods = KeyModifiers::NONE;
        for m in mod_part.split('+').filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
                "alt" | "option" | "meta" => mods |= KeyModifiers::ALT,
                "shift" => mods |= KeyModifiers::SHIFT,
                _ => return None,
            }
        }
        let lower = key.to_ascii_lowercase();
        let code = match lower.as_str() {
            "space" => KeyCode::Char(' '),
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" if mods.contains(KeyModifiers::SHIFT) => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => KeyCode::F(f[1..].parse().ok()?),
            _ => {
                let mut chars = key.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                let c = if mods.contains(KeyModifiers::SHIFT) { c.to_ascii_uppercase() } else { c };
                mods.remove(KeyModifiers::SHIFT);
                if mods.contains(KeyModifiers::CONTROL) { KeyCode::Char(c.to_ascii_lowercase()) } else { KeyCode::Char(c) }
            }
        };
        Some(Self { code, mods })
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            s.push_str("ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            s.push_str("alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            s.push_str("shift+");
        }
        match self.code {
            KeyCode::Char(' ') => s.push_str("space"),
            KeyCode::Char(c) => s.push(c),
            KeyCode::Enter => s.push_str("enter"),
            KeyCode::Esc => s.push_str("esc"),
            KeyCode::Tab => s.push_str("tab"),
            KeyCode::BackTab => s.push_str("shift+tab"),
            KeyCode::Backspace => s.push_str("backspace"),
            KeyCode::Delete => s.push_str("del"),
            KeyCode::Up => s.push_str("up"),
            KeyCode::Down => s.push_str("down"),
            KeyCode::Left => s.push_str("left"),
            KeyCode::Right => s.push_str("right"),
            KeyCode::PageUp => s.push_str("pgup"),
            KeyCode::PageDown => s.push_str("pgdn"),
            KeyCode::Home => s.push_str("home"),
            KeyCode::End => s.push_str("end"),
            KeyCode::F(n) => s.push_str(&format!("f{n}")),
            _ => s.push('?'),
        }
        s
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Quit,
    Help,
    ViewChanges,
    ViewHistory,
    ViewBranches,
    ViewInsight,
    ViewRepos,
    NextFocus,
    PrevFocus,
    Undo,
    Refresh,
    Fetch,
    Pull,
    Push,
    GitCommand,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Open,
    Back,
    Stage,
    StageAll,
    Discard,
    Commit,
    Amend,
    CommitEditor,
    SelectLines,
    NextHunk,
    PrevHunk,
    ToggleAllBranches,
    CopyHash,
    NewBranch,
    Checkout,
    DeleteBranch,
    RenameBranch,
    StashPush,
    StashPop,
    OpMenu,
    Rebase,
    TakeOurs,
    TakeTheirs,
    TakeBoth,
    TakeBase,
    FileOurs,
    FileTheirs,
    ShowBase,
    EditFile,
    RewordCommit,
    IgnoreFile,
    IgnoreRules,
    About,
    OpenFolder,
    CloneRepo,
    InitHere,
    TrustFolder,
}

pub struct ActionInfo {
    pub action: Action,
    pub name: &'static str,
    pub keys: &'static [&'static str],
    pub desc: &'static str,
}

macro_rules! actions {
    ($($a:ident, $name:literal, [$($k:literal),*], $desc:literal;)*) => {
        pub const ACTIONS: &[ActionInfo] = &[$(ActionInfo { action: Action::$a, name: $name, keys: &[$($k),*], desc: $desc }),*];
    };
}

actions! {
    Quit, "quit", ["q", "ctrl+c"], "Quit gitty";
    Help, "help", ["?"], "Show all keys";
    About, "about", [], "About gitty (also: a in the keys screen)";
    OpenFolder, "open_folder", ["o"], "Repositories: open any folder by path";
    CloneRepo, "clone", ["c"], "Repositories: clone from a URL";
    InitHere, "init_here", ["i"], "Repositories: set up git in a folder that has none";
    TrustFolder, "trust_folder", ["t"], "Repositories: trust a repository owned by another account";
    ViewChanges, "view_changes", ["1"], "Changes view";
    ViewHistory, "view_history", ["2"], "History view";
    ViewBranches, "view_branches", ["3"], "Branches & stashes view";
    ViewInsight, "view_insight", ["4"], "Insight view";
    ViewRepos, "view_repos", ["5"], "Repositories view";
    NextFocus, "next_focus", ["tab"], "Next pane";
    PrevFocus, "prev_focus", ["backtab"], "Previous pane";
    Undo, "undo", ["u", "ctrl+z"], "Undo the last gitty operation";
    Refresh, "refresh", ["R"], "Reload everything";
    Fetch, "fetch", ["f"], "Fetch all remotes";
    Pull, "pull", ["p"], "Pull the current branch";
    Push, "push", ["P"], "Push the current branch";
    GitCommand, "git_command", [":"], "Run any git command in the terminal";
    Up, "up", ["k", "up"], "Move up";
    Down, "down", ["j", "down"], "Move down";
    PageUp, "page_up", ["ctrl+u", "pageup"], "Page up";
    PageDown, "page_down", ["ctrl+d", "pagedown"], "Page down";
    Top, "top", ["g", "home"], "Go to top";
    Bottom, "bottom", ["G", "end"], "Go to bottom";
    Open, "open", ["enter", "l", "right"], "Open / go deeper";
    Back, "back", ["esc", "h", "left"], "Go back";
    Stage, "stage", ["space"], "Stage / unstage (file, hunk or selected lines)";
    StageAll, "stage_all", ["a"], "Stage all / unstage all";
    Discard, "discard", ["d"], "Discard changes (undoable)";
    Commit, "commit", ["c"], "Commit staged changes";
    Amend, "amend", ["A"], "Amend the last commit";
    CommitEditor, "commit_editor", ["C"], "Commit using $EDITOR";
    SelectLines, "select_lines", ["v"], "Start / stop selecting lines";
    NextHunk, "next_hunk", ["]", "n"], "Next hunk";
    PrevHunk, "prev_hunk", ["[", "N"], "Previous hunk";
    ToggleAllBranches, "toggle_all_branches", ["a"], "All branches / current branch only";
    CopyHash, "copy_hash", ["y"], "Copy commit hash";
    NewBranch, "new_branch", ["b"], "New branch from here";
    Checkout, "checkout", ["space", "enter"], "Switch to branch";
    DeleteBranch, "delete_branch", ["D"], "Delete branch";
    RenameBranch, "rename_branch", ["r"], "Rename branch";
    StashPush, "stash", ["s"], "Stash all changes";
    StashPop, "stash_pop", ["o"], "Pop the selected stash";
    OpMenu, "operation_menu", ["m"], "Continue / abort a merge or rebase";
    Rebase, "rebase", ["r"], "Interactive rebase from the selected commit";
    TakeOurs, "take_ours", ["o"], "Conflict: keep our side";
    TakeTheirs, "take_theirs", ["t"], "Conflict: keep their side";
    TakeBoth, "take_both", ["b"], "Conflict: keep both (ours first)";
    TakeBase, "take_base", ["e"], "Conflict: keep the original (base)";
    FileOurs, "file_ours", ["O"], "Conflict: our version of the whole file";
    FileTheirs, "file_theirs", ["T"], "Conflict: their version of the whole file";
    ShowBase, "show_base", ["B"], "Conflict: show the original between the sides";
    EditFile, "edit_file", ["E"], "Open the file in your editor";
    RewordCommit, "edit_message", ["e"], "Edit the message of the selected commit";
    IgnoreFile, "ignore", ["i"], "Ignore this file, its type or its folder";
    IgnoreRules, "ignore_rules", ["I"], "View and edit ignore rules";
}

pub const GLOBAL: &[Action] = &[
    Action::Quit,
    Action::Help,
    Action::About,
    Action::ViewChanges,
    Action::ViewHistory,
    Action::ViewBranches,
    Action::ViewInsight,
    Action::ViewRepos,
    Action::NextFocus,
    Action::PrevFocus,
    Action::Undo,
    Action::Refresh,
    Action::Fetch,
    Action::Pull,
    Action::Push,
    Action::GitCommand,
    Action::StashPush,
    Action::OpMenu,
    Action::IgnoreRules,
];

const NAV: [Action; 6] = [Action::Up, Action::Down, Action::PageUp, Action::PageDown, Action::Top, Action::Bottom];

pub const FILES: &[Action] = &[
    Action::IgnoreFile,
    Action::Stage,
    Action::StageAll,
    Action::Discard,
    Action::Commit,
    Action::Amend,
    Action::CommitEditor,
    Action::Open,
    NAV[0],
    NAV[1],
    NAV[2],
    NAV[3],
    NAV[4],
    NAV[5],
];

pub const DIFF: &[Action] = &[
    Action::Stage,
    Action::SelectLines,
    Action::NextHunk,
    Action::PrevHunk,
    Action::Discard,
    Action::Commit,
    Action::Amend,
    Action::CommitEditor,
    Action::Back,
    NAV[0],
    NAV[1],
    NAV[2],
    NAV[3],
    NAV[4],
    NAV[5],
];

pub const COMMITS: &[Action] = &[
    Action::RewordCommit,
    Action::Rebase,
    Action::ToggleAllBranches,
    Action::CopyHash,
    Action::NewBranch,
    Action::Open,
    NAV[0],
    NAV[1],
    NAV[2],
    NAV[3],
    NAV[4],
    NAV[5],
];

pub const COMMIT_FILES: &[Action] = &[Action::Open, Action::Back, NAV[0], NAV[1], NAV[2], NAV[3], NAV[4], NAV[5]];

pub const COMMIT_DIFF: &[Action] =
    &[Action::Back, Action::NextHunk, Action::PrevHunk, NAV[0], NAV[1], NAV[2], NAV[3], NAV[4], NAV[5]];

pub const BRANCHES: &[Action] = &[
    Action::StashPop,
    Action::Checkout,
    Action::NewBranch,
    Action::DeleteBranch,
    Action::RenameBranch,
    NAV[0],
    NAV[1],
    NAV[2],
    NAV[3],
    NAV[4],
    NAV[5],
];

pub const CONFLICT: &[Action] = &[
    Action::TakeOurs,
    Action::TakeTheirs,
    Action::TakeBoth,
    Action::TakeBase,
    Action::FileOurs,
    Action::FileTheirs,
    Action::ShowBase,
    Action::EditFile,
    Action::Stage,
    Action::NextHunk,
    Action::PrevHunk,
    Action::Back,
];

pub const HEALTH: &[Action] = &[Action::Open, Action::Stage, NAV[0], NAV[1], NAV[2], NAV[3], NAV[4], NAV[5]];
pub const INSIGHT_LIST: &[Action] = &[Action::Open, Action::Back, NAV[0], NAV[1], NAV[2], NAV[3], NAV[4], NAV[5]];
pub const REPOS: &[Action] = &[
    Action::Open,
    Action::OpenFolder,
    Action::CloneRepo,
    Action::InitHere,
    Action::TrustFolder,
    Action::Refresh,
    NAV[0],
    NAV[1],
    NAV[2],
    NAV[3],
    NAV[4],
    NAV[5],
];

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum KeySpec {
    One(String),
    Many(Vec<String>),
}

impl KeySpec {
    fn list(&self) -> Vec<String> {
        match self {
            KeySpec::One(s) => vec![s.clone()],
            KeySpec::Many(v) => v.clone(),
        }
    }
}

pub struct Keymap {
    binds: HashMap<Action, Vec<Chord>>,
}

impl Keymap {
    pub fn new(overrides: &HashMap<String, KeySpec>) -> (Self, Vec<String>) {
        let mut binds = HashMap::new();
        for info in ACTIONS {
            binds.insert(info.action, info.keys.iter().filter_map(|k| Chord::parse(k)).collect());
        }
        let mut warnings = Vec::new();
        let mut names: Vec<&String> = overrides.keys().collect();
        names.sort();
        for name in names {
            let Some(info) = ACTIONS.iter().find(|i| i.name == name.as_str()) else {
                warnings.push(format!("unknown action \"{name}\" in [keys]"));
                continue;
            };
            let mut chords = Vec::new();
            for k in overrides[name].list() {
                match Chord::parse(&k) {
                    Some(c) => chords.push(c),
                    None => warnings.push(format!("unknown key \"{k}\" for {name}")),
                }
            }
            binds.insert(info.action, chords);
        }
        (Self { binds }, warnings)
    }

    pub fn find(&self, actions: &[Action], chord: Chord) -> Option<Action> {
        actions.iter().copied().find(|a| self.binds.get(a).is_some_and(|v| v.contains(&chord)))
    }

    /// First key of an action, for compact hints.
    pub fn key(&self, a: Action) -> String {
        self.binds.get(&a).and_then(|v| v.first()).map(Chord::label).unwrap_or_else(|| "-".into())
    }

    pub fn all_keys(&self, a: Action) -> String {
        self.binds.get(&a).map(|v| v.iter().map(Chord::label).collect::<Vec<_>>().join(" / ")).unwrap_or_default()
    }
}

pub fn info(a: Action) -> &'static ActionInfo {
    ACTIONS.iter().find(|i| i.action == a).expect("every action has info")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keys() {
        assert_eq!(Chord::parse("space").unwrap().code, KeyCode::Char(' '));
        let c = Chord::parse("ctrl+d").unwrap();
        assert_eq!((c.code, c.mods), (KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(Chord::parse("shift+a").unwrap(), Chord { code: KeyCode::Char('A'), mods: KeyModifiers::NONE });
        assert_eq!(Chord::parse("G").unwrap().code, KeyCode::Char('G'));
        assert_eq!(Chord::parse("+").unwrap().code, KeyCode::Char('+'));
        assert_eq!(Chord::parse("ctrl++").unwrap().code, KeyCode::Char('+'));
        assert_eq!(Chord::parse("shift+tab").unwrap().code, KeyCode::BackTab);
        assert!(Chord::parse("hyper+x").is_none());
    }

    #[test]
    fn shifted_letter_event_matches() {
        let ev = KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT);
        assert_eq!(Chord::from_event(&ev), Chord::parse("P").unwrap());
    }

    #[test]
    fn overrides_replace_defaults() {
        let mut o = HashMap::new();
        o.insert("stage".to_string(), KeySpec::One("s".into()));
        o.insert("nope".to_string(), KeySpec::One("x".into()));
        let (k, w) = Keymap::new(&o);
        assert_eq!(k.find(FILES, Chord::parse("s").unwrap()), Some(Action::Stage));
        assert_eq!(k.find(FILES, Chord::parse("space").unwrap()), None);
        assert_eq!(w.len(), 1);
    }
}
