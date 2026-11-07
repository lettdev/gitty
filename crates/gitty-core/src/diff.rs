//! Unified diff parsing and partial-patch construction for hunk/line staging.

use crate::git::Result;
use crate::repo::Repo;

/// Stop parsing new hunks after this many lines; huge generated files would
/// otherwise make the UI sluggish.
pub const MAX_DIFF_LINES: usize = 30_000;
/// Untracked files larger than this are not previewed.
pub const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
    /// `\ No newline at end of file`
    NoNewline,
}

impl LineKind {
    pub fn is_change(self) -> bool {
        matches!(self, LineKind::Add | LineKind::Del)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line content without the leading sign. May end in `\r` for CRLF files.
    pub text: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    /// Everything before the first hunk (`diff --git`, `index`, `---`, `+++` …).
    pub header: Vec<String>,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
    pub truncated: bool,
}

impl FileDiff {
    pub fn line_count(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len() + 1).sum()
    }

    pub fn max_line_no(&self) -> u32 {
        self.hunks
            .iter()
            .map(|h| (h.old_start + h.old_len).max(h.new_start + h.new_len))
            .max()
            .unwrap_or(0)
    }
}

fn parse_range(s: &str) -> (u32, u32) {
    let s = s.trim_start_matches(['-', '+']);
    match s.split_once(',') {
        Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(0)),
        None => (s.parse().unwrap_or(0), 1),
    }
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    // @@ -a,b +c,d @@ optional section
    let inner = line.strip_prefix("@@ ")?;
    let end = inner.find(" @@")?;
    let mut parts = inner[..end].split(' ');
    let (old_start, old_len) = parse_range(parts.next()?);
    let (new_start, new_len) = parse_range(parts.next()?);
    Some(Hunk { header: line.to_string(), old_start, old_len, new_start, new_len, lines: Vec::new() })
}

/// Parse `git diff` output, which may contain several files.
pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur: Option<FileDiff> = None;
    let mut hunk: Option<Hunk> = None;
    let (mut old_no, mut new_no) = (0u32, 0u32);
    let mut total = 0usize;

    fn close_hunk(cur: &mut Option<FileDiff>, hunk: &mut Option<Hunk>) {
        if let (Some(f), Some(h)) = (cur.as_mut(), hunk.take()) {
            f.hunks.push(h);
        }
    }

    for line in text.split('\n') {
        if line.starts_with("diff --git ") {
            close_hunk(&mut cur, &mut hunk);
            if let Some(f) = cur.take() {
                files.push(f);
            }
            cur = Some(FileDiff { header: vec![line.to_string()], ..Default::default() });
            continue;
        }
        let Some(file) = cur.as_mut() else { continue };
        if file.truncated {
            continue;
        }
        if line.starts_with("@@ ") {
            close_hunk(&mut cur, &mut hunk);
            let file = cur.as_mut().expect("file exists");
            if total >= MAX_DIFF_LINES {
                file.truncated = true;
                continue;
            }
            if let Some(h) = parse_hunk_header(line) {
                old_no = h.old_start;
                new_no = h.new_start;
                hunk = Some(h);
            }
            continue;
        }
        match hunk.as_mut() {
            Some(h) => {
                let (kind, text) = match line.as_bytes().first() {
                    Some(b' ') => (LineKind::Context, &line[1..]),
                    Some(b'+') => (LineKind::Add, &line[1..]),
                    Some(b'-') => (LineKind::Del, &line[1..]),
                    Some(b'\\') => (LineKind::NoNewline, line),
                    _ => continue,
                };
                let (o, n) = match kind {
                    LineKind::Context => {
                        old_no += 1;
                        new_no += 1;
                        (Some(old_no - 1), Some(new_no - 1))
                    }
                    LineKind::Add => {
                        new_no += 1;
                        (None, Some(new_no - 1))
                    }
                    LineKind::Del => {
                        old_no += 1;
                        (Some(old_no - 1), None)
                    }
                    LineKind::NoNewline => (None, None),
                };
                h.lines.push(DiffLine { kind, text: text.to_string(), old_no: o, new_no: n });
                total += 1;
            }
            None => {
                if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                    file.binary = true;
                }
                if !line.is_empty() {
                    file.header.push(line.to_string());
                }
            }
        }
    }
    close_hunk(&mut cur, &mut hunk);
    if let Some(f) = cur.take() {
        files.push(f);
    }
    files
}

