//! Editing commit messages and ignore rules.

use gitty_core::ignore::IgnoreSource;
use gitty_core::status::EntryKind;
use gitty_core::{Repo, RepoState};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::{App, CommitForm, Focus, Popup, Suspend};
use crate::util;
use crate::widgets::{LineInput, TextArea};
use crate::worker::OpJob;

pub struct IgnoreOption {
    pub pattern: String,
    pub label: String,
    /// Pathspecs to stop tracking when the file is already tracked.
    pub untrack: Vec<String>,
}

pub struct IgnoreMenu {
    pub path: String,
    pub tracked: bool,
    pub options: Vec<IgnoreOption>,
    pub sel: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RulesInput {
    Add,
    Check,
}

pub struct IgnoreRulesView {
    pub sources: Vec<IgnoreSource>,
    /// (source index, rule index) per row; `None` rule = the source header.
    pub rows: Vec<(usize, Option<usize>)>,
    pub sel: usize,
    pub input: Option<(RulesInput, LineInput)>,
    pub result: Option<(String, bool)>,
}

impl IgnoreRulesView {
    fn new(repo: &Repo) -> Result<Self, String> {
        let mut v = Self { sources: Vec::new(), rows: Vec::new(), sel: 0, input: None, result: None };
        v.load(repo)?;
        // start on the root .gitignore
        v.sel = v.rows.iter().position(|(s, _)| v.sources[*s].label == ".gitignore").unwrap_or(0);
        Ok(v)
    }

    fn load(&mut self, repo: &Repo) -> Result<(), String> {
        self.sources = repo.ignore_sources().map_err(|e| e.to_string())?;
        self.rows.clear();
        for (si, s) in self.sources.iter().enumerate() {
            self.rows.push((si, None));
            self.rows.extend((0..s.rules.len()).map(|ri| (si, Some(ri))));
        }
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
        Ok(())
    }

    pub fn reload(&mut self, repo: &Repo) {
        let _ = self.load(repo);
    }

    pub fn current_source(&self) -> Option<&IgnoreSource> {
        self.rows.get(self.sel).map(|(s, _)| &self.sources[*s])
    }
}

fn ignore_options(path: &str, tracked: bool) -> Vec<IgnoreOption> {
    let trimmed = path.trim_end_matches('/');
    let (dir, name) = match trimmed.rsplit_once('/') {
        Some((d, n)) => (Some(d), n),
        None => (None, trimmed),
    };
    let untrack = |spec: String| if tracked { vec![spec] } else { Vec::new() };
    let mut out = Vec::new();
    if path.ends_with('/') {
        out.push(IgnoreOption { pattern: format!("/{trimmed}/"), label: "this folder".into(), untrack: untrack(trimmed.into()) });
        out.push(IgnoreOption {
            pattern: format!("{name}/"),
            label: format!("every folder named {name}"),
            untrack: untrack(format!(":(glob)**/{name}")),
        });
        return out;
    }
    out.push(IgnoreOption { pattern: format!("/{trimmed}"), label: "only this file".into(), untrack: untrack(trimmed.into()) });
    if let Some((_, ext)) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) {
        out.push(IgnoreOption {
            pattern: format!("*.{ext}"),
            label: format!("every .{ext} file"),
            untrack: untrack(format!(":(glob)**/*.{ext}")),
        });
    }
    if let Some(d) = dir {
        out.push(IgnoreOption { pattern: format!("/{d}/"), label: format!("the whole {d}/ folder"), untrack: untrack(d.into()) });
    }
    out.push(IgnoreOption {
        pattern: name.to_string(),
        label: format!("every file named {name}"),
        untrack: untrack(format!(":(glob)**/{name}")),
    });
    out
}

impl App {
    // ------------------------------------------------------------ commit message

    pub(super) fn open_reword(&mut self) {
        let Some(c) = self.selected_commit().cloned() else { return };
        if self.state != RepoState::Clean {
            self.notice_err("Finish or abort the current merge/rebase first (m).");
            return;
        }
        if c.parents.len() > 1 {
            self.notice_err("Editing a merge commit's message isn't supported.");
            return;
        }
        if !self.repo.git().succeeds(&["merge-base", "--is-ancestor", &c.hash, "HEAD"]) {
            self.notice_err(format!("{} isn't on the current branch — switch to its branch first.", c.short()));
            return;
        }
        let msg = self.repo.git().run_text(&["log", "-1", "--format=%B", &c.hash]).unwrap_or_default();
        let msg = msg.trim_end();
        let (summary, body) = msg.split_once('\n').unwrap_or((msg, ""));
        let pushed = self.repo.is_pushed(&c.hash);
        self.popup = Some(Popup::Commit(CommitForm {
            summary: LineInput::with(summary),
            body: TextArea::with(body.trim_start_matches('\n')),
            field: 0,
            amend: false,
            error: None,
            reword: Some(c.hash),
            pushed,
        }));
    }

    // ------------------------------------------------------------ ignore a file

    pub(super) fn open_ignore_menu(&mut self) {
        let Some((_, e)) = self.selected_entry() else {
            self.notice("Select a file first.");
            return;
        };
        if e.is_conflict() {
            self.notice_err("Resolve the conflict before ignoring this file.");
            return;
        }
        let tracked = e.kind != EntryKind::Untracked;
        let options = ignore_options(&e.path, tracked);
        self.popup = Some(Popup::IgnoreMenu(IgnoreMenu { path: e.path.clone(), tracked, options, sel: 0 }));
    }

