//! Pushing (with a preview of what leaves the machine) and the About screen.

use gitty_core::log::{Commit, LogQuery};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::{App, Popup};
use crate::worker::OpJob;

/// What `P` is about to send, shown before anything leaves the machine.
pub struct PushPreview {
    pub branch: String,
    pub remote: String,
    /// `None` when the branch isn't on the remote yet (first push publishes it).
    pub upstream: Option<String>,
    pub commits: Vec<Commit>,
    /// Commits beyond the ones listed.
    pub more: usize,
    /// The remote has commits this branch doesn't; git will refuse the push.
    pub behind: u32,
}

const PREVIEW_ROWS: usize = 10;

impl App {
    pub(super) fn push(&mut self) {
        let b = self.status.branch.clone();
        let Some(head) = b.head.clone() else {
            self.notice_err("HEAD is detached — switch to a branch before pushing.");
            return;
        };
        if b.oid.is_none() {
            self.notice_err("Nothing to push yet — make a commit first.");
            return;
        }
        let remote = match &b.upstream {
            Some(up) => up.split_once('/').map(|(r, _)| r.to_string()).unwrap_or_else(|| up.clone()),
            None => match self.remotes.iter().find(|r| *r == "origin").or(self.remotes.first()) {
                Some(r) => r.clone(),
                None => {
                    self.notice_err("No remote configured. Add one with `:remote add origin <url>`.");
                    return;
                }
            },
        };
        if b.upstream.is_some() && b.ahead == 0 {
            let up = b.upstream.clone().unwrap_or_default();
            if b.behind > 0 {
                let pull = self.keys.key(crate::keys::Action::Pull);
                self.notice(format!("Nothing to push — {up} is {} ahead of you. {pull} to pull.", plural(b.behind, "commit")));
            } else {
                self.notice(format!("Nothing to push — {head} is up to date with {up}."));
            }
            return;
        }
        let set_upstream = b.upstream.is_none().then(|| (remote.clone(), head.clone()));
        if !self.confirm_push {
            self.send_job(OpJob::Push { set_upstream });
            return;
        }
        let revs: Vec<String> = match &b.upstream {
            Some(_) => vec!["@{u}..HEAD".into()],
            None => vec!["HEAD".into(), "--not".into(), format!("--remotes={remote}")],
        };
        let mut commits = self.repo.log(&LogQuery { revs, skip: 0, limit: 500 }).unwrap_or_default();
        let total = match b.upstream {
            Some(_) => (b.ahead as usize).max(commits.len()),
            None => commits.len(),
        };
        commits.truncate(PREVIEW_ROWS);
        let more = total.saturating_sub(commits.len());
        self.popup = Some(Popup::Push(PushPreview {
            branch: head,
            remote,
            upstream: b.upstream.clone(),
            commits,
            more,
            behind: b.behind,
        }));
    }

    pub(super) fn on_push_key(&mut self, p: PushPreview, key: &KeyEvent) -> Option<Popup> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                let set_upstream = p.upstream.is_none().then(|| (p.remote.clone(), p.branch.clone()));
                self.send_job(OpJob::Push { set_upstream });
                None
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => None,
            _ => Some(Popup::Push(p)),
        }
    }

    /// Hints for the bottom bar: what's waiting to go up or come down.
    pub fn sync_hints(&self) -> Vec<(String, String)> {
        use crate::keys::Action;
        let b = &self.status.branch;
        let mut h = Vec::new();
        if b.head.is_none() || b.oid.is_none() || self.remotes.is_empty() {
            return h;
        }
        if b.upstream.is_none() {
            h.push((self.keys.key(Action::Push), "publish".into()));
            return h;
        }
        if b.ahead > 0 {
            h.push((self.keys.key(Action::Push), format!("push {}", b.ahead)));
        }
        if b.behind > 0 {
            h.push((self.keys.key(Action::Pull), format!("pull {}", b.behind)));
        }
        h
    }

    /// Appended to "Committed …" so the next step is obvious.
    pub(super) fn after_commit_hint(&self) -> Option<String> {
        if self.remotes.is_empty() || self.status.branch.head.is_none() {
            return None;
        }
        Some(format!("{} to push", self.keys.key(crate::keys::Action::Push)))
    }

    pub(super) fn on_about_key(&mut self, key: &KeyEvent) -> Option<Popup> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => None,
            _ => Some(Popup::About),
        }
    }

    /// Milliseconds since the app started, for the About animation.
    pub fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }
}

pub fn plural(n: u32, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}
