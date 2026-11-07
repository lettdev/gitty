//! Commit history and commit details.

use crate::git::{Error, Result};
use crate::repo::Repo;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum RefKind {
    /// HEAD; `name` is the checked-out branch, or "HEAD" when detached.
    Head,
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RefLabel {
    pub kind: RefKind,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub refs: Vec<RefLabel>,
    pub subject: String,
}

impl Commit {
    pub fn short(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }

    pub fn is_head(&self) -> bool {
        self.refs.iter().any(|r| r.kind == RefKind::Head)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CommitFile {
    pub status: char,
    pub path: String,
    pub orig: Option<String>,
    pub adds: Option<u32>,
    pub dels: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CommitDetail {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub date: String,
    pub message: String,
    pub files: Vec<CommitFile>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LogQuery {
    /// Revisions to walk, e.g. `["HEAD"]` or `["--branches", "--remotes", "--tags", "HEAD"]`.
    pub revs: Vec<String>,
    pub skip: usize,
    pub limit: usize,
}

const LOG_FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s%x1e";

pub fn parse_refs(decoration: &str) -> Vec<RefLabel> {
    decoration
        .split(", ")
        .filter(|s| !s.is_empty())
        .filter_map(|r| {
            let (kind, name) = if let Some(b) = r.strip_prefix("HEAD -> ") {
                (RefKind::Head, b.strip_prefix("refs/heads/").unwrap_or(b))
            } else if r == "HEAD" {
                (RefKind::Head, "HEAD")
            } else if let Some(t) = r.strip_prefix("tag: ") {
                (RefKind::Tag, t.strip_prefix("refs/tags/").unwrap_or(t))
            } else if let Some(b) = r.strip_prefix("refs/heads/") {
                (RefKind::Local, b)
            } else if let Some(b) = r.strip_prefix("refs/remotes/") {
                if b.ends_with("/HEAD") {
                    return None;
                }
                (RefKind::Remote, b)
            } else if r.starts_with("refs/stash") {
                return None;
            } else {
                (RefKind::Local, r)
            };
            Some(RefLabel { kind, name: name.to_string() })
        })
        .collect()
}

fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\x1e')
        .map(|r| r.trim_start_matches('\n'))
        .filter(|r| !r.is_empty())
        .filter_map(|r| {
            let f: Vec<&str> = r.splitn(7, '\x1f').collect();
            if f.len() < 7 {
                return None;
            }
            Some(Commit {
                hash: f[0].to_string(),
                parents: f[1].split_whitespace().map(str::to_string).collect(),
                author: f[2].to_string(),
                email: f[3].to_string(),
                time: f[4].parse().unwrap_or(0),
                refs: parse_refs(f[5]),
                subject: f[6].to_string(),
            })
        })
        .collect()
}

impl Repo {
    pub fn log(&self, q: &LogQuery) -> Result<Vec<Commit>> {
        if q.revs.is_empty() {
            return Ok(Vec::new());
        }
        let skip = format!("--skip={}", q.skip);
        let limit = format!("-n{}", q.limit);
        let mut args = vec!["log", "--topo-order", "--no-color", "--decorate=full", LOG_FORMAT, &skip, &limit];
        args.extend(q.revs.iter().map(String::as_str));
        args.push("--");
        match self.git.run_text(&args) {
            Ok(text) => Ok(parse_log(&text)),
            Err(Error::Command { stderr, .. })
                if stderr.contains("does not have any commits") || stderr.contains("unknown revision") =>
            {
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        }
    }

    /// Commits on any branch whose message, author or hash matches `query`
    /// (case-insensitive), newest first.
    pub fn search_log(&self, query: &str, limit: usize) -> Result<Vec<Commit>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let limit_arg = format!("-n{limit}");
        let run = |filter: String| -> Result<Vec<Commit>> {
            let args = ["log", "--all", "-i", "--fixed-strings", "--no-color", "--decorate=full", LOG_FORMAT, &limit_arg, &filter, "--"];
            match self.git.run_text(&args) {
                Ok(t) => Ok(parse_log(&t)),
                Err(Error::Command { .. }) => Ok(Vec::new()),
                Err(e) => Err(e),
            }
        };
        let mut found = run(format!("--grep={q}"))?;
        for c in run(format!("--author={q}"))? {
            if !found.iter().any(|f| f.hash == c.hash) {
                found.push(c);
            }
        }
        if q.len() >= 4 && q.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(t) = self.git.run_text(&["log", "-1", "--no-color", "--decorate=full", LOG_FORMAT, q, "--"]) {
                for c in parse_log(&t) {
                    if !found.iter().any(|f| f.hash == c.hash) {
                        found.insert(0, c);
                    }
                }
            }
        }
        found.sort_by_key(|c| std::cmp::Reverse(c.time));
        found.truncate(limit);
        Ok(found)
    }

    pub fn commit_detail(&self, hash: &str) -> Result<CommitDetail> {
        let meta = self.git.run_text(&[
            "show",
            "-s",
            "--no-color",
            "--date=format-local:%Y-%m-%d %H:%M",
            "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%ad%x1f%B",
            hash,
        ])?;
        let f: Vec<&str> = meta.splitn(6, '\x1f').collect();
        if f.len() < 6 {
            return Err(Error::Msg(format!("could not read commit {hash}")));
        }
        let parents: Vec<String> = f[1].split_whitespace().map(str::to_string).collect();
        let files = self.commit_files(hash, parents.first().map(String::as_str))?;
        Ok(CommitDetail {
            hash: f[0].to_string(),
            parents,
            author: f[2].to_string(),
            email: f[3].to_string(),
            date: f[4].to_string(),
            message: f[5].trim_end().to_string(),
            files,
        })
    }

    fn commit_files(&self, hash: &str, parent: Option<&str>) -> Result<Vec<CommitFile>> {
        let base: Vec<&str> = match parent {
            Some(p) => vec!["diff", "--no-color", "-M", "-z", p, hash],
            None => vec!["diff-tree", "--root", "-r", "-M", "--no-commit-id", "-z", hash],
        };
        let with = |flag: &'static str| -> Result<String> {
            let mut a = base.clone();
            a.insert(1, flag);
            self.git.run_text(&a)
        };
        let names = with("--name-status")?;
        let nums = with("--numstat")?;
        let mut files = parse_name_status(&names);
        for (file, (a, d)) in files.iter_mut().zip(parse_numstat(&nums)) {
            file.adds = a;
            file.dels = d;
        }
        Ok(files)
    }
}

fn parse_name_status(text: &str) -> Vec<CommitFile> {
    let mut out = Vec::new();
    let mut it = text.split('\0').filter(|s| !s.is_empty());
    while let Some(st) = it.next() {
        let status = st.chars().next().unwrap_or('M');
        if matches!(status, 'R' | 'C') {
            let orig = it.next().unwrap_or_default().to_string();
            let path = it.next().unwrap_or_default().to_string();
            out.push(CommitFile { status, path, orig: Some(orig), adds: None, dels: None });
        } else {
            let path = it.next().unwrap_or_default().to_string();
            out.push(CommitFile { status, path, orig: None, adds: None, dels: None });
        }
    }
    out
}

fn parse_numstat(text: &str) -> Vec<(Option<u32>, Option<u32>)> {
    let mut out = Vec::new();
    let mut it = text.split('\0');
    while let Some(rec) = it.next() {
        if rec.is_empty() {
            continue;
        }
        let mut f = rec.splitn(3, '\t');
        let a = f.next().and_then(|v| v.parse().ok());
        let d = f.next().and_then(|v| v.parse().ok());
        if f.next().unwrap_or_default().is_empty() {
            // rename: the two paths follow as separate records
            it.next();
            it.next();
        }
        out.push((a, d));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorations() {
        let r = parse_refs("HEAD -> refs/heads/feat/x, refs/remotes/origin/feat/x, refs/remotes/origin/HEAD, tag: refs/tags/v1");
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], RefLabel { kind: RefKind::Head, name: "feat/x".into() });
        assert_eq!(r[1].kind, RefKind::Remote);
        assert_eq!(r[2], RefLabel { kind: RefKind::Tag, name: "v1".into() });
    }

    #[test]
    fn numstat_with_rename() {
        let n = parse_numstat("1\t2\ta.txt\0-\t-\timg.png\x003\t0\t\0old.rs\0new.rs\0");
        assert_eq!(n, vec![(Some(1), Some(2)), (None, None), (Some(3), Some(0))]);
        let s = parse_name_status("M\0a.txt\0M\0img.png\0R090\0old.rs\0new.rs\0");
        assert_eq!(s[2].orig.as_deref(), Some("old.rs"));
        assert_eq!(s[2].path, "new.rs");
    }
}
