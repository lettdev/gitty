//! Tauri commands: thin wrappers around gitty-core. All git work runs on a
//! blocking thread so the window never freezes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine;
use gitty_core::branch::Branch;
use gitty_core::diff::{FileDiff, build_patch};
use gitty_core::graph::{Cell, GraphBuilder};
use gitty_core::insight::{Blob, Insight};
use gitty_core::log::{Commit, CommitDetail, LogQuery};
use gitty_core::ops::DiscardTarget;
use gitty_core::stash::Stash;
use gitty_core::status::{EntryKind, Status};
use gitty_core::workspace::{self, RepoSummary};
use gitty_core::{Repo, RepoState, unity};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

type Res<T> = Result<T, String>;

#[derive(Default)]
pub struct AppState {
    repo: Mutex<Option<Repo>>,
}

fn current(state: &State<'_, AppState>) -> Res<Repo> {
    state.repo.lock().map_err(|e| e.to_string())?.clone().ok_or_else(|| "No repository is open.".to_string())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
pub struct RepoInfo {
    name: String,
    root: String,
}

#[derive(Serialize)]
pub struct OpResult {
    command: String,
    message: String,
}

fn ok(command: String, message: impl Into<String>) -> Res<OpResult> {
    Ok(OpResult { command, message: message.into() })
}

// ------------------------------------------------------------------ repositories

fn recent_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("recent.json"))
}

fn load_recent(app: &AppHandle) -> Vec<String> {
    recent_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn remember(app: &AppHandle, root: &Path) {
    let Some(p) = recent_path(app) else { return };
    let root = root.display().to_string();
    let mut list = load_recent(app);
    list.retain(|r| r != &root);
    list.insert(0, root);
    list.truncate(20);
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, serde_json::to_string(&list).unwrap_or_default());
}

fn open(app: &AppHandle, state: &State<'_, AppState>, path: &Path) -> Res<RepoInfo> {
    let repo = Repo::discover(path).map_err(err)?;
    remember(app, repo.root());
    let info = RepoInfo { name: repo.name().to_string(), root: repo.root().display().to_string() };
    *state.repo.lock().map_err(err)? = Some(repo);
    Ok(info)
}

/// The repository to show at launch: a path argument, the working directory,
/// or the most recently opened one.
#[tauri::command]
pub async fn initial_repo(app: AppHandle, state: State<'_, AppState>) -> Res<Option<RepoInfo>> {
    let mut candidates: Vec<PathBuf> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).map(PathBuf::from).collect();
    if let Ok(cwd) = std::env::current_dir() {
        if cwd != Path::new("/") {
            candidates.push(cwd);
        }
    }
    candidates.extend(load_recent(&app).into_iter().map(PathBuf::from));
    for c in candidates {
        if let Ok(info) = open(&app, &state, &c) {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

/// View to start in (`GITTY_VIEW=history`), handy for screenshots and demos.
#[tauri::command]
pub fn start_view() -> Option<String> {
    std::env::var("GITTY_VIEW").ok()
}

#[tauri::command]
pub async fn open_repo(app: AppHandle, state: State<'_, AppState>, path: String) -> Res<RepoInfo> {
    open(&app, &state, Path::new(&path))
}

/// Recent repositories plus their siblings, summarized.
#[tauri::command]
pub async fn repo_list(app: AppHandle) -> Res<Vec<RepoSummary>> {
    let recent = load_recent(&app);
    blocking(move || {
        let mut roots: Vec<PathBuf> = Vec::new();
        for r in &recent {
            if let Some(parent) = Path::new(r).parent() {
                if !roots.iter().any(|x| x == parent) {
                    roots.push(parent.to_path_buf());
                }
            }
        }
        let mut paths: Vec<PathBuf> = recent.iter().map(PathBuf::from).filter(|p| p.join(".git").exists()).collect();
        for p in workspace::find_repos(&roots, 2) {
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        paths.truncate(60);
        let handles: Vec<_> = paths.into_iter().map(|p| std::thread::spawn(move || workspace::summarize(&p))).collect();
        let mut out: Vec<RepoSummary> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.last_commit));
        Ok(out)
    })
    .await
}

// ------------------------------------------------------------------ status

#[derive(Serialize)]
pub struct StatusDto {
    status: Status,
    state: RepoState,
    state_label: Option<&'static str>,
    stashes: Vec<Stash>,
    last_op: Option<String>,
    fingerprint: String,
    remotes: Vec<String>,
}

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> Res<StatusDto> {
    let repo = current(&state)?;
    blocking(move || {
        let status = repo.status().map_err(err)?;
        let fingerprint = format!("{}|{:?}|{:?}", repo.refs_fingerprint().unwrap_or_default(), status.branch.head, status.branch.oid);
        let st = repo.state();
        Ok(StatusDto {
            status,
            state: st,
            state_label: st.label(),
            stashes: repo.stashes().unwrap_or_default(),
            last_op: repo.last_op().map(|e| e.description),
            fingerprint,
            remotes: repo.remotes().unwrap_or_default(),
        })
    })
    .await
}

// ------------------------------------------------------------------ diffs

#[derive(Serialize, Default)]
pub struct ImagePair {
    before: Option<String>,
    after: Option<String>,
    before_size: Option<usize>,
    after_size: Option<usize>,
}

#[derive(Serialize, Default)]
pub struct DiffDto {
    diff: Option<FileDiff>,
    summary: Vec<String>,
    image: Option<ImagePair>,
    note: Option<String>,
    raw: Option<String>,
    patchable: bool,
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif"];
const MAX_IMAGE: usize = 30 * 1024 * 1024;

fn image_mime(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return None;
    }
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => "image/avif",
    })
}

