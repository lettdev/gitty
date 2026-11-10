//! Repositories opened recently, newest first (shared by every gitty window).

use std::path::{Path, PathBuf};

const MAX: usize = 20;

fn path() -> Option<PathBuf> {
    // tests open many throwaway repositories; keep them out of the real list
    if cfg!(test) {
        return None;
    }
    dirs::data_local_dir().map(|d| d.join("gitty").join("recent.json"))
}

pub fn load() -> Vec<PathBuf> {
    let Some(p) = path() else { return Vec::new() };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<PathBuf>>(&t).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}

/// Move `root` to the front of the list.
pub fn remember(root: &Path) {
    let Some(p) = path() else { return };
    let mut list = load();
    let same = |a: &Path| a.canonicalize().ok() == root.canonicalize().ok();
    list.retain(|r| !same(r));
    list.insert(0, root.to_path_buf());
    list.truncate(MAX);
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(p, text);
    }
}
