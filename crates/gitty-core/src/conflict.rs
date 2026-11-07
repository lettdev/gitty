//! Parse and resolve conflict markers in a worktree file.

use crate::git::{Error, Result};
use crate::oplog::OpKind;
use crate::repo::Repo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// Index of the `<<<<<<<` line.
    pub start: usize,
    /// Index of the `>>>>>>>` line.
    pub end: usize,
    pub ours: Vec<String>,
    /// Present when the file uses diff3/zdiff3 markers.
    pub base: Option<Vec<String>>,
    pub theirs: Vec<String>,
    pub ours_label: String,
    pub theirs_label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConflictFile {
    /// Lines including their line endings.
    pub lines: Vec<String>,
    pub regions: Vec<Region>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Ours,
    Theirs,
    /// Ours, then theirs.
    Both,
    Base,
}

fn marker(line: &str, m: &str) -> Option<String> {
    let t = line.trim_end_matches(['\n', '\r']);
    let rest = t.strip_prefix(m)?;
    if rest.is_empty() || rest.starts_with(' ') {
        Some(rest.trim().to_string())
    } else {
        None
    }
}

pub fn parse(text: &str) -> ConflictFile {
    let lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    let mut regions = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(ours_label) = marker(&lines[i], "<<<<<<<") else {
            i += 1;
            continue;
        };
        let start = i;
        let (mut ours, mut base, mut theirs) = (Vec::new(), None::<Vec<String>>, Vec::new());
        let mut section = 0; // 0 ours, 1 base, 2 theirs
        let mut end = None;
        let mut theirs_label = String::new();
        i += 1;
        while i < lines.len() {
            let l = &lines[i];
            if section == 0 && marker(l, "|||||||").is_some() {
                section = 1;
                base = Some(Vec::new());
            } else if section < 2 && marker(l, "=======").is_some() {
                section = 2;
            } else if section == 2 && marker(l, ">>>>>>>").is_some() {
                theirs_label = marker(l, ">>>>>>>").unwrap_or_default();
                end = Some(i);
                break;
            } else {
                let content = l.clone();
                match section {
                    0 => ours.push(content),
                    1 => base.get_or_insert_with(Vec::new).push(content),
                    _ => theirs.push(content),
                }
            }
            i += 1;
        }
        if let Some(end) = end {
            regions.push(Region { start, end, ours, base, theirs, ours_label, theirs_label });
        }
        i += 1;
    }
    ConflictFile { lines, regions }
}

/// The file text with region `idx` replaced by the chosen side.
pub fn resolve(cf: &ConflictFile, idx: usize, pick: Pick) -> Option<String> {
    let r = cf.regions.get(idx)?;
    let chosen: Vec<String> = match pick {
        Pick::Ours => r.ours.clone(),
        Pick::Theirs => r.theirs.clone(),
        Pick::Both => r.ours.iter().chain(r.theirs.iter()).cloned().collect(),
        Pick::Base => r.base.clone()?,
    };
    let mut out = String::new();
    for l in &cf.lines[..r.start] {
        out.push_str(l);
    }
    for l in &chosen {
        out.push_str(l);
    }
    for l in &cf.lines[r.end + 1..] {
        out.push_str(l);
    }
    Some(out)
}

impl Repo {
    pub fn read_conflict(&self, path: &str) -> Result<ConflictFile> {
        let bytes = std::fs::read(self.root().join(path))?;
        if bytes.iter().take(8000).any(|&b| b == 0) {
            return Err(Error::Msg("Binary file — pick a whole side instead.".into()));
        }
        Ok(parse(&String::from_utf8_lossy(&bytes)))
    }

    /// Write a (partially) resolved file. Undoable.
    pub fn write_resolution(&self, path: &str, text: &str, what: &str) -> Result<String> {
        self.record(OpKind::Edit, format!("{what} in {path}"), |r, backups| {
            r.backup_path(path, backups)?;
            std::fs::write(r.root().join(path), text)?;
            Ok(format!("resolve {path}"))
        })
    }

    /// Take one side for the whole file (works for binary files too).
    pub fn take_side(&self, path: &str, ours: bool) -> Result<String> {
        let flag = if ours { "--ours" } else { "--theirs" };
        let side = if ours { "ours" } else { "theirs" };
        self.record(OpKind::Edit, format!("Take {side} for {path}"), |r, backups| {
            r.backup_path(path, backups)?;
            r.git.run(&["checkout", flag, "--", path])?;
            Ok(format!("git checkout {flag} -- {path}"))
        })
    }

    /// Re-create the markers with the common ancestor shown (diff3 style).
    /// Throws away manual edits to the file, so it is undoable.
    pub fn show_conflict_base(&self, path: &str) -> Result<String> {
        self.record(OpKind::Edit, format!("Show base in {path}"), |r, backups| {
            r.backup_path(path, backups)?;
            r.git.run(&["checkout", "--conflict=diff3", "--", path])?;
            Ok(format!("git checkout --conflict=diff3 -- {path}"))
        })
    }

    /// Resolve a delete/modify conflict by deleting the file.
    pub fn resolve_delete(&self, path: &str) -> Result<String> {
        self.record(OpKind::Edit, format!("Delete {path}"), |r, backups| {
            r.backup_path(path, backups)?;
            r.git.run(&["rm", "-q", "--", path])?;
            Ok(format!("git rm -- {path}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\n<<<<<<< HEAD\nours 1\nours 2\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> feat\nb\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feat\n";

    #[test]
    fn parse_diff3_and_merge_styles() {
        let cf = parse(TEXT);
        assert_eq!(cf.regions.len(), 2);
        let r = &cf.regions[0];
        assert_eq!(r.ours, vec!["ours 1\n", "ours 2\n"]);
        assert_eq!(r.base.as_deref(), Some(&["base\n".to_string()][..]));
        assert_eq!(r.theirs, vec!["theirs\n"]);
        assert_eq!((r.ours_label.as_str(), r.theirs_label.as_str()), ("HEAD", "feat"));
        assert_eq!(cf.regions[1].base, None);
    }

    #[test]
    fn resolve_regions_one_by_one() {
        let cf = parse(TEXT);
        let once = resolve(&cf, 0, Pick::Theirs).unwrap();
        let cf2 = parse(&once);
        assert_eq!(cf2.regions.len(), 1);
        let done = resolve(&cf2, 0, Pick::Both).unwrap();
        assert_eq!(done, "a\ntheirs\nb\nx\ny\n");
        assert!(resolve(&cf2, 0, Pick::Base).is_none());
    }
}