/// Read a version of a file: `None` rev = worktree, `Some("")` = index.
fn read_bytes(repo: &Repo, rev: Option<&str>, path: &str) -> Option<Vec<u8>> {
    match rev {
        None => std::fs::read(repo.root().join(path)).ok(),
        Some(r) => repo.git().run(&["cat-file", "blob", &format!("{r}:{path}")]).ok(),
    }
}

fn data_url(mime: &str, bytes: Option<Vec<u8>>) -> (Option<String>, Option<usize>) {
    match bytes {
        Some(b) if b.len() <= MAX_IMAGE => {
            let n = b.len();
            (Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(b))), Some(n))
        }
        Some(b) => (None, Some(b.len())),
        None => (None, None),
    }
}

fn image_pair(repo: &Repo, path: &str, old_path: &str, old: Option<&str>, new: Option<&str>, has_old: bool) -> Option<ImagePair> {
    let mime = image_mime(path)?;
    let (before, before_size) = if has_old { data_url(mime, read_bytes(repo, old, old_path)) } else { (None, None) };
    let (after, after_size) = data_url(mime, read_bytes(repo, new, path));
    Some(ImagePair { before, after, before_size, after_size })
}

#[tauri::command]
pub async fn file_diff(state: State<'_, AppState>, path: String, section: String, orig: Option<String>) -> Res<DiffDto> {
    let repo = current(&state)?;
    blocking(move || {
        let st = repo.status().map_err(err)?;
        let entry = st.entries.iter().find(|e| e.path == path).cloned();
        let mut dto = DiffDto::default();
        match section.as_str() {
            "conflicts" => {
                dto.raw = Some(String::from_utf8_lossy(&std::fs::read(repo.root().join(&path)).unwrap_or_default()).into_owned());
            }
            "unstaged" if entry.as_ref().is_some_and(|e| e.kind == EntryKind::Untracked) => {
                if path.ends_with('/') {
                    dto.note = Some("Untracked folder — stage it to add everything inside.".into());
                } else {
                    dto.image = image_pair(&repo, &path, &path, None, None, false);
                    if dto.image.is_none() {
                        dto.diff = Some(repo.diff_untracked(&path).map_err(err)?);
                    }
                }
            }
            "unstaged" => {
                let d = repo.diff_worktree(&path).map_err(err)?;
                dto.patchable = !d.binary && !d.hunks.is_empty();
                dto.image = if d.binary { image_pair(&repo, &path, &path, Some(""), None, true) } else { None };
                dto.summary = summary(&repo, &path, &path, Some(""), None, &d);
                dto.diff = Some(d);
            }
            _ => {
                let old = orig.clone().unwrap_or_else(|| path.clone());
                let d = repo.diff_index(&path, orig.as_deref()).map_err(err)?;
                let renamed = entry.as_ref().is_some_and(|e| e.kind == EntryKind::Renamed);
                dto.patchable = !d.binary && !d.hunks.is_empty() && !renamed;
                let is_new = entry.as_ref().is_some_and(|e| e.index == 'A');
                dto.image = if d.binary { image_pair(&repo, &path, &old, Some("HEAD"), Some(""), !is_new) } else { None };
                dto.summary = summary(&repo, &path, &old, Some("HEAD"), Some(""), &d);
                dto.diff = Some(d);
            }
        }
        Ok(dto)
    })
    .await
}

