//! Conflicts, Insight, Repositories, interactive rebase and stashes.

use std::path::PathBuf;

use gitty_core::conflict::{self, ConflictFile, Pick};
use gitty_core::insight::{Fix, Health, Severity, SizeNode, human_size};
use gitty_core::oplog::OpKind;
use gitty_core::rebase::{self, RebaseAction, RebaseStep};
use gitty_core::stash::Stash;
use gitty_core::status::StatusEntry;
use gitty_core::{RepoState, unity};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, BranchRow, Focus, InputPurpose, Popup, Section, Suspend, View};
use crate::util;
use crate::widgets::LineInput;
use crate::worker::{self, OpJob};

#[derive(Default)]
pub struct ConflictView {
    pub path: String,
    pub file: Option<ConflictFile>,
    pub region: usize,
    pub note: Option<String>,
    /// Index / worktree status letters of the conflicted entry.
    pub xy: (char, char),
}

impl ConflictView {
    pub fn remaining(&self) -> usize {
        self.file.as_ref().map(|f| f.regions.len()).unwrap_or(0)
    }
}

pub struct RebaseForm {
    pub base: Option<String>,
    /// Oldest first, as git applies them.
    pub steps: Vec<RebaseStep>,
    /// Selected row in display order (newest first).
    pub sel: usize,
    pub error: Option<String>,
}

impl RebaseForm {
    pub fn step_index(&self, display: usize) -> usize {
        self.steps.len() - 1 - display
    }
}

impl App {
    // ------------------------------------------------------------ unity

    /// Structural summary of a Unity scene/prefab diff; `old`/`new` are revs
    /// for `read_version` (None = worktree, Some("") = index).
    pub(super) fn unity_summary(&self, path: &str, old_path: &str, old: Option<&str>, new: Option<&str>, d: &gitty_core::diff::FileDiff) -> Vec<String> {
        if !unity::is_unity_yaml(path) || d.hunks.is_empty() {
            return Vec::new();
        }
        let old_text = self.repo.read_version(old, old_path);
        let new_text = self.repo.read_version(new, path);
        unity::summarize(&old_text, &new_text, d)
    }

    // ------------------------------------------------------------ conflicts

    pub(super) fn load_conflict(&mut self, e: &StatusEntry) {
        let same = self.conflict.path == e.path;
        let region = if same { self.conflict.region } else { 0 };
        let mut cv = ConflictView { path: e.path.clone(), xy: (e.index, e.worktree), ..Default::default() };
        if e.index == 'D' || e.worktree == 'D' {
            let who = if e.index == 'D' { "us (HEAD)" } else { "them" };
            cv.note = Some(format!(
                "This file was deleted by {who} and changed on the other side.\n\nO  keep our version{}\nT  keep their version{}",
                if e.index == 'D' { " (delete it)" } else { "" },
                if e.worktree == 'D' { " (delete it)" } else { "" }
            ));
        } else {
            match self.repo.read_conflict(&e.path) {
                Ok(cf) => {
                    if cf.regions.is_empty() {
                        cv.note = Some("No conflict markers left. Press space to mark the file as resolved.".into());
                    }
                    cv.region = region.min(cf.regions.len().saturating_sub(1));
                    cv.file = Some(cf);
                }
                Err(err) => cv.note = Some(format!("{err}\n\nO  keep our version\nT  keep their version")),
            }
        }
        self.conflict = cv;
    }

    pub(super) fn conflict_region_nav(&mut self, forward: bool) {
        let n = self.conflict.remaining();
        if n == 0 {
            return;
        }
        self.conflict.region = if forward { (self.conflict.region + 1).min(n - 1) } else { self.conflict.region.saturating_sub(1) };
    }