    pub(super) fn on_ignore_menu_key(&mut self, mut m: IgnoreMenu, key: &KeyEvent) -> Option<Popup> {
        let pick = |m: &IgnoreMenu, i: usize| m.options.get(i).map(|o| OpJob::AddIgnore {
            rel: ".gitignore".into(),
            pattern: o.pattern.clone(),
            untrack: o.untrack.clone(),
        });
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => None,
            KeyCode::Char('j') | KeyCode::Down => {
                m.sel = (m.sel + 1).min(m.options.len() - 1);
                Some(Popup::IgnoreMenu(m))
            }
            KeyCode::Char('k') | KeyCode::Up => {
                m.sel = m.sel.saturating_sub(1);
                Some(Popup::IgnoreMenu(m))
            }
            KeyCode::Char(c @ '1'..='9') => {
                if let Some(job) = pick(&m, c as usize - '1' as usize) {
                    self.send_job(job);
                    return None;
                }
                Some(Popup::IgnoreMenu(m))
            }
            KeyCode::Enter => {
                if let Some(job) = pick(&m, m.sel) {
                    self.send_job(job);
                }
                None
            }
            _ => Some(Popup::IgnoreMenu(m)),
        }
    }

    // ------------------------------------------------------------ ignore rules

    pub(super) fn open_ignore_rules(&mut self) {
        match IgnoreRulesView::new(&self.repo) {
            Ok(mut v) => {
                if self.focus == Focus::Files {
                    if let Some((_, e)) = self.selected_entry() {
                        let path = e.path.clone();
                        v.result = Some(self.why_text(&path));
                    }
                }
                self.popup = Some(Popup::IgnoreRules(v));
            }
            Err(e) => self.notice_err(e),
        }
    }

    fn why_text(&self, path: &str) -> (String, bool) {
        match self.repo.why_ignored(path) {
            Ok(Some(w)) if w.negated => {
                (format!("{} is NOT ignored — rule {} on line {} of {} re-includes it.", util::clean(path), w.pattern, w.line, w.source), false)
            }
            Ok(Some(w)) => (format!("{} is ignored by `{}` (line {} of {}).", util::clean(path), w.pattern, w.line, w.source), true),
            Ok(None) => (format!("{} is not ignored by any rule.", util::clean(path)), false),
            Err(e) => (e.to_string(), false),
        }
    }

    pub(super) fn on_ignore_rules_key(&mut self, mut v: IgnoreRulesView, key: &KeyEvent) -> Option<Popup> {
        if let Some((mode, mut input)) = v.input.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let text = input.text.trim().to_string();
                    if !text.is_empty() {
                        match mode {
                            RulesInput::Check => v.result = Some(self.why_text(&text)),
                            RulesInput::Add => {
                                let rel = v.current_source().and_then(|s| s.rel.clone()).unwrap_or_else(|| ".gitignore".into());
                                self.send_job(OpJob::AddIgnore { rel, pattern: text, untrack: Vec::new() });
                            }
                        }
                    }
                }
                _ => {
                    input.handle(key);
                    v.input = Some((mode, input));
                }
            }
            return Some(Popup::IgnoreRules(v));
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return None,
            KeyCode::Char('j') | KeyCode::Down => v.sel = (v.sel + 1).min(v.rows.len().saturating_sub(1)),
            KeyCode::Char('k') | KeyCode::Up => v.sel = v.sel.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => v.sel = 0,
            KeyCode::Char('G') | KeyCode::End => v.sel = v.rows.len().saturating_sub(1),
            KeyCode::Char('/') => {
                v.input = Some((RulesInput::Check, LineInput::default()));
                v.result = None;
            }
            KeyCode::Char('a') => {
                if v.current_source().is_some_and(|s| s.rel.is_none()) {
                    v.result = Some(("That file is outside this repository — press e to edit it.".into(), false));
                } else {
                    v.input = Some((RulesInput::Add, LineInput::default()));
                    v.result = None;
                }
            }
            KeyCode::Char('d') => match v.rows.get(v.sel) {
                Some((si, Some(ri))) => {
                    let src = &v.sources[*si];
                    match &src.rel {
                        Some(rel) => {
                            let rule = &src.rules[*ri];
                            self.send_job(OpJob::RemoveIgnore { rel: rel.clone(), line: rule.line, text: rule.text.clone() });
                        }
                        None => v.result = Some(("That file is outside this repository — press e to edit it.".into(), false)),
                    }
                }
                _ => v.result = Some(("Select a rule to delete.".into(), false)),
            },
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(src) = v.current_source() {
                    let (file, record) = match &src.rel {
                        Some(rel) => (rel.clone(), Some((gitty_core::oplog::OpKind::Edit, format!("Edit {rel}")))),
                        None => (src.abs.display().to_string(), None),
                    };
                    self.suspend = Some(Suspend { args: Vec::new(), record, edit: Some(file) });
                    return None;
                }
            }
            _ => {}
        }
        Some(Popup::IgnoreRules(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_for_a_file() {
        let o = ignore_options("Assets/Art/cliff.psd", false);
        let p: Vec<&str> = o.iter().map(|x| x.pattern.as_str()).collect();
        assert_eq!(p, ["/Assets/Art/cliff.psd", "*.psd", "/Assets/Art/", "cliff.psd"]);
        assert!(o.iter().all(|x| x.untrack.is_empty()));
        let o = ignore_options(".env", true);
        assert_eq!(o[0].untrack, vec![".env".to_string()]);
        assert!(!o.iter().any(|x| x.pattern == "*.env"), "dotfiles have no extension");
        let o = ignore_options("Builds/", false);
        assert_eq!(o[0].pattern, "/Builds/");
    }
}