fn summary(repo: &Repo, path: &str, old_path: &str, old: Option<&str>, new: Option<&str>, d: &FileDiff) -> Vec<String> {
    if !unity::is_unity_yaml(path) || d.hunks.is_empty() {
        return Vec::new();
    }
    unity::summarize(&repo.read_version(old, old_path), &repo.read_version(new, path), d)
}

#[tauri::command]
pub async fn commit_file_diff(
    state: State<'_, AppState>,
    hash: String,
    parent: Option<String>,
    path: String,
    orig: Option<String>,
    status: String,
) -> Res<DiffDto> {
    let repo = current(&state)?;
    blocking(move || {
        let d = repo.diff_commit_file(&hash, parent.as_deref(), &path, orig.as_deref()).map_err(err)?;
        let old = orig.clone().unwrap_or_else(|| path.clone());
        let mut dto = DiffDto::default();
        if d.binary {
            dto.image = image_pair(&repo, &path, &old, parent.as_deref(), Some(&hash), parent.is_some() && status != "A");
        }
        if let Some(p) = &parent {
            dto.summary = summary(&repo, &path, &old, Some(p), Some(&hash), &d);
        }
        dto.diff = Some(d);
        Ok(dto)
    })
    .await
}

// ------------------------------------------------------------------ staging & commits

#[tauri::command]
pub async fn stage(state: State<'_, AppState>, paths: Vec<String>) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.stage(&paths).map_err(err)?, "Staged")).await
}

#[tauri::command]
pub async fn unstage(state: State<'_, AppState>, paths: Vec<String>) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.unstage(&paths).map_err(err)?, "Unstaged")).await
}

#[tauri::command]
pub async fn stage_all(state: State<'_, AppState>, stage: bool) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || {
        if stage { ok(repo.stage_all().map_err(err)?, "Staged everything") } else { ok(repo.unstage_all().map_err(err)?, "Unstaged everything") }
    })
    .await
}

/// Stage, unstage or discard selected (hunk, line) pairs of a file's diff.
#[tauri::command]
pub async fn apply_lines(
    state: State<'_, AppState>,
    path: String,
    section: String,
    lines: Vec<(usize, usize)>,
    discard: bool,
) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || {
        let staged = section == "staged";
        let d = if staged { repo.diff_index(&path, None) } else { repo.diff_worktree(&path) }.map_err(err)?;
        let set: HashSet<(usize, usize)> = lines.into_iter().collect();
        let reverse = staged || discard;
        let patch = build_patch(&d, |h, l| set.contains(&(h, l)), reverse).ok_or("No changed lines selected.")?;
        if discard {
            ok(repo.discard_patch(&patch, &path).map_err(err)?, "Discarded lines — undo to bring them back")
        } else {
            let cmd = repo.apply_to_index(&patch, staged, &path).map_err(err)?;
            ok(cmd, if staged { "Unstaged lines" } else { "Staged lines" })
        }
    })
    .await
}