    pub(super) fn resolve_region(&mut self, pick: Pick) {
        let cv = &self.conflict;
        let Some(cf) = &cv.file else {
            self.notice("Use O or T to pick a whole version of this file.");
            return;
        };
        let Some(r) = cf.regions.get(cv.region) else {
            self.notice("No conflicts left in this file — press space to mark it resolved.");
            return;
        };
        if pick == Pick::Base && r.base.is_none() {
            self.notice("The original isn't shown yet — press B to show it between the sides.");
            return;
        }
        let label = match pick {
            Pick::Ours => "ours",
            Pick::Theirs => "theirs",
            Pick::Both => "both",
            Pick::Base => "the original",
        };
        let n = cv.region + 1;
        if let Some(text) = conflict::resolve(cf, cv.region, pick) {
            let job = OpJob::Resolve { path: cv.path.clone(), text, what: format!("Kept {label} for conflict {n}") };
            self.send_job(job);
        }
    }

    pub(super) fn take_whole_file(&mut self, ours: bool) {
        let (x, y) = self.conflict.xy;
        let path = self.conflict.path.clone();
        if path.is_empty() {
            return;
        }
        let deletes = (ours && x == 'D') || (!ours && y == 'D');
        self.send_job(if deletes { OpJob::ResolveDelete(path) } else { OpJob::TakeSide { path, ours } });
    }

    pub(super) fn show_base(&mut self) {
        if self.conflict.file.is_none() {
            return;
        }
        let path = self.conflict.path.clone();
        self.popup = Some(Popup::Confirm {
            title: "Show the original".into(),
            message: format!(
                "Re-create the conflict markers in {} with the original version between the sides?\nManual edits to this file are replaced — u undoes it.",
                util::clean(&path)
            ),
            job: OpJob::ShowBase(path),
        });
    }

    pub(super) fn mark_resolved(&mut self) {
        let path = self.conflict.path.clone();
        if path.is_empty() {
            return;
        }
        let left = self.conflict.remaining();
        if left > 0 {
            self.popup = Some(Popup::Confirm {
                title: "Still conflicted".into(),
                message: format!("{} still has {left} conflict marker block(s).\nMark it resolved anyway?", util::clean(&path)),
                job: OpJob::Stage(vec![path]),
            });
        } else {
            self.send_job(OpJob::Stage(vec![path]));
        }
    }

    pub(super) fn edit_file(&mut self) {
        let path = match self.focus {
            Focus::Conflict => self.conflict.path.clone(),
            _ => self.selected_entry().map(|(_, e)| e.path.clone()).unwrap_or_default(),
        };
        if path.is_empty() || path.ends_with('/') {
            return;
        }
        self.suspend = Some(Suspend { args: Vec::new(), record: Some((OpKind::Edit, format!("Edit {path}"))), edit: Some(path) });
    }

    // ------------------------------------------------------------ operation menu

    pub(super) fn on_opmenu_key(&mut self, key: &KeyEvent) -> Option<Popup> {
        match key.code {
            KeyCode::Char('c') => {
                self.send_job(OpJob::Continue);
                None
            }
            KeyCode::Char('a') => {
                let what = self.state.label().unwrap_or("operation").to_lowercase();
                Some(Popup::Confirm {
                    title: "Abort".into(),
                    message: format!("Abort the {what} and go back to where you started?\nResolutions made so far are lost."),
                    job: OpJob::Abort,
                })
            }
            KeyCode::Char('s') if self.state == RepoState::Rebasing => {
                self.send_job(OpJob::Skip);
                None
            }
            KeyCode::Esc | KeyCode::Char('q') => None,
            _ => Some(Popup::OpMenu),
        }
    }

    // ------------------------------------------------------------ stashes

    pub fn selected_stash(&self) -> Option<&Stash> {
        match self.branch_rows.get(self.branch_sel)? {
            BranchRow::Stash(i) => self.stashes.get(*i),
            _ => None,
        }
    }

    pub(super) fn open_stash_input(&mut self) {
        if self.status.entries.is_empty() {
            self.notice("Nothing to stash — the working tree is clean.");
            return;
        }
        let untracked = self.status.entries.iter().any(|e| e.is_untracked());
        self.popup = Some(Popup::Input {
            title: String::new(),
            input: LineInput::default(),
            purpose: InputPurpose::Stash { untracked },
        });
    }

