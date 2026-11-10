//! Background work: repository refreshes and mutating operations.
//!
//! Operations run one at a time on a dedicated thread so two git commands never
//! fight over `index.lock`. Refreshes are read-only and run on their own thread.

use std::sync::mpsc::{Receiver, Sender};


use gitty_core::branch::Branch;
use gitty_core::insight::{Blob, Insight};
use gitty_core::log::{Commit, LogQuery};
use gitty_core::ops::{DiscardTarget, describe_paths};
use gitty_core::rebase::RebaseStep;
use gitty_core::stash::Stash;
use gitty_core::status::Status;
use gitty_core::{Repo, RepoState};

pub enum Msg {
    Refreshed(Result<Box<RefreshData>, String>),
    LogPage { generation: u64, result: Result<Vec<Commit>, String> },
    BranchLog { name: String, result: Result<Vec<Commit>, String> },
    Progress(String),
    OpDone(OpOutcome),
    FsChanged,
    Insight(Result<Box<Insight>, String>),
    Blobs(Result<Vec<Blob>, String>),
}

pub struct RefreshData {
    pub status: Status,
    pub branches: Vec<Branch>,
    pub remotes: Vec<String>,
    pub state: RepoState,
    pub fingerprint: String,
    pub log: Option<Vec<Commit>>,
    pub log_limit: usize,
    pub last_op: Option<String>,
    pub stashes: Vec<Stash>,
}

#[derive(Clone, Debug)]
pub enum OpJob {
    Stage(Vec<String>),
    Unstage(Vec<String>),
    StageAll,
    UnstageAll,
    ApplyIndex { patch: String, reverse: bool, path: String },
    DiscardPatch { patch: String, path: String },
    Discard(Vec<DiscardTarget>),
    Commit { message: String, amend: bool },
    Switch(String),
    SwitchTrack(String),
    CreateBranch { name: String, start: Option<String> },
    DeleteBranch { name: String, force: bool },
    RenameBranch { old: String, new: String },
    Fetch,
    Pull,
    Push { set_upstream: Option<(String, String)> },
    Undo,
    StashPush { message: String, untracked: bool },
    StashApply { name: String, pop: bool },
    StashDrop(Stash),
    Rebase { base: Option<String>, steps: Vec<RebaseStep> },
    Continue,
    Abort,
    Skip,
    Resolve { path: String, text: String, what: String },
    TakeSide { path: String, ours: bool },
    ShowBase(String),
    ResolveDelete(String),
    ApplyIgnore { pattern: String, untrack: Vec<String> },
    AddIgnore { rel: String, pattern: String, untrack: Vec<String> },
    RemoveIgnore { rel: String, line: usize, text: String },
    RewordCommit { hash: String, message: String },
    LfsTrack { patterns: Vec<String>, convert: Vec<String> },
}

impl OpJob {
    pub fn progress_label(&self) -> String {
        match self {
            OpJob::Stage(_) | OpJob::StageAll => "Staging…".into(),
            OpJob::Unstage(_) | OpJob::UnstageAll => "Unstaging…".into(),
            OpJob::ApplyIndex { reverse, .. } => if *reverse { "Unstaging lines…" } else { "Staging lines…" }.into(),
            OpJob::DiscardPatch { .. } | OpJob::Discard(_) => "Discarding…".into(),
            OpJob::Commit { .. } => "Committing…".into(),
            OpJob::Switch(n) => format!("Switching to {n}…"),
            OpJob::SwitchTrack(n) => format!("Checking out {n}…"),
            OpJob::CreateBranch { name, .. } => format!("Creating {name}…"),
            OpJob::DeleteBranch { name, .. } => format!("Deleting {name}…"),
            OpJob::RenameBranch { .. } => "Renaming…".into(),
            OpJob::Fetch => "Fetching…".into(),
            OpJob::Pull => "Pulling…".into(),
            OpJob::Push { .. } => "Pushing…".into(),
            OpJob::Undo => "Undoing…".into(),
            OpJob::StashPush { .. } => "Stashing…".into(),
            OpJob::StashApply { pop, .. } => if *pop { "Popping stash…" } else { "Applying stash…" }.into(),
            OpJob::StashDrop(_) => "Dropping stash…".into(),
            OpJob::Rebase { .. } => "Rebasing…".into(),
            OpJob::Continue => "Continuing…".into(),
            OpJob::Abort => "Aborting…".into(),
            OpJob::Skip => "Skipping…".into(),
            OpJob::Resolve { .. } | OpJob::TakeSide { .. } | OpJob::ShowBase(_) | OpJob::ResolveDelete(_) => "Resolving…".into(),
            OpJob::ApplyIgnore { .. } | OpJob::AddIgnore { .. } | OpJob::RemoveIgnore { .. } => "Updating ignore rules…".into(),
            OpJob::RewordCommit { .. } => "Rewriting the message…".into(),
            OpJob::LfsTrack { .. } => "Moving files to LFS…".into(),
        }
    }
}