#[tauri::command]
pub async fn discard_file(state: State<'_, AppState>, path: String, section: String, orig: Option<String>, index_status: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || {
        let st = repo.status().map_err(err)?;
        let untracked = st.entries.iter().any(|e| e.path == path && e.kind == EntryKind::Untracked);
        let target = if section == "staged" {
            DiscardTarget::Staged { path, orig, index_status: index_status.chars().next().unwrap_or('M') }
        } else if untracked {
            DiscardTarget::Untracked { path }
        } else {
            DiscardTarget::Worktree { path }
        };
        ok(repo.discard(&[target]).map_err(err)?, "Discarded — undo to bring it back")
    })
    .await
}

#[tauri::command]
pub async fn commit(state: State<'_, AppState>, message: String, amend: bool) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.commit(&message, amend).map_err(err)?, if amend { "Amended" } else { "Committed" })).await
}

#[tauri::command]
pub async fn last_message(state: State<'_, AppState>) -> Res<String> {
    let repo = current(&state)?;
    blocking(move || repo.last_commit_message().map_err(err)).await
}

#[tauri::command]
pub async fn undo(state: State<'_, AppState>) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || {
        let desc = repo.undo_last().map_err(err)?;
        ok("gitty undo".into(), format!("Undid: {desc}"))
    })
    .await
}

#[tauri::command]
pub async fn reword(state: State<'_, AppState>, hash: String, message: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.reword_commit(&hash, &message).map_err(err)?, "Message updated")).await
}

// ------------------------------------------------------------------ history

#[derive(Serialize)]
pub struct LogRow {
    commit: Commit,
    cells: Vec<Cell>,
}

#[tauri::command]
pub async fn log(state: State<'_, AppState>, all: bool, limit: usize) -> Res<Vec<LogRow>> {
    let repo = current(&state)?;
    blocking(move || {
        let has_head = !repo.head_is_unborn();
        let mut revs = Vec::new();
        if all {
            revs.extend(["--branches", "--remotes", "--tags"].map(String::from));
        }
        if has_head {
            revs.push("HEAD".into());
        }
        let commits = repo.log(&LogQuery { revs, skip: 0, limit }).map_err(err)?;
        let mut g = GraphBuilder::new();
        Ok(commits.into_iter().map(|c| LogRow { cells: g.push(&c), commit: c }).collect())
    })
    .await
}

#[tauri::command]
pub async fn commit_detail(state: State<'_, AppState>, hash: String) -> Res<CommitDetail> {
    let repo = current(&state)?;
    blocking(move || repo.commit_detail(&hash).map_err(err)).await
}

#[tauri::command]
pub async fn commit_message(state: State<'_, AppState>, hash: String) -> Res<(String, bool)> {
    let repo = current(&state)?;
    blocking(move || {
        let msg = repo.git().run_text(&["log", "-1", "--format=%B", &hash]).map_err(err)?;
        Ok((msg.trim_end().to_string(), repo.is_pushed(&hash)))
    })
    .await
}

// ------------------------------------------------------------------ branches

#[tauri::command]
pub async fn branches(state: State<'_, AppState>) -> Res<Vec<Branch>> {
    let repo = current(&state)?;
    blocking(move || repo.branches().map_err(err)).await
}

#[tauri::command]
pub async fn switch_branch(state: State<'_, AppState>, name: String, remote: bool) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || {
        if remote {
            let local = name.split_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| name.clone());
            let exists = repo.branches().map_err(err)?.iter().any(|b| !b.remote && b.name == local);
            if exists {
                return ok(repo.switch(&local).map_err(err)?, format!("Switched to {local}"));
            }
            return ok(repo.switch_track(&name).map_err(err)?, format!("Checked out {name}"));
        }
        ok(repo.switch(&name).map_err(err)?, format!("Switched to {name}"))
    })
    .await
}

#[tauri::command]
pub async fn create_branch(state: State<'_, AppState>, name: String, start: Option<String>) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.create_branch(&name, start.as_deref()).map_err(err)?, format!("Created {name}"))).await
}

#[tauri::command]
pub async fn stash_push(state: State<'_, AppState>, message: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.stash_push(&message, true).map_err(err)?, "Stashed")).await
}

#[tauri::command]
pub async fn stash_apply(state: State<'_, AppState>, name: String, pop: bool) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.stash_apply(&name, pop).map_err(err)?, if pop { "Popped stash" } else { "Applied stash" })).await
}