    pub(super) fn load_stash_files(&mut self) {
        let Some(st) = self.selected_stash().cloned() else { return };
        if self.stash_files_for.as_deref() == Some(st.name.as_str()) {
            return;
        }
        self.stash_files = self.repo.stash_files(&st.oid).unwrap_or_default();
        self.stash_files_for = Some(st.name);
    }

    pub(super) fn drop_stash(&mut self) {
        let Some(st) = self.selected_stash().cloned() else { return };
        self.popup = Some(Popup::Confirm {
            title: "Drop stash".into(),
            message: format!("Drop {} \"{}\"?\nPress u afterwards to undo.", st.name, util::clean(&st.message)),
            job: OpJob::StashDrop(st),
        });
    }

    // ------------------------------------------------------------ rebase

    pub(super) fn open_rebase(&mut self) {
        let Some(c) = self.selected_commit().cloned() else { return };
        if self.state != RepoState::Clean {
            self.notice_err("Finish or abort the current merge/rebase first (m).");
            return;
        }
        if self.status.branch.head.is_none() {
            self.notice_err("HEAD is detached — switch to a branch before rebasing.");
            return;
        }
        if !self.repo.git().succeeds(&["merge-base", "--is-ancestor", &c.hash, "HEAD"]) {
            self.notice_err(format!("{} isn't on the current branch.", c.short()));
            return;
        }
        let base = c.parents.first().cloned();
        match self.repo.rebase_plan(base.as_deref()) {
            Ok(steps) => self.popup = Some(Popup::Rebase(RebaseForm { base, steps, sel: 0, error: None })),
            Err(e) => self.notice_err(e.to_string()),
        }
    }

    pub(super) fn on_rebase_key(&mut self, mut f: RebaseForm, key: &KeyEvent) -> Option<Popup> {
        let n = f.steps.len();
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        f.error = None;
        let set = |f: &mut RebaseForm, a: RebaseAction| {
            let i = f.step_index(f.sel);
            f.steps[i].action = a;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return None,
            KeyCode::Up if shift => move_step(&mut f, true),
            KeyCode::Down if shift => move_step(&mut f, false),
            KeyCode::Char('K') => move_step(&mut f, true),
            KeyCode::Char('J') => move_step(&mut f, false),
            KeyCode::Char('k') | KeyCode::Up => f.sel = f.sel.saturating_sub(1),
            KeyCode::Char('j') | KeyCode::Down => f.sel = (f.sel + 1).min(n - 1),
            KeyCode::Char('p') => set(&mut f, RebaseAction::Pick),
            KeyCode::Char('s') => set(&mut f, RebaseAction::Squash),
            KeyCode::Char('f') => set(&mut f, RebaseAction::Fixup),
            KeyCode::Char('d') => set(&mut f, RebaseAction::Drop),
            KeyCode::Char('r') => {
                let idx = f.step_index(f.sel);
                let current = f.steps[idx].message.clone().unwrap_or_else(|| f.steps[idx].subject.clone());
                return Some(Popup::Input {
                    title: format!("New message for {}", &f.steps[idx].hash[..7]),
                    input: LineInput::with(&current),
                    purpose: InputPurpose::RebaseReword { form: Box::new(f), idx },
                });
            }
            KeyCode::Enter => {
                if let Err(e) = rebase::validate(&f.steps) {
                    f.error = Some(e.to_string());
                    return Some(Popup::Rebase(f));
                }
                let changed = f.steps.iter().any(|s| s.action != RebaseAction::Pick) || !self.rebase_in_original_order(&f);
                if !changed {
                    f.error = Some("Nothing changed — set an action (p r s f d) or reorder with J/K.".into());
                    return Some(Popup::Rebase(f));
                }
                let result = rebase::preview(&f.steps);
                let pushed = f.steps.iter().filter(|s| s.pushed).count();
                let mut msg = format!("The branch will end with {} commit(s) here (newest first):\n", result.len());
                for s in result.iter().rev().take(12) {
                    msg.push_str(&format!("  • {}\n", util::clean(s)));
                }
                if result.len() > 12 {
                    msg.push_str(&format!("  … {} more\n", result.len() - 12));
                }
                if pushed > 0 {
                    msg.push_str(&format!(
                        "\n⚠ {pushed} of these commits are already on a remote. You'll need to force-push afterwards (`:push --force-with-lease`).\n"
                    ));
                }
                msg.push_str("\nUncommitted changes are stashed and restored automatically. Press u afterwards to undo.");
                return Some(Popup::Confirm { title: "Rebase".into(), message: msg, job: OpJob::Rebase { base: f.base, steps: f.steps } });
            }
            _ => {}
        }
        Some(Popup::Rebase(f))
    }

