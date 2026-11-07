//! Ignore rules: where they come from, why a path is ignored, and editing them.

use std::fs;
use std::path::PathBuf;

use crate::git::{Error, Result};
use crate::oplog::OpKind;
use crate::repo::Repo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoreRule {
    /// 1-based line number in the file.
    pub line: usize,
    pub text: String,
    pub comment: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoreSource {
    pub label: String,
    /// Path relative to the repo root, when the file lives inside it (editable
    /// with undo). `None` for the global excludes file.
    pub rel: Option<String>,
    pub abs: PathBuf,
    pub exists: bool,
    pub rules: Vec<IgnoreRule>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhyIgnored {
    pub source: String,
    pub line: usize,
    pub pattern: String,
    /// The matching rule is a `!negation`, so the path is NOT ignored.
    pub negated: bool,
}

fn read_rules(path: &std::path::Path) -> (bool, Vec<IgnoreRule>) {
    let Ok(text) = fs::read_to_string(path) else { return (false, Vec::new()) };
    let rules = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| IgnoreRule { line: i + 1, text: l.to_string(), comment: l.trim_start().starts_with('#') })
        .collect();
    (true, rules)
}

/// Parse `source:line:pattern<TAB>path` from `git check-ignore -v`.
fn parse_verbose(line: &str) -> Option<WhyIgnored> {
    let (left, _) = line.split_once('\t')?;
    // the source may contain ':' (Windows drive letters), so find ":<digits>:"
    let bytes = left.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b':' {
            continue;
        }
        let rest = &left[i + 1..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(':') {
            let pattern = rest[digits + 1..].to_string();
            return Some(WhyIgnored {
                source: left[..i].to_string(),
                line: rest[..digits].parse().ok()?,
                negated: pattern.starts_with('!'),
                pattern,
            });
        }
    }
    None
}

impl Repo {
    /// Every ignore file that applies to this repository, most specific last.
    pub fn ignore_sources(&self) -> Result<Vec<IgnoreSource>> {
        let mut out = Vec::new();
        let mut add = |label: String, rel: Option<String>, abs: PathBuf| {
            let (exists, rules) = read_rules(&abs);
            out.push(IgnoreSource { label, rel, abs, exists, rules });
        };

        let global = self
            .git
            .run_text(&["config", "--path", "--get", "core.excludesFile"])
            .ok()
            .map(|p| PathBuf::from(p.trim()))
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                let base = std::env::var_os("XDG_CONFIG_HOME")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
                Some(base.join("git").join("ignore"))
            });
        if let Some(g) = global {
            if g.exists() {
                add(format!("global · {}", g.display()), None, g);
            }
        }
        let exclude = self.git_dir().join("info").join("exclude");
        let rel = exclude.strip_prefix(self.root()).ok().map(|p| p.to_string_lossy().replace('\\', "/"));
        add(".git/info/exclude (only on this machine)".into(), rel, exclude);
        add(".gitignore".into(), Some(".gitignore".into()), self.root().join(".gitignore"));

        let listed = self.git.run(&["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?;
        let mut nested: Vec<String> = String::from_utf8_lossy(&listed)
            .split('\0')
            .filter(|p| p.ends_with("/.gitignore"))
            .map(str::to_string)
            .collect();
        nested.sort();
        nested.dedup();
        for rel in nested {
            let abs = self.root().join(&rel);
            add(rel.clone(), Some(rel), abs);
        }
        Ok(out)
    }

    /// Which rule decides whether `path` is ignored, if any.
    pub fn why_ignored(&self, path: &str) -> Result<Option<WhyIgnored>> {
        match self.git.run_text(&["check-ignore", "-v", "--no-index", "--", path]) {
            Ok(out) => Ok(out.lines().next().and_then(parse_verbose)),
            Err(Error::Command { stderr, .. }) if stderr.is_empty() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Append `pattern` to an ignore file inside the repo and stop tracking `untrack`.
    pub fn add_ignore_rule(&self, rel: &str, pattern: &str, untrack: &[String]) -> Result<String> {
        self.record(OpKind::Edit, format!("Ignore {pattern}"), |r, backups| {
            r.backup_path(rel, backups)?;
            let path = r.root().join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut text = fs::read_to_string(&path).unwrap_or_default();
            if !text.lines().any(|l| l.trim() == pattern) {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(pattern);
                text.push('\n');
                fs::write(&path, text)?;
            }
            let mut commands = vec![format!("echo '{pattern}' >> {rel}")];
            if !untrack.is_empty() {
                let mut args = vec!["rm", "-r", "--cached", "-q", "--ignore-unmatch", "--"];
                args.extend(untrack.iter().map(String::as_str));
                r.git.run(&args)?;
                commands.push(crate::git::display_command(&args));
            }
            Ok(commands.join(" && "))
        })
    }

    /// Remove one line from an ignore file. `expected` guards against the file
    /// having changed since it was read.
    pub fn remove_ignore_rule(&self, rel: &str, line: usize, expected: &str) -> Result<String> {
        self.record(OpKind::Edit, format!("Remove ignore rule {expected}"), |r, backups| {
            let path = r.root().join(rel);
            let text = fs::read_to_string(&path)?;
            let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
            let current = lines.get(line.wrapping_sub(1)).map(|l| l.trim_end_matches(['\n', '\r']));
            if current != Some(expected) {
                return Err(Error::Msg(format!("{rel} changed since it was read — reopen the rules and try again.")));
            }
            r.backup_path(rel, backups)?;
            lines.remove(line - 1);
            fs::write(&path, lines.concat())?;
            Ok(format!("remove line {line} from {rel}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbose_output() {
        let w = parse_verbose(".gitignore:3:/Library/\tLibrary/x").unwrap();
        assert_eq!((w.source.as_str(), w.line, w.pattern.as_str(), w.negated), (".gitignore", 3, "/Library/", false));
        let w = parse_verbose("C:/Users/me/.config/git/ignore:12:!keep.txt\tkeep.txt").unwrap();
        assert_eq!(w.source, "C:/Users/me/.config/git/ignore");
        assert!(w.negated);
        let w = parse_verbose("a/.gitignore:1:*.tmp:x\tf").unwrap();
        assert_eq!(w.pattern, "*.tmp:x");
    }
}