// ------------------------------------------------------------------ remotes

fn emit_progress(app: &AppHandle) -> impl FnMut(&str) + '_ {
    move |line: &str| {
        let _ = app.emit("git-progress", line.to_string());
    }
}

/// `(remote, branch)` to set as upstream when the branch has none yet.
fn upstream_target(repo: &Repo) -> Res<Option<(String, String)>> {
    let st = repo.status().map_err(err)?;
    let head = st.branch.head.clone().ok_or("HEAD is detached — switch to a branch first.")?;
    if st.branch.upstream.is_some() {
        return Ok(None);
    }
    let remotes = repo.remotes().map_err(err)?;
    let r = if remotes.iter().any(|r| r == "origin") { "origin".to_string() } else { remotes.first().cloned().ok_or("No remote configured — add one first.")? };
    Ok(Some((r, head)))
}

#[tauri::command]
pub async fn remote_op(app: AppHandle, state: State<'_, AppState>, op: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || {
        match op.as_str() {
            "fetch" => ok(repo.fetch(emit_progress(&app)).map_err(err)?, "Fetched"),
            "pull" => ok(repo.pull(emit_progress(&app)).map_err(err)?, "Pulled"),
            "push" => {
                let up = upstream_target(&repo)?;
                let cmd = repo.push(up.as_ref().map(|(r, b)| (r.as_str(), b.as_str())), emit_progress(&app)).map_err(err)?;
                ok(cmd, "Pushed")
            }
            "push-force" => ok(repo.push_force_with_lease(emit_progress(&app)).map_err(err)?, "Force-pushed (with lease)"),
            // fetch, pull what's new, then push what's local — one button for the common case
            "sync" => {
                let mut cmds = vec![repo.fetch(emit_progress(&app)).map_err(err)?];
                let st = repo.status().map_err(err)?;
                if st.branch.upstream.is_some() && st.branch.behind > 0 {
                    cmds.push(repo.pull(emit_progress(&app)).map_err(err)?);
                }
                let st = repo.status().map_err(err)?;
                if st.branch.upstream.is_none() || st.branch.ahead > 0 {
                    let up = upstream_target(&repo)?;
                    cmds.push(repo.push(up.as_ref().map(|(r, b)| (r.as_str(), b.as_str())), emit_progress(&app)).map_err(err)?);
                }
                let n = cmds.len();
                ok(cmds.join(" && "), if n == 1 { "Already up to date" } else { "Synced" })
            }
            other => Err(format!("unknown operation {other}")),
        }
    })
    .await
}

// ------------------------------------------------------------------ insight

#[tauri::command]
pub async fn insight(state: State<'_, AppState>) -> Res<Insight> {
    let repo = current(&state)?;
    blocking(move || repo.insight().map_err(err)).await
}

#[tauri::command]
pub async fn largest_blobs(state: State<'_, AppState>, force: bool) -> Res<Vec<Blob>> {
    let repo = current(&state)?;
    blocking(move || repo.largest_blobs(25, !force).map_err(err)).await
}

#[tauri::command]
pub async fn apply_fix(state: State<'_, AppState>, fix: serde_json::Value) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || {
        let strings = |k: &str| -> Vec<String> {
            fix.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
        };
        if let Some(pattern) = fix.get("pattern").and_then(|p| p.as_str()) {
            return ok(repo.apply_ignore(pattern, &strings("untrack")).map_err(err)?, format!("Ignored {pattern}"));
        }
        let patterns = strings("patterns");
        if fix.get("convert").is_some() {
            return ok(repo.lfs_track(&patterns, &strings("convert")).map_err(err)?, "Tracking with LFS — commit to finish");
        }
        Err("This fix rewrites history; run it from the terminal app.".into())
    })
    .await
}

// ------------------------------------------------------------------ rebase

#[derive(serde::Deserialize)]
pub struct StepIn {
    hash: String,
    subject: String,
    action: String,
    message: Option<String>,
}