    fn rebase_in_original_order(&self, f: &RebaseForm) -> bool {
        // rebase_plan returns oldest first; the original order follows history
        let pos: Vec<Option<usize>> = f.steps.iter().map(|s| self.commits.iter().position(|c| c.hash == s.hash)).collect();
        pos.windows(2).all(|w| match (w[0], w[1]) {
            (Some(a), Some(b)) => a > b,
            _ => true,
        })
    }

    // ------------------------------------------------------------ insight

    pub(super) fn ensure_insight(&mut self, force: bool) {
        if self.insight_loading || self.blobs_loading {
            return;
        }
        if self.insight.is_some() && !force && !self.insight_stale {
            return;
        }
        self.insight_loading = true;
        self.blobs_loading = true;
        self.insight_stale = false;
        worker::spawn_insight(self.repo.clone(), self.sender(), !force);
    }

    /// Health checks plus the history check that needs the blob scan.
    pub fn health_items(&self) -> Vec<Health> {
        let Some(ins) = &self.insight else { return Vec::new() };
        let mut items = ins.health.clone();
        if let Some(blobs) = &self.blobs {
            let heavy: Vec<_> = blobs.iter().filter(|b| !b.lfs && b.size >= 1024 * 1024).collect();
            if !heavy.is_empty() && ins.lfs_installed {
                let total: u64 = heavy.iter().map(|b| b.size).sum();
                let mut patterns: Vec<String> = heavy
                    .iter()
                    .filter_map(|b| b.path.rsplit_once('.').map(|(_, e)| format!("*.{}", e.to_ascii_lowercase())))
                    .collect();
                patterns.sort();
                patterns.dedup();
                let list: Vec<String> = heavy
                    .iter()
                    .take(5)
                    .map(|b| {
                        let c = b.commit.as_deref().map(|c| &c[..7]).unwrap_or("?");
                        format!("{}  {}  ({c})", human_size(b.size), b.path)
                    })
                    .collect();
                items.push(Health {
                    severity: Severity::Info,
                    title: format!("{} large files in history are not in LFS ({})", heavy.len(), human_size(total)),
                    detail: format!(
                        "Every clone downloads them, even if they were deleted since.\nMoving them to LFS rewrites history for everyone; coordinate with your team first.\n\n{}",
                        list.join("\n")
                    ),
                    fix: Some(Fix::LfsMigrate { patterns }),
                });
            }
        }
        items.sort_by_key(|h| h.severity);
        items
    }

    pub(super) fn apply_health_fix(&mut self) {
        let items = self.health_items();
        let Some(h) = items.get(self.health_sel) else { return };
        match &h.fix {
            None => self.notice("Nothing gitty can fix automatically here — see the details."),
            Some(Fix::Ignore { pattern, untrack }) => {
                let untrack_note = if untrack.is_empty() {
                    String::new()
                } else {
                    format!("\nStop tracking {} (files stay on disk; teammates get them deleted on pull).", untrack.join(", "))
                };
                self.popup = Some(Popup::Confirm {
                    title: "Fix".into(),
                    message: format!("Add `{pattern}` to .gitignore?{untrack_note}\nThe change is staged for you to commit. u undoes it."),
                    job: OpJob::ApplyIgnore { pattern: pattern.clone(), untrack: untrack.clone() },
                });
            }
            Some(Fix::LfsTrack { patterns, convert }) => {
                self.popup = Some(Popup::Confirm {
                    title: "Move to LFS".into(),
                    message: format!(
                        "Track {} with Git LFS and convert {} current file(s)?\n.gitattributes and the converted files are staged; commit them to finish.\nOlder versions stay in history. u undoes it.",
                        patterns.join(" "),
                        convert.len()
                    ),
                    job: OpJob::LfsTrack { patterns: patterns.clone(), convert: convert.clone() },
                });
            }
            Some(Fix::LfsMigrate { patterns }) => {
                let include = patterns.join(",");
                self.popup = Some(Popup::ConfirmRun {
                    title: "Rewrite history".into(),
                    message: format!(
                        "Run `git lfs migrate import --everything --include=\"{include}\"`?\n\n⚠ This rewrites every branch. Everyone must re-clone or reset afterwards, and you must force-push.\nIt runs in the terminal and cannot be undone with u (git keeps the old commits in the reflog)."
                    ),
                    args: vec!["lfs".into(), "migrate".into(), "import".into(), "--everything".into(), format!("--include={include}")],
                });
            }
        }
    }

