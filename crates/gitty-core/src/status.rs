//! Parser for `git status --porcelain=v2 -z --branch`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum EntryKind {
    Ordinary,
    Renamed,
    Unmerged,
    Untracked,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct StatusEntry {
    pub path: String,
    pub orig_path: Option<String>,
    /// Index status letter (`.` = unchanged).
    pub index: char,
    /// Worktree status letter (`.` = unchanged).
    pub worktree: char,
    pub kind: EntryKind,
}

impl StatusEntry {
    pub fn is_conflict(&self) -> bool {
        self.kind == EntryKind::Unmerged
    }

    pub fn is_untracked(&self) -> bool {
        self.kind == EntryKind::Untracked
    }

    pub fn is_staged(&self) -> bool {
        matches!(self.kind, EntryKind::Ordinary | EntryKind::Renamed) && self.index != '.'
    }

    pub fn is_unstaged(&self) -> bool {
        self.kind == EntryKind::Untracked
            || (matches!(self.kind, EntryKind::Ordinary | EntryKind::Renamed) && self.worktree != '.')
    }

    /// Untracked directories are reported collapsed, with a trailing slash.
    pub fn is_dir(&self) -> bool {
        self.path.ends_with('/')
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct BranchStatus {
    /// `None` when HEAD is detached.
    pub head: Option<String>,
    /// `None` when the branch has no commits yet.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Status {
    pub branch: BranchStatus,
    pub entries: Vec<StatusEntry>,
}

pub fn parse(bytes: &[u8]) -> Status {
    let text = String::from_utf8_lossy(bytes);
    let mut status = Status::default();
    let mut records = text.split('\0');
    while let Some(rec) = records.next() {
        if rec.is_empty() {
            continue;
        }
        if let Some(header) = rec.strip_prefix("# ") {
            let (key, val) = header.split_once(' ').unwrap_or((header, ""));
            let b = &mut status.branch;
            match key {
                "branch.oid" => b.oid = (val != "(initial)").then(|| val.to_string()),
                "branch.head" => b.head = (val != "(detached)").then(|| val.to_string()),
                "branch.upstream" => b.upstream = Some(val.to_string()),
                "branch.ab" => {
                    for part in val.split_whitespace() {
                        if let Some(n) = part.strip_prefix('+') {
                            b.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            b.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let (tag, rest) = rec.split_once(' ').unwrap_or((rec, ""));
        let entry = match tag {
            "1" => {
                // XY sub mH mI mW hH hI path
                let f: Vec<&str> = rest.splitn(8, ' ').collect();
                (f.len() == 8).then(|| entry(f[0], f[7], None, EntryKind::Ordinary))
            }
            "2" => {
                // XY sub mH mI mW hH hI Xscore path, then the original path as its own record
                let f: Vec<&str> = rest.splitn(9, ' ').collect();
                let orig = records.next().map(str::to_string);
                (f.len() == 9).then(|| entry(f[0], f[8], orig, EntryKind::Renamed))
            }
            "u" => {
                // XY sub m1 m2 m3 mW h1 h2 h3 path
                let f: Vec<&str> = rest.splitn(10, ' ').collect();
                (f.len() == 10).then(|| entry(f[0], f[9], None, EntryKind::Unmerged))
            }
            "?" => Some(StatusEntry {
                path: rest.to_string(),
                orig_path: None,
                index: '?',
                worktree: '?',
                kind: EntryKind::Untracked,
            }),
            _ => None,
        };
        if let Some(e) = entry {
            status.entries.push(e);
        }
    }
    status
}

fn entry(xy: &str, path: &str, orig: Option<String>, kind: EntryKind) -> StatusEntry {
    let mut c = xy.chars();
    StatusEntry {
        path: path.to_string(),
        orig_path: orig,
        index: c.next().unwrap_or('.'),
        worktree: c.next().unwrap_or('.'),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_record_kinds() {
        let raw = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa aaa src/a b.rs\0\
2 R. N... 100644 100644 100644 aaa aaa R100 new.rs\0old.rs\0\
u UU N... 100644 100644 100644 100644 a b c conflict.txt\0\
? Thiết kế/\0";
        let s = parse(raw.as_bytes());
        assert_eq!(s.branch.head.as_deref(), Some("main"));
        assert_eq!((s.branch.ahead, s.branch.behind), (2, 1));
        assert_eq!(s.entries.len(), 4);
        assert_eq!(s.entries[0].path, "src/a b.rs");
        assert!(s.entries[0].is_unstaged() && !s.entries[0].is_staged());
        assert_eq!(s.entries[1].orig_path.as_deref(), Some("old.rs"));
        assert!(s.entries[1].is_staged());
        assert!(s.entries[2].is_conflict());
        assert!(s.entries[3].is_untracked() && s.entries[3].is_dir());
    }

    #[test]
    fn detached_and_unborn() {
        let s = parse(b"# branch.oid (initial)\0# branch.head (detached)\0");
        assert_eq!(s.branch.head, None);
        assert_eq!(s.branch.oid, None);
    }
}