#[derive(Serialize)]
pub struct PlanDto {
    base: Option<String>,
    steps: Vec<gitty_core::rebase::RebaseStep>,
}

/// Commits from `hash` (inclusive) to HEAD, oldest first.
#[tauri::command]
pub async fn rebase_plan(state: State<'_, AppState>, hash: String) -> Res<PlanDto> {
    let repo = current(&state)?;
    blocking(move || {
        if repo.state() != RepoState::Clean {
            return Err("Finish or abort the current merge/rebase first.".into());
        }
        if !repo.git().succeeds(&["merge-base", "--is-ancestor", &hash, "HEAD"]) {
            return Err(format!("{} isn't on the current branch.", &hash[..hash.len().min(7)]));
        }
        let parents = repo.git().run_text(&["rev-list", "--parents", "-n", "1", &hash]).map_err(err)?;
        let base = parents.split_whitespace().nth(1).map(str::to_string);
        let steps = repo.rebase_plan(base.as_deref()).map_err(err)?;
        Ok(PlanDto { base, steps })
    })
    .await
}

#[tauri::command]
pub async fn run_rebase(state: State<'_, AppState>, base: Option<String>, steps: Vec<StepIn>) -> Res<OpResult> {
    use gitty_core::rebase::{RebaseAction, RebaseStep};
    let repo = current(&state)?;
    blocking(move || {
        let steps: Vec<RebaseStep> = steps
            .into_iter()
            .map(|s| RebaseStep {
                action: match s.action.as_str() {
                    "Reword" => RebaseAction::Reword,
                    "Squash" => RebaseAction::Squash,
                    "Fixup" => RebaseAction::Fixup,
                    "Drop" => RebaseAction::Drop,
                    _ => RebaseAction::Pick,
                },
                hash: s.hash,
                subject: s.subject,
                message: s.message,
                pushed: false,
            })
            .collect();
        ok(repo.run_rebase(base.as_deref(), &steps).map_err(err)?, "Rebased — undo to go back")
    })
    .await
}

// ------------------------------------------------------------------ conflicts

#[tauri::command]
pub async fn read_conflict(state: State<'_, AppState>, path: String) -> Res<gitty_core::conflict::ConflictFile> {
    let repo = current(&state)?;
    blocking(move || repo.read_conflict(&path).map_err(err)).await
}

#[tauri::command]
pub async fn resolve_region(state: State<'_, AppState>, path: String, region: usize, pick: String) -> Res<OpResult> {
    use gitty_core::conflict::{self, Pick};
    let repo = current(&state)?;
    blocking(move || {
        let pick = match pick.as_str() {
            "ours" => Pick::Ours,
            "theirs" => Pick::Theirs,
            "both" => Pick::Both,
            "base" => Pick::Base,
            other => return Err(format!("unknown side {other}")),
        };
        let cf = repo.read_conflict(&path).map_err(err)?;
        let text = conflict::resolve(&cf, region, pick).ok_or("That conflict is gone or has no original version.")?;
        let what = format!("Resolved conflict {}", region + 1);
        ok(repo.write_resolution(&path, &text, &what).map_err(err)?, what)
    })
    .await
}

#[tauri::command]
pub async fn conflict_file_op(state: State<'_, AppState>, path: String, op: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || match op.as_str() {
        "ours" => ok(repo.take_side(&path, true).map_err(err)?, "Kept our version"),
        "theirs" => ok(repo.take_side(&path, false).map_err(err)?, "Kept their version"),
        "delete" => ok(repo.resolve_delete(&path).map_err(err)?, "Deleted the file"),
        "base" => ok(repo.show_conflict_base(&path).map_err(err)?, "Showing the original between the sides"),
        other => Err(format!("unknown operation {other}")),
    })
    .await
}

#[tauri::command]
pub async fn operation(state: State<'_, AppState>, op: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || match op.as_str() {
        "continue" => ok(repo.continue_operation().map_err(err)?, "Continued"),
        "abort" => ok(repo.abort_operation().map_err(err)?, "Aborted"),
        "skip" => ok(repo.skip_operation().map_err(err)?, "Skipped the commit"),
        other => Err(format!("unknown operation {other}")),
    })
    .await
}

