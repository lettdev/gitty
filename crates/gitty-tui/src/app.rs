//! Application state and input handling.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use gitty_core::branch::Branch;
use gitty_core::diff::{FileDiff, build_patch};
use gitty_core::graph::{Cell, GraphBuilder};
use gitty_core::log::{Commit, CommitDetail};
use gitty_core::oplog::OpKind;
use gitty_core::ops::DiscardTarget;
use gitty_core::status::{EntryKind, Status, StatusEntry};
use gitty_core::{Repo, RepoState};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::config::Config;
use crate::keys::{self, Action, Chord, Keymap};
use crate::theme::{self, Symbols, Theme};
use crate::util;
use crate::widgets::{LineInput, TextArea};
use crate::worker::{self, Msg, OpJob, OpOutcome, RefreshData};

pub const LOG_PAGE: usize = 400;
const DETAIL_DELAY: Duration = Duration::from_millis(80);
const FS_DEBOUNCE: Duration = Duration::from_millis(300);
const NOTICE_TIME: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Changes,
    History,
    Branches,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Files,
    Diff,
    Commits,
    CommitFiles,
    CommitDiff,
    Branches,
}

impl Focus {
    fn actions(self) -> &'static [Action] {
        match self {
            Focus::Files => keys::FILES,
            Focus::Diff => keys::DIFF,
            Focus::Commits => keys::COMMITS,
            Focus::CommitFiles => keys::COMMIT_FILES,
            Focus::CommitDiff => keys::COMMIT_DIFF,
            Focus::Branches => keys::BRANCHES,
        }
    }

    fn view(self) -> View {
        match self {
            Focus::Files | Focus::Diff => View::Changes,
            Focus::Commits | Focus::CommitFiles | Focus::CommitDiff => View::History,
            Focus::Branches => View::Branches,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Conflicts,
    Unstaged,
    Staged,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Conflicts => "Conflicts",
            Section::Unstaged => "Unstaged",
            Section::Staged => "Staged",
        }
    }

    fn contains(self, e: &StatusEntry) -> bool {
        match self {
            Section::Conflicts => e.is_conflict(),
            Section::Unstaged => e.is_unstaged() && !e.is_conflict(),
            Section::Staged => e.is_staged() && !e.is_conflict(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum FileRow {
    Header(Section, usize),
    File(Section, usize),
}

#[derive(Clone, Copy, Debug)]
pub enum BranchRow {
    Header(&'static str, usize),
    Branch(usize),
}

/// A scrollable diff with a line cursor and optional range selection.
#[derive(Default)]
pub struct DiffView {
    pub path: String,
    pub section: Option<Section>,
    pub diff: Option<FileDiff>,
    /// Plain text instead of a parsed diff (conflicts).
    pub raw: Option<Vec<String>>,
    pub note: Option<String>,
    /// (hunk, line) per visible row; `None` line = hunk header.
    pub flat: Vec<(usize, Option<usize>)>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub scroll: usize,
    pub patchable: bool,
}

impl DiffView {
    fn set(&mut self, d: FileDiff, patchable: bool) {
        self.flat.clear();
        for (hi, h) in d.hunks.iter().enumerate() {
            self.flat.push((hi, None));
            self.flat.extend((0..h.lines.len()).map(|li| (hi, Some(li))));
        }
        if d.binary {
            self.note = Some("Binary file — no text diff.".into());
        } else if d.hunks.is_empty() {
            self.note = Some("No text changes (mode or permission change only).".into());
        }
        self.patchable = patchable && !d.binary && !d.hunks.is_empty();
        self.diff = Some(d);
        self.cursor = if self.flat.len() > 1 { 1 } else { 0 };
    }

    pub fn len(&self) -> usize {
        self.raw.as_ref().map(Vec::len).unwrap_or(self.flat.len())
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.len();
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
    }

    fn jump_hunk(&mut self, forward: bool) {
        let cur = self.flat.get(self.cursor).map(|f| f.0).unwrap_or(0);
        let target = if forward { self.flat.iter().position(|f| f.0 > cur) } else {
            let prev = cur.checked_sub(1);
            prev.and_then(|p| self.flat.iter().position(|f| f.0 == p))
        };
        if let Some(t) = target {
            self.cursor = (t + 1).min(self.flat.len() - 1);
        }
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.anchor.map(|a| (a.min(self.cursor), a.max(self.cursor)))
    }

    /// Lines to act on: the selected range, or the hunk under the cursor.
    fn selected(&self) -> HashSet<(usize, usize)> {
        match self.selection_range() {
            Some((a, b)) => self.flat[a..=b].iter().filter_map(|&(h, l)| l.map(|l| (h, l))).collect(),
            None => {
                let Some(&(h, _)) = self.flat.get(self.cursor) else { return HashSet::new() };
                self.flat.iter().filter(|f| f.0 == h).filter_map(|&(h, l)| l.map(|l| (h, l))).collect()
            }
        }
    }
}

#[derive(Clone)]
pub enum InputPurpose {
    NewBranch { start: Option<String> },
    RenameBranch { old: String },
    GitCommand,
}

pub struct CommitForm {
    pub summary: LineInput,
    pub body: TextArea,
    /// 0 = summary, 1 = description
    pub field: u8,
    pub amend: bool,
    pub error: Option<String>,
}

pub enum Popup {
    Help { scroll: u16 },
    Confirm { title: String, message: String, job: OpJob },
    Input { title: String, input: LineInput, purpose: InputPurpose },
    Commit(CommitForm),
    Message { title: String, text: String, error: bool, scroll: u16, retry: Option<Vec<String>> },
}

pub struct Suspend {
    pub args: Vec<String>,
    /// Record the command in the operation log so it can be undone.
    pub record: Option<(OpKind, String)>,
}

#[derive(Clone, Copy, Debug)]
pub enum Hit {
    Tab(View),
    /// A list pane: rows start at the rect's top, `offset` is the first row index.
    List(Focus, usize),
}

pub struct Notice {
    pub text: String,
    pub error: bool,
    at: Instant,
}

pub struct App {
    pub repo: Repo,
    pub theme: Theme,
    pub sym: Symbols,
    pub keys: Keymap,
    pub mouse: bool,

    pub view: View,
    pub focus: Focus,

    pub status: Status,
    pub branches: Vec<Branch>,
    pub remotes: Vec<String>,
    pub state: RepoState,
    pub last_op: Option<String>,
    fingerprint: String,
    pub loaded_once: bool,

    // Changes
    pub file_rows: Vec<FileRow>,
    pub file_sel: usize,
    pub file_off: usize,
    pub diff: DiffView,
    commit_draft: Option<(String, String)>,

    // History
    pub log_all: bool,
    pub commits: Vec<Commit>,
    pub graph: Vec<Vec<Cell>>,
    builder: GraphBuilder,
    pub commit_sel: usize,
    pub commit_off: usize,
    log_generation: u64,
    pub log_exhausted: bool,
    log_loading: bool,
    pub detail: Option<CommitDetail>,
    detail_for: Option<String>,
    detail_due: Option<Instant>,
    pub cfile_sel: usize,
    pub cfile_off: usize,
    pub cdiff: DiffView,

    // Branches
    pub branch_rows: Vec<BranchRow>,
    pub branch_sel: usize,
    pub branch_off: usize,
    pub branch_log: Vec<Commit>,
    pub branch_log_for: Option<String>,
    branch_log_due: Option<Instant>,

    pub popup: Option<Popup>,
    pub notice: Option<Notice>,
    pub last_cmd: String,
    pub progress: Option<String>,
    pub busy: usize,
    started: Instant,

    pub hits: Vec<(Rect, Hit)>,
    pub page: usize,

    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ops: Sender<OpJob>,
    refresh_inflight: bool,
    refresh_pending: bool,
    force_log: bool,
    fs_dirty: Option<Instant>,

    quit: bool,
    suspend: Option<Suspend>,
    dirty: bool,
}

impl App {
    pub fn new(repo: Repo, cfg: Config, cfg_error: Option<String>) -> Self {
        let (keys, warnings) = Keymap::new(&cfg.keys);
        let (tx, rx) = mpsc::channel();
        let (ops, op_rx) = mpsc::channel();
        worker::spawn_op_thread(repo.clone(), op_rx, tx.clone());
        let mut app = Self {
            theme: Theme::new(theme::detect_depth(&cfg.color)),
            sym: Symbols::detect(&cfg.symbols),
            keys,
            mouse: cfg.mouse,
            repo,
            view: View::Changes,
            focus: Focus::Files,
            status: Status::default(),
            branches: Vec::new(),
            remotes: Vec::new(),
            state: RepoState::Clean,
            last_op: None,
            fingerprint: String::new(),
            loaded_once: false,
            file_rows: Vec::new(),
            file_sel: 0,
            file_off: 0,
            diff: DiffView::default(),
            commit_draft: None,
            log_all: cfg.all_branches,
            commits: Vec::new(),
            graph: Vec::new(),
            builder: GraphBuilder::new(),
            commit_sel: 0,
            commit_off: 0,
            log_generation: 0,
            log_exhausted: true,
            log_loading: false,
            detail: None,
            detail_for: None,
            detail_due: None,
            cfile_sel: 0,
            cfile_off: 0,
            cdiff: DiffView::default(),
            branch_rows: Vec::new(),
            branch_sel: 0,
            branch_off: 0,
            branch_log: Vec::new(),
            branch_log_for: None,
            branch_log_due: None,
            popup: None,
            notice: None,
            last_cmd: String::new(),
            progress: None,
            busy: 0,
            started: Instant::now(),
            hits: Vec::new(),
            page: 10,
            tx,
            rx,
            ops,
            refresh_inflight: false,
            refresh_pending: false,
            force_log: false,
            fs_dirty: None,
            quit: false,
            suspend: None,
            dirty: true,
        };
        if let Some(e) = cfg_error {
            app.message("Config error — using defaults", e, true);
        } else if !warnings.is_empty() {
            app.message("Config warnings", warnings.join("\n"), true);
        }
        app.request_refresh(true);
        app
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn take_suspend(&mut self) -> Option<Suspend> {
        self.suspend.take()
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn spinner(&self) -> &'static str {
        let frames = self.sym.spinner;
        frames[(self.started.elapsed().as_millis() / 100) as usize % frames.len()]
    }

    pub fn after_suspend(&mut self, outcome: Result<String, String>) {
        match outcome {
            Ok(cmd) => self.last_cmd = cmd,
            Err(e) => self.notice_err(e),
        }
        self.request_refresh(true);
        self.dirty = true;
    }

    // ---------------------------------------------------------------- notices

    fn notice(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), error: false, at: Instant::now() });
    }

    fn notice_err(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), error: true, at: Instant::now() });
    }

    fn message(&mut self, title: impl Into<String>, text: impl Into<String>, error: bool) {
        self.popup = Some(Popup::Message { title: title.into(), text: text.into(), error, scroll: 0, retry: None });
    }

    // ---------------------------------------------------------------- background

    pub fn request_refresh(&mut self, force_log: bool) {
        self.force_log |= force_log;
        if self.refresh_inflight {
            self.refresh_pending = true;
            return;
        }
        self.refresh_inflight = true;
        self.refresh_pending = false;
        let force = std::mem::take(&mut self.force_log);
        let limit = self.commits.len().clamp(LOG_PAGE, 5000);
        worker::spawn_refresh(self.repo.clone(), self.tx.clone(), self.fingerprint.clone(), force, self.log_all, limit);
    }

    fn send_job(&mut self, job: OpJob) {
        self.busy += 1;
        self.progress = Some(job.progress_label());
        let _ = self.ops.send(job);
        self.dirty = true;
    }

    pub fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.on_msg(msg);
            self.dirty = true;
        }
        let now = Instant::now();
        if self.fs_dirty.is_some_and(|t| now.duration_since(t) >= FS_DEBOUNCE) {
            self.fs_dirty = None;
            self.request_refresh(false);
        }
        if self.detail_due.is_some_and(|t| now >= t) {
            self.detail_due = None;
            self.load_detail();
            self.dirty = true;
        }
        if self.branch_log_due.is_some_and(|t| now >= t) {
            self.branch_log_due = None;
            self.load_branch_log();
        }
        if self.notice.as_ref().is_some_and(|n| n.at.elapsed() > NOTICE_TIME) {
            self.notice = None;
            self.dirty = true;
        }
        if self.busy > 0 {
            self.dirty = true;
        }
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Refreshed(r) => {
                self.refresh_inflight = false;
                match r {
                    Ok(d) => self.apply_refresh(*d),
                    Err(e) => self.notice_err(format!("Refresh failed: {e}")),
                }
                if self.refresh_pending {
                    self.request_refresh(false);
                }
            }
            Msg::LogPage { generation, result } => {
                self.log_loading = false;
                if generation != self.log_generation {
                    return;
                }
                match result {
                    Ok(page) => {
                        self.log_exhausted = page.len() < LOG_PAGE;
                        for c in &page {
                            self.graph.push(self.builder.push(c));
                        }
                        self.commits.extend(page);
                    }
                    Err(e) => self.notice_err(e),
                }
            }
            Msg::BranchLog { name, result } => {
                if self.selected_branch().map(|b| b.name.as_str()) == Some(name.as_str()) {
                    self.branch_log = result.unwrap_or_default();
                    self.branch_log_for = Some(name);
                }
            }
            Msg::Progress(line) => self.progress = Some(line),
            Msg::OpDone(o) => self.on_op_done(o),
            Msg::FsChanged => {
                if self.fs_dirty.is_none() {
                    self.fs_dirty = Some(Instant::now());
                }
            }
        }
    }

    fn on_op_done(&mut self, o: OpOutcome) {
        self.busy = self.busy.saturating_sub(1);
        if self.busy == 0 {
            self.progress = None;
        }
        if !o.command.is_empty() {
            self.last_cmd = o.command.clone();
        }
        match o.error {
            None => self.notice(o.label),
            Some(err) => {
                if let OpJob::DeleteBranch { name, force: false } = &o.job {
                    if err.contains("not fully merged") {
                        self.popup = Some(Popup::Confirm {
                            title: "Branch not merged".into(),
                            message: format!(
                                "{name} has commits that are not merged anywhere else.\nDelete it anyway? (u can still undo)"
                            ),
                            job: OpJob::DeleteBranch { name: name.clone(), force: true },
                        });
                        self.request_refresh(true);
                        return;
                    }
                }
                let mut text = err;
                if o.retry.is_some() {
                    text.push_str("\n\nIf git needed a password or passphrase, press ! to run it in the terminal.");
                }
                self.popup = Some(Popup::Message { title: "Git error".into(), text, error: true, scroll: 0, retry: o.retry });
            }
        }
        self.request_refresh(true);
    }

    fn apply_refresh(&mut self, d: RefreshData) {
        // remember selections before the indexes they point into are replaced
        let prev_file = self.file_key();
        let prev_branch = self.selected_branch().map(|b| b.name.clone());
        self.status = d.status;
        self.branches = d.branches;
        self.remotes = d.remotes;
        self.state = d.state;
        self.last_op = d.last_op;
        self.fingerprint = d.fingerprint;
        self.loaded_once = true;
        self.rebuild_files(prev_file);
        self.reload_diff();
        if let Some(log) = d.log {
            self.set_log(log, d.log_limit);
        }
        self.rebuild_branches(prev_branch);
    }

    fn log_revs(&self) -> Vec<String> {
        worker::log_revs(self.log_all, self.status.branch.oid.is_some())
    }

    fn set_log(&mut self, log: Vec<Commit>, limit: usize) {
        let selected = self.commits.get(self.commit_sel).map(|c| c.hash.clone());
        self.log_generation += 1;
        self.log_loading = false;
        self.log_exhausted = log.len() < limit;
        self.builder = GraphBuilder::new();
        self.graph = log.iter().map(|c| self.builder.push(c)).collect();
        self.commits = log;
        self.commit_sel = selected
            .and_then(|h| self.commits.iter().position(|c| c.hash == h))
            .unwrap_or_else(|| self.commit_sel.min(self.commits.len().saturating_sub(1)));
        // refs on the selected commit may have changed
        self.detail_for = None;
        self.detail_due = Some(Instant::now());
    }

    fn maybe_load_more(&mut self) {
        if self.log_exhausted || self.log_loading || self.commit_sel + 60 < self.commits.len() {
            return;
        }
        self.log_loading = true;
        worker::spawn_log_page(self.repo.clone(), self.tx.clone(), self.log_generation, self.log_revs(), self.commits.len(), LOG_PAGE);
    }

    // ---------------------------------------------------------------- changes

    fn file_key(&self) -> Option<(Section, String)> {
        match self.file_rows.get(self.file_sel)? {
            FileRow::File(s, i) => Some((*s, self.status.entries[*i].path.clone())),
            FileRow::Header(..) => None,
        }
    }

    pub fn selected_entry(&self) -> Option<(Section, &StatusEntry)> {
        match self.file_rows.get(self.file_sel)? {
            FileRow::File(s, i) => Some((*s, &self.status.entries[*i])),
            FileRow::Header(..) => None,
        }
    }

    fn rebuild_files(&mut self, prev: Option<(Section, String)>) {
        let prev_idx = self.file_sel;
        let mut rows = Vec::new();
        for sec in [Section::Conflicts, Section::Unstaged, Section::Staged] {
            let idx: Vec<usize> =
                self.status.entries.iter().enumerate().filter(|(_, e)| sec.contains(e)).map(|(i, _)| i).collect();
            if !idx.is_empty() {
                rows.push(FileRow::Header(sec, idx.len()));
                rows.extend(idx.into_iter().map(|i| FileRow::File(sec, i)));
            }
        }
        self.file_rows = rows;
        let exact = prev.and_then(|(s, p)| {
            self.file_rows.iter().position(|r| matches!(r, FileRow::File(rs, i) if *rs == s && self.status.entries[*i].path == p))
        });
        self.file_sel = exact.unwrap_or_else(|| self.nearest_file_row(prev_idx));
    }

    fn nearest_file_row(&self, idx: usize) -> usize {
        let len = self.file_rows.len();
        if len == 0 {
            return 0;
        }
        let idx = idx.min(len - 1);
        let is_file = |i: usize| matches!(self.file_rows[i], FileRow::File(..));
        (idx..len).find(|&i| is_file(i)).or_else(|| (0..idx).rev().find(|&i| is_file(i))).unwrap_or(0)
    }

    fn reload_diff(&mut self) {
        let Some((sec, e)) = self.selected_entry().map(|(s, e)| (s, e.clone())) else {
            self.diff = DiffView::default();
            if self.focus == Focus::Diff {
                self.focus = Focus::Files;
            }
            return;
        };
        let same = self.diff.path == e.path && self.diff.section == Some(sec);
        let (cursor, scroll, anchor) = (self.diff.cursor, self.diff.scroll, self.diff.anchor);
        let mut dv = DiffView { path: e.path.clone(), section: Some(sec), ..Default::default() };
        let res = match sec {
            Section::Conflicts => self.repo.diff_conflict(&e.path).map(|t| {
                dv.raw = Some(t.lines().map(str::to_string).collect());
            }),
            Section::Unstaged if e.kind == EntryKind::Untracked => {
                if e.is_dir() {
                    dv.note = Some("Untracked folder. Press space in the file list to stage everything inside.".into());
                    Ok(())
                } else {
                    self.repo.diff_untracked(&e.path).map(|d| dv.set(d, false))
                }
            }
            Section::Unstaged => self.repo.diff_worktree(&e.path).map(|d| dv.set(d, true)),
            Section::Staged => self
                .repo
                .diff_index(&e.path, e.orig_path.as_deref())
                .map(|d| dv.set(d, e.kind != EntryKind::Renamed)),
        };
        if let Err(err) = res {
            dv.note = Some(err.to_string());
        }
        if same {
            let len = dv.len();
            if len > 0 {
                dv.cursor = cursor.min(len - 1);
                dv.scroll = scroll;
                dv.anchor = anchor.filter(|a| *a < len);
            }
        }
        self.diff = dv;
    }

    fn select_file_row(&mut self, idx: usize) {
        if idx < self.file_rows.len() && matches!(self.file_rows[idx], FileRow::File(..)) && idx != self.file_sel {
            self.file_sel = idx;
            self.reload_diff();
        }
    }

    fn move_file(&mut self, delta: isize) {
        let files: Vec<usize> =
            (0..self.file_rows.len()).filter(|&i| matches!(self.file_rows[i], FileRow::File(..))).collect();
        if files.is_empty() {
            return;
        }
        let pos = files.iter().position(|&i| i == self.file_sel).unwrap_or(0) as isize;
        let next = (pos + delta).clamp(0, files.len() as isize - 1) as usize;
        self.select_file_row(files[next]);
    }

    fn has_staged(&self) -> bool {
        self.status.entries.iter().any(|e| e.is_staged())
    }

    fn stage_selected_file(&mut self) {
        let Some((sec, e)) = self.selected_entry() else { return };
        let job = match sec {
            Section::Unstaged | Section::Conflicts => OpJob::Stage(vec![e.path.clone()]),
            Section::Staged => {
                let mut paths = vec![e.path.clone()];
                if let Some(o) = &e.orig_path {
                    paths.insert(0, o.clone());
                }
                OpJob::Unstage(paths)
            }
        };
        self.send_job(job);
    }

    fn stage_in_diff(&mut self) {
        if !self.diff.patchable {
            self.stage_selected_file();
            return;
        }
        let Some(d) = &self.diff.diff else { return };
        let sel = self.diff.selected();
        let reverse = self.diff.section == Some(Section::Staged);
        match build_patch(d, |h, l| sel.contains(&(h, l)), reverse) {
            Some(patch) => {
                let path = self.diff.path.clone();
                self.diff.anchor = None;
                self.send_job(OpJob::ApplyIndex { patch, reverse, path });
            }
            None => self.notice("No changed lines selected."),
        }
    }

    fn discard_file(&mut self) {
        let Some((sec, e)) = self.selected_entry() else { return };
        let name = util::clean(&e.path);
        let (target, what) = match sec {
            Section::Conflicts => {
                self.notice_err("Resolve the conflict first, or abort the merge with `:merge --abort`.");
                return;
            }
            Section::Unstaged if e.kind == EntryKind::Untracked => {
                (DiscardTarget::Untracked { path: e.path.clone() }, format!("Delete untracked {name}?"))
            }
            Section::Unstaged => (DiscardTarget::Worktree { path: e.path.clone() }, format!("Discard unstaged changes in {name}?")),
            Section::Staged => (
                DiscardTarget::Staged { path: e.path.clone(), orig: e.orig_path.clone(), index_status: e.index },
                format!("Reset {name} to the last commit? This drops its staged and unstaged changes."),
            ),
        };
        self.popup = Some(Popup::Confirm {
            title: "Discard".into(),
            message: format!("{what}\nA backup is kept — press u afterwards to undo."),
            job: OpJob::Discard(vec![target]),
        });
    }

    fn discard_in_diff(&mut self) {
        if self.diff.section != Some(Section::Unstaged) || !self.diff.patchable {
            if self.diff.section == Some(Section::Staged) {
                self.notice("Unstage these lines first, then discard them.");
            } else {
                self.discard_file();
            }
            return;
        }
        let Some(d) = &self.diff.diff else { return };
        let sel = self.diff.selected();
        let Some(patch) = build_patch(d, |h, l| sel.contains(&(h, l)), true) else {
            self.notice("No changed lines selected.");
            return;
        };
        let what = if self.diff.anchor.is_some() { "the selected lines" } else { "this hunk" };
        let path = self.diff.path.clone();
        self.popup = Some(Popup::Confirm {
            title: "Discard".into(),
            message: format!("Discard {what} in {}?\nA backup is kept — press u afterwards to undo.", util::clean(&path)),
            job: OpJob::DiscardPatch { patch, path },
        });
        self.diff.anchor = None;
    }

    fn open_commit(&mut self, amend: bool) {
        if !amend && !self.has_staged() && self.state != RepoState::Merging {
            self.notice_err("Nothing staged yet — stage files with space first.");
            return;
        }
        let (summary, body) = if amend {
            let msg = self.repo.last_commit_message().unwrap_or_default();
            let (s, b) = msg.split_once('\n').unwrap_or((&msg, ""));
            (s.to_string(), b.trim_start_matches('\n').to_string())
        } else if let Some(d) = self.commit_draft.take() {
            d
        } else if let Some(m) = self.repo.merge_message() {
            let (s, b) = m.split_once('\n').unwrap_or((&m, ""));
            (s.to_string(), b.trim().to_string())
        } else {
            (String::new(), String::new())
        };
        self.popup = Some(Popup::Commit(CommitForm {
            summary: LineInput::with(&summary),
            body: TextArea::with(&body),
            field: 0,
            amend,
            error: None,
        }));
    }

    // ---------------------------------------------------------------- history

    pub fn selected_commit(&self) -> Option<&Commit> {
        self.commits.get(self.commit_sel)
    }

    fn select_commit(&mut self, idx: usize) {
        if self.commits.is_empty() {
            return;
        }
        let idx = idx.min(self.commits.len() - 1);
        if idx != self.commit_sel {
            self.commit_sel = idx;
            self.detail_due = Some(Instant::now() + DETAIL_DELAY);
        }
        self.maybe_load_more();
    }

    fn load_detail(&mut self) {
        let Some(hash) = self.selected_commit().map(|c| c.hash.clone()) else {
            self.detail = None;
            return;
        };
        if self.detail_for.as_deref() == Some(hash.as_str()) {
            return;
        }
        match self.repo.commit_detail(&hash) {
            Ok(d) => {
                self.detail = Some(d);
                self.detail_for = Some(hash);
                self.cfile_sel = 0;
                self.cfile_off = 0;
                self.load_cdiff();
            }
            Err(e) => {
                self.detail = None;
                self.notice_err(e.to_string());
            }
        }
    }

    fn load_cdiff(&mut self) {
        let mut dv = DiffView::default();
        if let Some(d) = &self.detail {
            if let Some(f) = d.files.get(self.cfile_sel) {
                dv.path = f.path.clone();
                match self.repo.diff_commit_file(&d.hash, d.parents.first().map(String::as_str), &f.path, f.orig.as_deref()) {
                    Ok(fd) => dv.set(fd, false),
                    Err(e) => dv.note = Some(e.to_string()),
                }
                dv.cursor = 0;
            }
        }
        self.cdiff = dv;
    }

    fn select_cfile(&mut self, idx: usize) {
        let n = self.detail.as_ref().map(|d| d.files.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        let idx = idx.min(n - 1);
        if idx != self.cfile_sel {
            self.cfile_sel = idx;
            self.load_cdiff();
        }
    }

    // ---------------------------------------------------------------- branches

    fn rebuild_branches(&mut self, prev: Option<String>) {
        let local: Vec<usize> = (0..self.branches.len()).filter(|&i| !self.branches[i].remote).collect();
        let remote: Vec<usize> = (0..self.branches.len()).filter(|&i| self.branches[i].remote).collect();
        let mut rows = Vec::new();
        if !local.is_empty() {
            rows.push(BranchRow::Header("Local", local.len()));
            rows.extend(local.into_iter().map(BranchRow::Branch));
        }
        if !remote.is_empty() {
            rows.push(BranchRow::Header("Remote", remote.len()));
            rows.extend(remote.into_iter().map(BranchRow::Branch));
        }
        self.branch_rows = rows;
        let found = prev.and_then(|n| {
            self.branch_rows.iter().position(|r| matches!(r, BranchRow::Branch(i) if self.branches[*i].name == n))
        });
        let first = self.branch_rows.iter().position(|r| matches!(r, BranchRow::Branch(_))).unwrap_or(0);
        self.branch_sel = found.unwrap_or(first);
        if self.view == View::Branches {
            self.branch_log_for = None;
            self.branch_log_due = Some(Instant::now());
        }
    }

    pub fn selected_branch(&self) -> Option<&Branch> {
        match self.branch_rows.get(self.branch_sel)? {
            BranchRow::Branch(i) => self.branches.get(*i),
            BranchRow::Header(..) => None,
        }
    }

    fn move_branch(&mut self, delta: isize) {
        let rows: Vec<usize> =
            (0..self.branch_rows.len()).filter(|&i| matches!(self.branch_rows[i], BranchRow::Branch(_))).collect();
        if rows.is_empty() {
            return;
        }
        let pos = rows.iter().position(|&i| i == self.branch_sel).unwrap_or(0) as isize;
        let next = rows[(pos + delta).clamp(0, rows.len() as isize - 1) as usize];
        self.select_branch_row(next);
    }

    fn select_branch_row(&mut self, idx: usize) {
        if idx < self.branch_rows.len() && matches!(self.branch_rows[idx], BranchRow::Branch(_)) && idx != self.branch_sel {
            self.branch_sel = idx;
            self.branch_log_due = Some(Instant::now() + DETAIL_DELAY);
        }
    }

    fn load_branch_log(&mut self) {
        if let Some(b) = self.selected_branch() {
            if self.branch_log_for.as_deref() != Some(b.name.as_str()) {
                worker::spawn_branch_log(self.repo.clone(), self.tx.clone(), b.name.clone(), b.full.clone());
            }
        }
    }

    fn checkout_selected(&mut self) {
        let Some(b) = self.selected_branch().cloned() else { return };
        if b.head {
            self.notice(format!("Already on {}", b.name));
            return;
        }
        if !b.remote {
            self.send_job(OpJob::Switch(b.name));
            return;
        }
        let local = b.local_name().to_string();
        if self.branches.iter().any(|x| !x.remote && x.name == local) {
            self.send_job(OpJob::Switch(local));
        } else {
            self.send_job(OpJob::SwitchTrack(b.name));
        }
    }

    // ---------------------------------------------------------------- events

    pub fn on_event(&mut self, ev: Event) {
        self.dirty = true;
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Mouse(m) if self.mouse => self.on_mouse(m),
            Event::Paste(text) => self.on_paste(&text),
            Event::FocusGained => self.request_refresh(false),
            _ => {}
        }
    }

    fn on_paste(&mut self, text: &str) {
        match &mut self.popup {
            Some(Popup::Input { input, .. }) => input.insert_str(&text.replace(['\r', '\n'], " ")),
            Some(Popup::Commit(f)) if f.field == 0 => {
                let mut lines = text.splitn(2, '\n');
                f.summary.insert_str(lines.next().unwrap_or_default().trim_end_matches('\r'));
                if let Some(rest) = lines.next() {
                    f.field = 1;
                    f.body.insert_str(&rest.replace('\r', ""));
                }
            }
            Some(Popup::Commit(f)) => f.body.insert_str(&text.replace('\r', "")),
            _ => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if self.popup.is_some() {
            self.on_popup_key(key);
            return;
        }
        let chord = Chord::from_event(&key);
        let action = self.keys.find(self.focus.actions(), chord).or_else(|| self.keys.find(keys::GLOBAL, chord));
        if let Some(a) = action {
            self.dispatch(a);
        }
    }

    fn set_view(&mut self, v: View) {
        self.view = v;
        self.focus = match v {
            View::Changes => Focus::Files,
            View::History => Focus::Commits,
            View::Branches => Focus::Branches,
        };
        match v {
            View::History => {
                if self.detail_for.is_none() {
                    self.detail_due = Some(Instant::now());
                }
            }
            View::Branches => self.branch_log_due = Some(Instant::now()),
            View::Changes => {}
        }
    }

    fn cycle_focus(&mut self, forward: bool) {
        let order: &[Focus] = match self.view {
            View::Changes => &[Focus::Files, Focus::Diff],
            View::History => &[Focus::Commits, Focus::CommitFiles, Focus::CommitDiff],
            View::Branches => &[Focus::Branches],
        };
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        let n = order.len();
        let next = if forward { (i + 1) % n } else { (i + n - 1) % n };
        self.focus = order[next];
    }

    fn move_by(&mut self, delta: isize) {
        match self.focus {
            Focus::Files => self.move_file(delta),
            Focus::Diff => self.diff.move_cursor(delta),
            Focus::Commits => self.select_commit((self.commit_sel as isize + delta).max(0) as usize),
            Focus::CommitFiles => self.select_cfile((self.cfile_sel as isize + delta).max(0) as usize),
            Focus::CommitDiff => self.cdiff.move_cursor(delta),
            Focus::Branches => self.move_branch(delta),
        }
    }

    fn dispatch(&mut self, a: Action) {
        let page = self.page.max(1) as isize;
        match a {
            Action::Quit => self.quit = true,
            Action::Help => self.popup = Some(Popup::Help { scroll: 0 }),
            Action::ViewChanges => self.set_view(View::Changes),
            Action::ViewHistory => self.set_view(View::History),
            Action::ViewBranches => self.set_view(View::Branches),
            Action::NextFocus => self.cycle_focus(true),
            Action::PrevFocus => self.cycle_focus(false),
            Action::Refresh => {
                self.request_refresh(true);
                self.notice("Refreshed");
            }
            Action::Undo => match self.last_op.clone() {
                Some(_) if self.busy > 0 => self.notice("Wait for the running operation to finish."),
                Some(_) => self.send_job(OpJob::Undo),
                None => self.notice("Nothing to undo."),
            },
            Action::Fetch => self.send_job(OpJob::Fetch),
            Action::Pull => {
                if self.status.branch.upstream.is_none() {
                    let b = self.status.branch.head.clone().unwrap_or_else(|| "HEAD".into());
                    self.notice_err(format!("{b} has no upstream. Push it first (P) or run `:branch -u origin/{b}`."));
                } else {
                    self.send_job(OpJob::Pull);
                }
            }
            Action::Push => self.push(),
            Action::GitCommand => {
                self.popup = Some(Popup::Input {
                    title: "Run git command (in the terminal)".into(),
                    input: LineInput::default(),
                    purpose: InputPurpose::GitCommand,
                })
            }
            Action::Up => self.move_by(-1),
            Action::Down => self.move_by(1),
            Action::PageUp => self.move_by(-page),
            Action::PageDown => self.move_by(page),
            Action::Top => self.move_by(-1_000_000),
            Action::Bottom => self.move_by(1_000_000),
            Action::Open => match self.focus {
                Focus::Files if self.diff.len() > 0 => self.focus = Focus::Diff,
                Focus::Commits => self.focus = Focus::CommitFiles,
                Focus::CommitFiles if self.cdiff.len() > 0 => self.focus = Focus::CommitDiff,
                _ => {}
            },
            Action::Back => match self.focus {
                Focus::Diff if self.diff.anchor.is_some() => self.diff.anchor = None,
                Focus::Diff => self.focus = Focus::Files,
                Focus::CommitFiles => self.focus = Focus::Commits,
                Focus::CommitDiff => self.focus = Focus::CommitFiles,
                _ => {}
            },
            Action::Stage => match self.focus {
                Focus::Files => self.stage_selected_file(),
                Focus::Diff => self.stage_in_diff(),
                _ => {}
            },
            Action::StageAll => {
                if self.status.entries.iter().any(|e| e.is_unstaged() || e.is_conflict()) {
                    self.send_job(OpJob::StageAll);
                } else if self.has_staged() {
                    self.send_job(OpJob::UnstageAll);
                }
            }
            Action::Discard => match self.focus {
                Focus::Files => self.discard_file(),
                Focus::Diff => self.discard_in_diff(),
                _ => {}
            },
            Action::Commit => self.open_commit(false),
            Action::Amend => {
                if self.status.branch.oid.is_none() {
                    self.notice_err("There is no commit to amend yet.");
                } else {
                    self.open_commit(true);
                }
            }
            Action::CommitEditor => {
                if !self.has_staged() && self.state != RepoState::Merging {
                    self.notice_err("Nothing staged yet — stage files with space first.");
                } else {
                    self.suspend = Some(Suspend {
                        args: vec!["commit".into()],
                        record: Some((OpKind::Commit, "Commit (editor)".into())),
                    });
                }
            }
            Action::SelectLines => {
                if self.diff.patchable {
                    self.diff.anchor = match self.diff.anchor {
                        Some(_) => None,
                        None => Some(self.diff.cursor),
                    };
                }
            }
            Action::NextHunk | Action::PrevHunk => {
                let fwd = a == Action::NextHunk;
                if self.focus == Focus::CommitDiff { self.cdiff.jump_hunk(fwd) } else { self.diff.jump_hunk(fwd) }
            }
            Action::ToggleAllBranches => {
                self.log_all = !self.log_all;
                self.request_refresh(true);
                self.notice(if self.log_all { "Showing all branches" } else { "Showing the current branch only" });
            }
            Action::CopyHash => {
                if let Some(c) = self.selected_commit() {
                    let h = c.hash.clone();
                    util::copy_to_clipboard(&h);
                    self.notice(format!("Copied {}", &h[..h.len().min(12)]));
                }
            }
            Action::NewBranch => {
                let start = match self.focus {
                    Focus::Commits => self.selected_commit().map(|c| c.hash.clone()),
                    Focus::Branches => self.selected_branch().map(|b| b.name.clone()),
                    _ => None,
                };
                let from = start.as_deref().map(|s| if s.len() == 40 { &s[..7] } else { s }).unwrap_or("HEAD").to_string();
                self.popup = Some(Popup::Input {
                    title: format!("New branch from {from}"),
                    input: LineInput::default(),
                    purpose: InputPurpose::NewBranch { start },
                });
            }
            Action::Checkout => self.checkout_selected(),
            Action::DeleteBranch => {
                let Some(b) = self.selected_branch().cloned() else { return };
                if b.remote {
                    self.notice_err(format!("To delete a remote branch run `:push {} --delete {}`", b.name.split('/').next().unwrap_or("origin"), b.local_name()));
                } else if b.head {
                    self.notice_err("You can't delete the branch you're on. Switch to another branch first.");
                } else {
                    self.popup = Some(Popup::Confirm {
                        title: "Delete branch".into(),
                        message: format!("Delete local branch {}?\nPress u afterwards to undo.", b.name),
                        job: OpJob::DeleteBranch { name: b.name, force: false },
                    });
                }
            }
            Action::RenameBranch => {
                let Some(b) = self.selected_branch().cloned() else { return };
                if b.remote {
                    self.notice_err("Remote branches can't be renamed from here.");
                } else {
                    self.popup = Some(Popup::Input {
                        title: format!("Rename {}", b.name),
                        input: LineInput::with(&b.name),
                        purpose: InputPurpose::RenameBranch { old: b.name },
                    });
                }
            }
        }
    }

    fn push(&mut self) {
        let b = &self.status.branch;
        let Some(head) = b.head.clone() else {
            self.notice_err("HEAD is detached — switch to a branch before pushing.");
            return;
        };
        let set_upstream = if b.upstream.is_some() {
            None
        } else {
            let remote = if self.remotes.iter().any(|r| r == "origin") { Some("origin".to_string()) } else { self.remotes.first().cloned() };
            match remote {
                Some(r) => Some((r, head)),
                None => {
                    self.notice_err("No remote configured. Add one with `:remote add origin <url>`.");
                    return;
                }
            }
        };
        self.send_job(OpJob::Push { set_upstream });
    }

    fn on_popup_key(&mut self, key: KeyEvent) {
        let Some(popup) = self.popup.take() else { return };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let keep = match popup {
            Popup::Help { mut scroll } => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    scroll = scroll.saturating_add(1);
                    Some(Popup::Help { scroll })
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    scroll = scroll.saturating_sub(1);
                    Some(Popup::Help { scroll })
                }
                _ => None,
            },
            Popup::Confirm { title, message, job } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.send_job(job);
                    None
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => None,
                _ => Some(Popup::Confirm { title, message, job }),
            },
            Popup::Message { title, text, error, mut scroll, retry } => match key.code {
                KeyCode::Char('!') if retry.is_some() => {
                    self.suspend = Some(Suspend { args: retry.unwrap_or_default(), record: None });
                    None
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    scroll = scroll.saturating_add(1);
                    Some(Popup::Message { title, text, error, scroll, retry })
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    scroll = scroll.saturating_sub(1);
                    Some(Popup::Message { title, text, error, scroll, retry })
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => None,
                _ => Some(Popup::Message { title, text, error, scroll, retry }),
            },
            Popup::Input { title, mut input, purpose } => match key.code {
                KeyCode::Esc => None,
                KeyCode::Enter => {
                    self.submit_input(input.text.trim().to_string(), purpose);
                    None
                }
                _ => {
                    input.handle(&key);
                    Some(Popup::Input { title, input, purpose })
                }
            },
            Popup::Commit(mut f) => {
                let submit = (key.code == KeyCode::Enter && f.field == 0) || (ctrl && key.code == KeyCode::Char('s'));
                if key.code == KeyCode::Esc {
                    if !f.amend {
                        let draft = (f.summary.text.clone(), f.body.text());
                        if !draft.0.is_empty() || !draft.1.trim().is_empty() {
                            self.commit_draft = Some(draft);
                        }
                    }
                    None
                } else if submit {
                    let summary = f.summary.text.trim().to_string();
                    if summary.is_empty() {
                        f.error = Some("Write a short summary first.".into());
                        Some(Popup::Commit(f))
                    } else {
                        let body = f.body.text().trim().to_string();
                        let message = if body.is_empty() { summary } else { format!("{summary}\n\n{body}") };
                        self.send_job(OpJob::Commit { message, amend: f.amend });
                        None
                    }
                } else if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                    f.field = 1 - f.field;
                    Some(Popup::Commit(f))
                } else {
                    f.error = None;
                    if f.field == 0 {
                        if key.code == KeyCode::Down {
                            f.field = 1;
                        } else {
                            f.summary.handle(&key);
                        }
                    } else if key.code == KeyCode::Up && f.body.row == 0 {
                        f.field = 0;
                    } else {
                        f.body.handle(&key);
                    }
                    Some(Popup::Commit(f))
                }
            }
        };
        if self.popup.is_none() {
            self.popup = keep;
        }
    }

    fn submit_input(&mut self, text: String, purpose: InputPurpose) {
        if text.is_empty() {
            return;
        }
        match purpose {
            InputPurpose::NewBranch { start } => {
                if text.contains(char::is_whitespace) {
                    self.notice_err("Branch names can't contain spaces.");
                } else {
                    self.send_job(OpJob::CreateBranch { name: text, start });
                }
            }
            InputPurpose::RenameBranch { old } => {
                if text != old {
                    self.send_job(OpJob::RenameBranch { old, new: text });
                }
            }
            InputPurpose::GitCommand => {
                let mut args = util::shell_split(&text);
                if args.first().map(String::as_str) == Some("git") {
                    args.remove(0);
                }
                if !args.is_empty() {
                    self.suspend = Some(Suspend { args, record: None });
                }
            }
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        if self.popup.is_some() {
            return;
        }
        let pos = Position { x: m.column, y: m.row };
        let Some((rect, hit)) = self.hits.iter().rev().find(|(r, _)| r.contains(pos)).copied() else { return };
        match (m.kind, hit) {
            (MouseEventKind::Down(MouseButton::Left), Hit::Tab(v)) => self.set_view(v),
            (MouseEventKind::Down(MouseButton::Left), Hit::List(f, offset)) => {
                if f.view() != self.view {
                    return;
                }
                self.focus = f;
                let row = offset + (m.row - rect.y) as usize;
                match f {
                    Focus::Files => self.select_file_row(row),
                    Focus::Diff => {
                        if row < self.diff.len() {
                            self.diff.cursor = row;
                        }
                    }
                    Focus::Commits => self.select_commit(row),
                    Focus::CommitFiles => self.select_cfile(row),
                    Focus::CommitDiff => {
                        if row < self.cdiff.len() {
                            self.cdiff.cursor = row;
                        }
                    }
                    Focus::Branches => self.select_branch_row(row),
                }
            }
            (MouseEventKind::ScrollDown | MouseEventKind::ScrollUp, Hit::List(f, _)) if f.view() == self.view => {
                self.focus = f;
                self.move_by(if m.kind == MouseEventKind::ScrollDown { 3 } else { -3 });
            }
            _ => {}
        }
    }
}
