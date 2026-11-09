use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::keys::KeySpec;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// auto | unicode | nerd | ascii
    pub symbols: String,
    /// auto | truecolor | 256 | 16 | none
    pub color: String,
    /// Capture the mouse (hold Shift/Option to select text in the terminal).
    pub mouse: bool,
    /// Show all branches in History (toggle with `a`).
    pub all_branches: bool,
    /// Show what a push will send and wait for Enter before pushing.
    pub confirm_push: bool,
    /// Folders scanned for repositories in the Repos view (default: the
    /// folder containing the current repository).
    pub workspaces: Vec<String>,
    /// Action name → key or list of keys, e.g. `stage = "s"`.
    pub keys: HashMap<String, KeySpec>,
}

impl Default for Config {
    fn default() -> Self {
        Self { symbols: "auto".into(), color: "auto".into(), mouse: true, all_branches: true, confirm_push: true, workspaces: Vec::new(), keys: HashMap::new() }
    }
}

pub fn path() -> Option<PathBuf> {
    if cfg!(windows) {
        return dirs::config_dir().map(|d| d.join("gitty").join("config.toml"));
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|d| d.join("gitty").join("config.toml"))
}

/// Expand a leading `~` to the home folder.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest.trim_start_matches(['/', '\\']))).unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

/// Load the config; a broken file falls back to defaults plus an error message.
pub fn load() -> (Config, Option<String>) {
    let Some(p) = path() else { return (Config::default(), None) };
    let Ok(text) = std::fs::read_to_string(&p) else { return (Config::default(), None) };
    match toml::from_str::<Config>(&text) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(format!("{}:\n{e}", p.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example() {
        let c: Config = toml::from_str("symbols = \"ascii\"\nmouse = false\n[keys]\nstage = \"s\"\nquit = [\"q\", \"ctrl+q\"]\n").unwrap();
        assert_eq!(c.symbols, "ascii");
        assert!(!c.mouse);
        assert_eq!(c.color, "auto");
        assert_eq!(c.keys.len(), 2);
    }
}