// ------------------------------------------------------------------ ignore rules

#[tauri::command]
pub async fn ignore_sources(state: State<'_, AppState>) -> Res<Vec<gitty_core::ignore::IgnoreSource>> {
    let repo = current(&state)?;
    blocking(move || repo.ignore_sources().map_err(err)).await
}

#[tauri::command]
pub async fn why_ignored(state: State<'_, AppState>, path: String) -> Res<Option<gitty_core::ignore::WhyIgnored>> {
    let repo = current(&state)?;
    blocking(move || repo.why_ignored(&path).map_err(err)).await
}

#[tauri::command]
pub async fn add_ignore(state: State<'_, AppState>, rel: String, pattern: String, untrack: Vec<String>) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.add_ignore_rule(&rel, &pattern, &untrack).map_err(err)?, format!("Added {pattern} to {rel}"))).await
}

#[tauri::command]
pub async fn remove_ignore(state: State<'_, AppState>, rel: String, line: usize, text: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.remove_ignore_rule(&rel, line, &text).map_err(err)?, format!("Removed {text}"))).await
}

#[tauri::command]
pub async fn read_text_file(state: State<'_, AppState>, rel: String) -> Res<String> {
    let repo = current(&state)?;
    blocking(move || Ok(std::fs::read_to_string(repo.root().join(&rel)).unwrap_or_default())).await
}

#[tauri::command]
pub async fn write_ignore_file(state: State<'_, AppState>, rel: String, text: String) -> Res<OpResult> {
    let repo = current(&state)?;
    blocking(move || ok(repo.write_ignore_file(&rel, &text).map_err(err)?, format!("Saved {rel}"))).await
}

// ------------------------------------------------------------------ clone

#[derive(Serialize)]
pub struct CloneDefaults {
    parent: String,
}

/// Where new clones go by default: next to the open repository, or the home folder.
#[tauri::command]
pub async fn clone_defaults(app: AppHandle, state: State<'_, AppState>) -> Res<CloneDefaults> {
    let from_repo = current(&state).ok().and_then(|r| r.root().parent().map(Path::to_path_buf));
    let from_recent = load_recent(&app).first().and_then(|r| Path::new(r).parent().map(Path::to_path_buf));
    let home = app.path().home_dir().ok();
    let parent = from_repo.or(from_recent).or(home).unwrap_or_else(|| PathBuf::from("."));
    Ok(CloneDefaults { parent: parent.display().to_string() })
}

#[tauri::command]
pub fn clone_name(url: String) -> String {
    workspace::clone_dir_name(&url)
}

#[tauri::command]
pub async fn clone_repo(app: AppHandle, state: State<'_, AppState>, url: String, parent: String, name: String) -> Res<RepoInfo> {
    let app2 = app.clone();
    let target = blocking(move || {
        workspace::clone_repo(&url, Path::new(&parent), name.trim(), emit_progress(&app2)).map_err(|e| {
            let text = err(e);
            if text.contains("could not read Username") || text.contains("Authentication failed") || text.contains("Permission denied") {
                format!("{text}\n\nGit couldn't sign in. For private repositories, set up a credential helper (HTTPS) or an SSH key, or clone once from the terminal.")
            } else {
                text
            }
        })
    })
    .await?;
    open(&app, &state, &target)
}

// ------------------------------------------------------------------ misc

#[tauri::command]
pub async fn tags(state: State<'_, AppState>) -> Res<Vec<(String, String, i64)>> {
    let repo = current(&state)?;
    blocking(move || repo.tags().map_err(err)).await
}

#[tauri::command]
pub async fn search_log(state: State<'_, AppState>, query: String) -> Res<Vec<Commit>> {
    let repo = current(&state)?;
    blocking(move || repo.search_log(&query, 300).map_err(err)).await
}

#[tauri::command]
pub async fn init_repo(app: AppHandle, state: State<'_, AppState>, path: String) -> Res<RepoInfo> {
    let target = blocking(move || workspace::init_repo(Path::new(&path)).map_err(err)).await?;
    open(&app, &state, &target)
}