/// Build a patch containing only the selected change lines.
///
/// * `reverse == false`: the patch is meant to be applied forward to the index
///   (staging part of the worktree diff).
/// * `reverse == true`: the patch is applied with `--reverse` (unstaging part of
///   the cached diff, or discarding part of the worktree diff).
///
/// Unselected changes are turned into context or dropped so the patch still
/// matches the side it is applied to. Hunk counts are left to `--recount`.
pub fn build_patch(fd: &FileDiff, is_selected: impl Fn(usize, usize) -> bool, reverse: bool) -> Option<String> {
    let mut body = String::new();
    for (hi, h) in fd.hunks.iter().enumerate() {
        let any = h.lines.iter().enumerate().any(|(li, l)| l.kind.is_change() && is_selected(hi, li));
        if !any {
            continue;
        }
        let mut lines = String::new();
        let mut prev_emitted = false;
        for (li, l) in h.lines.iter().enumerate() {
            let sel = is_selected(hi, li);
            let prefix = match l.kind {
                LineKind::Context => Some(' '),
                LineKind::Add if sel => Some('+'),
                LineKind::Add if reverse => Some(' '),
                LineKind::Add => None,
                LineKind::Del if sel => Some('-'),
                LineKind::Del if reverse => None,
                LineKind::Del => Some(' '),
                LineKind::NoNewline => {
                    if prev_emitted {
                        lines.push_str(&l.text);
                        lines.push('\n');
                    }
                    continue;
                }
            };
            match prefix {
                Some(p) => {
                    lines.push(p);
                    lines.push_str(&l.text);
                    lines.push('\n');
                    prev_emitted = true;
                }
                None => prev_emitted = false,
            }
        }
        body.push_str(&format!("@@ -{},{} +{},{} @@\n", h.old_start, h.old_len, h.new_start, h.new_len));
        body.push_str(&lines);
    }
    if body.is_empty() {
        return None;
    }
    let mut patch = fd.header.join("\n");
    patch.push('\n');
    patch.push_str(&body);
    Some(patch)
}

impl Repo {
    fn one_diff(&self, args: &[&str]) -> Result<FileDiff> {
        let out = self.git.run(args)?;
        let text = String::from_utf8_lossy(&out);
        Ok(parse(&text).into_iter().next().unwrap_or_default())
    }

    /// Unstaged changes of a tracked file (worktree vs index).
    pub fn diff_worktree(&self, path: &str) -> Result<FileDiff> {
        self.one_diff(&["diff", "--no-ext-diff", "--no-color", "-U3", "--", path])
    }

    /// Staged changes of a file (index vs HEAD).
    pub fn diff_index(&self, path: &str, orig: Option<&str>) -> Result<FileDiff> {
        let mut args = vec!["diff", "--cached", "--no-ext-diff", "--no-color", "-M", "-U3", "--"];
        if let Some(o) = orig {
            args.push(o);
        }
        args.push(path);
        self.one_diff(&args)
    }