pub struct OpOutcome {
    pub job: OpJob,
    pub label: String,
    pub command: String,
    pub error: Option<String>,
    /// Arguments to retry the command interactively (for auth prompts).
    pub retry: Option<Vec<String>>,
}

pub fn spawn_op_thread(repo: Repo, jobs: Receiver<OpJob>, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        for job in jobs {
            let outcome = run_job(&repo, job, &tx);
            if tx.send(Msg::OpDone(outcome)).is_err() {
                break;
            }
        }
    });
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn run_job(repo: &Repo, job: OpJob, tx: &Sender<Msg>) -> OpOutcome {
    let progress = |l: &str| {
        let _ = tx.send(Msg::Progress(l.to_string()));
    };
    let mut retry = None;
    let (label, result) = match &job {
        OpJob::Stage(p) => (format!("Staged {}", describe_paths(p)), repo.stage(p)),
        OpJob::Unstage(p) => (format!("Unstaged {}", describe_paths(p)), repo.unstage(p)),
        OpJob::StageAll => ("Staged all changes".into(), repo.stage_all()),
        OpJob::UnstageAll => ("Unstaged everything".into(), repo.unstage_all()),
        OpJob::ApplyIndex { patch, reverse, path } => {
            let what = if *reverse { "Unstaged" } else { "Staged" };
            (format!("{what} lines in {}", describe_paths(std::slice::from_ref(path))), repo.apply_to_index(patch, *reverse, path))
        }
        OpJob::DiscardPatch { patch, path } => {
            (format!("Discarded lines in {} — u to undo", describe_paths(std::slice::from_ref(path))), repo.discard_patch(patch, path))
        }
        OpJob::Discard(t) => {
            let paths: Vec<String> = t.iter().map(|t| t.path().to_string()).collect();
            (format!("Discarded {} — u to undo", describe_paths(&paths)), repo.discard(t))
        }
        OpJob::Commit { message, amend } => {
            let summary = message.lines().next().unwrap_or_default();
            (format!("{} \"{summary}\"", if *amend { "Amended" } else { "Committed" }), repo.commit(message, *amend))
        }
        OpJob::Switch(n) => (format!("Switched to {n}"), repo.switch(n)),
        OpJob::SwitchTrack(n) => (format!("Checked out {n}"), repo.switch_track(n)),
        OpJob::CreateBranch { name, start } => (format!("Created and switched to {name}"), repo.create_branch(name, start.as_deref())),
        OpJob::DeleteBranch { name, force } => (format!("Deleted {name} — u to undo"), repo.delete_branch(name, *force)),
        OpJob::RenameBranch { old, new } => (format!("Renamed {old} to {new}"), repo.rename_branch(old, new)),
        OpJob::Fetch => {
            retry = Some(strings(&["fetch", "--all", "--prune"]));
            ("Fetched all remotes".into(), repo.fetch(progress))
        }
        OpJob::Pull => {
            retry = Some(strings(&["pull"]));
            ("Pulled — u to undo".into(), repo.pull(progress))
        }
        OpJob::Push { set_upstream } => {
            let mut args = strings(&["push"]);
            if let Some((r, b)) = set_upstream {
                args.extend([String::from("-u"), r.clone(), b.clone()]);
            }
            retry = Some(args);
            let up = set_upstream.as_ref().map(|(r, b)| (r.as_str(), b.as_str()));
            ("Pushed".into(), repo.push(up, progress))
        }
        OpJob::Undo => match repo.undo_last() {
            Ok(desc) => (format!("Undid: {desc}"), Ok("gitty undo".to_string())),
            Err(e) => ("Undo".into(), Err(e)),
        },
        OpJob::StashPush { message, untracked } => ("Stashed — u to undo".into(), repo.stash_push(message, *untracked)),
        OpJob::StashApply { name, pop } => {
            (format!("{} {name}", if *pop { "Popped" } else { "Applied" }), repo.stash_apply(name, *pop))
        }
        OpJob::StashDrop(st) => (format!("Dropped {} — u to undo", st.name), repo.stash_drop(st)),
        OpJob::Rebase { base, steps } => ("Rebased — u to undo".into(), repo.run_rebase(base.as_deref(), steps)),
        OpJob::Continue => ("Continued".into(), repo.continue_operation()),
        OpJob::Abort => ("Aborted".into(), repo.abort_operation()),
        OpJob::Skip => ("Skipped commit".into(), repo.skip_operation()),
        OpJob::Resolve { path, text, what } => (format!("{what} — u to undo"), repo.write_resolution(path, text, what)),
        OpJob::TakeSide { path, ours } => {
            (format!("Took {} for the whole file", if *ours { "ours" } else { "theirs" }), repo.take_side(path, *ours))
        }
        OpJob::ShowBase(path) => ("Showing the original version between the sides".into(), repo.show_conflict_base(path)),
        OpJob::ResolveDelete(path) => (format!("Deleted {path}"), repo.resolve_delete(path)),
        OpJob::ApplyIgnore { pattern, untrack } => (format!("Ignored {pattern} — u to undo"), repo.apply_ignore(pattern, untrack)),
        OpJob::AddIgnore { rel, pattern, untrack } => {
            (format!("Added {pattern} to {rel} — u to undo"), repo.add_ignore_rule(rel, pattern, untrack))
        }
        OpJob::RemoveIgnore { rel, line, text } => {
            (format!("Removed {text} from {rel} — u to undo"), repo.remove_ignore_rule(rel, *line, text))
        }
        OpJob::RewordCommit { hash, message } => {
            let summary = message.lines().next().unwrap_or_default();
            (format!("Message of {} is now \"{summary}\" — u to undo", &hash[..7]), repo.reword_commit(hash, message))
        }
        OpJob::LfsTrack { patterns, convert } => {
            (format!("Tracking {} with LFS — commit to finish", patterns.join(" ")), repo.lfs_track(patterns, convert))
        }
    };
    match result {
        Ok(command) => OpOutcome { job, label, command, error: None, retry: None },
        Err(e) => OpOutcome { job, label, command: String::new(), error: Some(e.to_string()), retry },
    }
}