    pub fn size_node(&self) -> Option<&SizeNode> {
        let ins = self.insight.as_ref()?;
        ins.tree.find(&self.size_path).or(Some(&ins.tree))
    }

    pub(super) fn size_open(&mut self) {
        let Some(node) = self.size_node() else { return };
        let Some(child) = node.children.get(self.size_sel) else { return };
        if child.is_dir() && !child.children.is_empty() {
            self.size_path = child.path.clone();
            self.size_sel = 0;
            self.size_off = 0;
        }
    }

    pub(super) fn size_up(&mut self) {
        if self.size_path.is_empty() {
            return;
        }
        let old = self.size_path.clone();
        let trimmed = old.trim_end_matches('/');
        self.size_path = match trimmed.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/"),
            None => String::new(),
        };
        self.size_sel = self.size_node().and_then(|n| n.children.iter().position(|c| c.path == old)).unwrap_or(0);
    }

    pub(super) fn blob_goto_commit(&mut self) {
        let Some(hash) = self.blobs.as_ref().and_then(|b| b.get(self.blob_sel)).and_then(|b| b.commit.clone()) else {
            self.notice("No commit found for this file.");
            return;
        };
        match self.commits.iter().position(|c| c.hash == hash) {
            Some(i) => {
                self.set_view(View::History);
                self.commit_sel = i;
                self.detail_due = Some(std::time::Instant::now());
            }
            None => {
                util::copy_to_clipboard(&hash);
                self.notice(format!("{} isn't loaded in History yet — hash copied.", &hash[..7]));
            }
        }
    }

    // ------------------------------------------------------------ repositories

    pub(super) fn ensure_repos(&mut self, force: bool) {
        if self.repos_loading || (!self.repos.is_empty() && !force) {
            return;
        }
        self.repos_loading = true;
        worker::spawn_repos(self.workspaces.clone(), self.sender());
    }

    pub(super) fn open_selected_repo(&mut self) {
        let Some(r) = self.repos.get(self.repo_sel) else { return };
        let same = r.path.canonicalize().ok() == self.repo.root().canonicalize().ok();
        if same {
            self.notice(format!("{} is already open.", r.name));
        } else {
            self.switch_to = Some(r.path.clone());
        }
    }

    pub fn take_switch(&mut self) -> Option<PathBuf> {
        self.switch_to.take()
    }

    pub(super) fn after_op_insight(&mut self) {
        self.insight_stale = true;
        if self.view == View::Insight {
            self.ensure_insight(false);
        }
    }

    pub fn conflict_focus_target(&self) -> Focus {
        match self.selected_entry() {
            Some((Section::Conflicts, _)) => Focus::Conflict,
            _ => Focus::Diff,
        }
    }
}

fn move_step(f: &mut RebaseForm, up: bool) {
    let n = f.steps.len();
    if up && f.sel > 0 {
        let (a, b) = (f.step_index(f.sel), f.step_index(f.sel - 1));
        f.steps.swap(a, b);
        f.sel -= 1;
    } else if !up && f.sel + 1 < n {
        let (a, b) = (f.step_index(f.sel), f.step_index(f.sel + 1));
        f.steps.swap(a, b);
        f.sel += 1;
    }
}