    /// Show an untracked file as an all-added diff, without touching the index.
    pub fn diff_untracked(&self, path: &str) -> Result<FileDiff> {
        let full = self.root().join(path);
        let header = vec![
            format!("diff --git a/{path} b/{path}"),
            "new file mode 100644".to_string(),
            "--- /dev/null".to_string(),
            format!("+++ b/{path}"),
        ];
        let meta = std::fs::metadata(&full)?;
        if meta.len() > MAX_PREVIEW_BYTES {
            return Ok(FileDiff { header, binary: true, ..Default::default() });
        }
        let bytes = std::fs::read(&full)?;
        if bytes.iter().take(8000).any(|&b| b == 0) {
            return Ok(FileDiff { header, binary: true, ..Default::default() });
        }
        let text = String::from_utf8_lossy(&bytes);
        let ends_with_newline = text.ends_with('\n');
        let mut lines: Vec<DiffLine> = Vec::new();
        let body = text.strip_suffix('\n').unwrap_or(&text);
        let mut truncated = false;
        if !text.is_empty() {
            for (i, l) in body.split('\n').enumerate() {
                if i >= MAX_DIFF_LINES {
                    truncated = true;
                    break;
                }
                lines.push(DiffLine { kind: LineKind::Add, text: l.to_string(), old_no: None, new_no: Some(i as u32 + 1) });
            }
            if !ends_with_newline && !truncated {
                lines.push(DiffLine {
                    kind: LineKind::NoNewline,
                    text: "\\ No newline at end of file".into(),
                    old_no: None,
                    new_no: None,
                });
            }
        }
        let count = lines.iter().filter(|l| l.kind == LineKind::Add).count() as u32;
        let hunks = if lines.is_empty() {
            Vec::new()
        } else {
            vec![Hunk {
                header: format!("@@ -0,0 +1,{count} @@"),
                old_start: 0,
                old_len: 0,
                new_start: 1,
                new_len: count,
                lines,
            }]
        };
        Ok(FileDiff { header, hunks, binary: false, truncated })
    }

    /// One file's change inside a commit (against its first parent).
    pub fn diff_commit_file(&self, hash: &str, parent: Option<&str>, path: &str, orig: Option<&str>) -> Result<FileDiff> {
        let mut args: Vec<&str> = match parent {
            Some(p) => vec!["diff", "--no-ext-diff", "--no-color", "-M", "-U3", p, hash, "--"],
            None => vec!["diff-tree", "-p", "-r", "-M", "--root", "--no-commit-id", "--no-color", hash, "--"],
        };
        if let Some(o) = orig {
            args.push(o);
        }
        args.push(path);
        self.one_diff(&args)
    }

    /// Text of `rev:path` (e.g. `HEAD:a.txt`, `:a.txt` for the index), or of the
    /// worktree file when `rev` is `None`. Empty when it doesn't exist.
    pub fn read_version(&self, rev: Option<&str>, path: &str) -> String {
        match rev {
            None => std::fs::read(self.root().join(path)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default(),
            Some(r) => self.git.run(&["cat-file", "blob", &format!("{r}:{path}")]).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default(),
        }
    }

    /// Raw combined diff of a conflicted file (shown as plain text).
    pub fn diff_conflict(&self, path: &str) -> Result<String> {
        Ok(self.git.run_text(&["diff", "--no-ext-diff", "--no-color", "--", path])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/f.txt b/f.txt
index 1111111..2222222 100644
--- a/f.txt
+++ b/f.txt
@@ -1,4 +1,4 @@ intro
 one
-two
+TWO
+two and a half
 three
@@ -10,3 +10,2 @@
 ten
-eleven
 twelve
";

    #[test]
    fn parses_hunks_and_line_numbers() {
        let d = &parse(SAMPLE)[0];
        assert_eq!(d.hunks.len(), 2);
        let h = &d.hunks[0];
        assert_eq!((h.old_start, h.new_start), (1, 1));
        assert_eq!(h.lines[1].kind, LineKind::Del);
        assert_eq!(h.lines[1].old_no, Some(2));
        assert_eq!(h.lines[2].new_no, Some(2));
        assert_eq!(h.lines[4].old_no, Some(3));
        assert_eq!(h.lines[4].new_no, Some(4));
    }

    #[test]
    fn forward_patch_keeps_unselected_deletions_as_context() {
        let d = &parse(SAMPLE)[0];
        // select only the "+TWO" line in hunk 0
        let p = build_patch(d, |h, l| h == 0 && l == 2, false).unwrap();
        assert!(p.contains("\n two\n+TWO\n three\n"), "{p}");
        assert!(!p.contains("half"));
        assert!(!p.contains("eleven"));
    }

    #[test]
    fn reverse_patch_keeps_unselected_additions_as_context() {
        let d = &parse(SAMPLE)[0];
        let p = build_patch(d, |h, l| h == 0 && l == 1, true).unwrap();
        assert!(p.contains("-two\n TWO\n two and a half\n"), "{p}");
    }

    #[test]
    fn nothing_selected_gives_no_patch() {
        let d = &parse(SAMPLE)[0];
        assert!(build_patch(d, |_, _| false, false).is_none());
    }
}