pub fn log_revs(all: bool, has_head: bool) -> Vec<String> {
    let mut v = Vec::new();
    if all {
        v.extend(["--branches", "--remotes", "--tags"].map(String::from));
    }
    if has_head {
        v.push("HEAD".into());
    }
    v
}

pub fn spawn_refresh(repo: Repo, tx: Sender<Msg>, last_fingerprint: String, force_log: bool, all: bool, limit: usize) {
    std::thread::spawn(move || {
        let r = refresh(&repo, &last_fingerprint, force_log, all, limit).map(Box::new).map_err(|e| e.to_string());
        let _ = tx.send(Msg::Refreshed(r));
    });
}

fn refresh(repo: &Repo, last: &str, force_log: bool, all: bool, limit: usize) -> gitty_core::Result<RefreshData> {
    let status = repo.status()?;
    let fingerprint = format!("{}|{:?}|{:?}", repo.refs_fingerprint()?, status.branch.head, status.branch.oid);
    let branches = repo.branches()?;
    let remotes = repo.remotes()?;
    let log = if force_log || fingerprint != last {
        let revs = log_revs(all, status.branch.oid.is_some());
        Some(repo.log(&LogQuery { revs, skip: 0, limit })?)
    } else {
        None
    };
    Ok(RefreshData {
        status,
        branches,
        remotes,
        state: repo.state(),
        fingerprint,
        log,
        log_limit: limit,
        last_op: repo.last_op().map(|e| e.description),
        stashes: repo.stashes().unwrap_or_default(),
    })
}

pub fn spawn_log_page(repo: Repo, tx: Sender<Msg>, generation: u64, revs: Vec<String>, skip: usize, limit: usize) {
    std::thread::spawn(move || {
        let result = repo.log(&LogQuery { revs, skip, limit }).map_err(|e| e.to_string());
        let _ = tx.send(Msg::LogPage { generation, result });
    });
}

pub fn spawn_branch_log(repo: Repo, tx: Sender<Msg>, name: String, full_ref: String) {
    std::thread::spawn(move || {
        let result = repo.log(&LogQuery { revs: vec![full_ref], skip: 0, limit: 60 }).map_err(|e| e.to_string());
        let _ = tx.send(Msg::BranchLog { name, result });
    });
}

pub fn spawn_insight(repo: Repo, tx: Sender<Msg>, use_cache: bool) {
    let tx2 = tx.clone();
    let repo2 = repo.clone();
    std::thread::spawn(move || {
        let r = repo.insight().map(Box::new).map_err(|e| e.to_string());
        let _ = tx.send(Msg::Insight(r));
    });
    // the history scan is much slower; run it alongside
    std::thread::spawn(move || {
        let r = repo2.largest_blobs(25, use_cache).map_err(|e| e.to_string());
        let _ = tx2.send(Msg::Blobs(r));
    });
}

