//! Interactive rebase driven by a plan instead of an editor, plus
//! continue/abort for any operation that stopped on conflicts.

use std::collections::HashSet;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::git::{Error, Result, display_command};
use crate::oplog::{OpEntry, OpKind, Snapshot, now};
use crate::repo::{Repo, RepoState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebaseAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl RebaseAction {
    pub fn label(self) -> &'static str {
        match self {
            RebaseAction::Pick => "pick",
            RebaseAction::Reword => "reword",
            RebaseAction::Squash => "squash",
            RebaseAction::Fixup => "fixup",
            RebaseAction::Drop => "drop",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebaseStep {
    pub hash: String,
    pub subject: String,
    pub action: RebaseAction,
    /// New message for `Reword`.
    pub message: Option<String>,
    /// Already on a remote branch, so rewriting it affects others.
    pub pushed: bool,
}

/// An operation that stopped on conflicts; recorded for undo once it finishes.
#[derive(Serialize, Deserialize)]
struct Pending {
    kind: OpKind,
    description: String,
    command: String,
    before: Snapshot,
}

/// What the branch will look like after the plan runs (oldest first).
pub fn preview(steps: &[RebaseStep]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in steps {
        match s.action {
            RebaseAction::Drop => {}
            RebaseAction::Squash | RebaseAction::Fixup => {
                if let Some(last) = out.last_mut() {
                    if !last.ends_with(" (+ squashed)") {
                        last.push_str(" (+ squashed)");
                    }
                }
            }
            RebaseAction::Reword => {
                let m = s.message.as_deref().unwrap_or(&s.subject);
                out.push(m.lines().next().unwrap_or_default().to_string());
            }
            RebaseAction::Pick => out.push(s.subject.clone()),
        }
    }
    out
}

pub fn validate(steps: &[RebaseStep]) -> Result<()> {
    match steps.iter().find(|s| s.action != RebaseAction::Drop) {
        None => Err(Error::Msg("Every commit is dropped — that would empty the branch.".into())),
        Some(s) if matches!(s.action, RebaseAction::Squash | RebaseAction::Fixup) => {
            Err(Error::Msg("The oldest kept commit can't be squashed: there is nothing below it to squash into.".into()))
        }
        _ => Ok(()),
    }
}

fn quote(path: &str) -> String {
    format!("'{}'", path.replace('\\', "/").replace('\'', "'\\''"))
}

impl Repo {
    fn pending_path(&self) -> std::path::PathBuf {
        self.git_dir().join("gitty").join("pending.json")
    }

    fn save_pending(&self, kind: OpKind, description: &str, command: &str, before: Snapshot) {
        let p = Pending { kind, description: description.into(), command: command.into(), before };
        if let Ok(text) = serde_json::to_string(&p) {
            let _ = fs::create_dir_all(self.git_dir().join("gitty"));
            let _ = fs::write(self.pending_path(), text);
        }
    }

    fn finish_pending(&self) {
        let Ok(text) = fs::read_to_string(self.pending_path()) else { return };
        let _ = fs::remove_file(self.pending_path());
        let (Ok(p), Ok(after)) = (serde_json::from_str::<Pending>(&text), self.snapshot()) else { return };
        let _ = self.oplog_push(OpEntry {
            time: now(),
            kind: p.kind,
            description: p.description,
            command: p.command,
            before: p.before,
            after,
            backups: Vec::new(),
            stash: None,
        });
    }

    /// Commits after `base` up to HEAD, oldest first. `base = None` means from the root.
    pub fn rebase_plan(&self, base: Option<&str>) -> Result<Vec<RebaseStep>> {
        let range = match base {
            Some(b) => format!("{b}..HEAD"),
            None => "HEAD".to_string(),
        };
        let out = self.git.run_text(&["log", "--reverse", "--format=%H%x1f%P%x1f%s", &range, "--"])?;
        let mut steps = Vec::new();
        for line in out.lines() {
            let f: Vec<&str> = line.splitn(3, '\x1f').collect();
            if f.len() < 3 {
                continue;
            }
            if f[1].split_whitespace().count() > 1 {
                return Err(Error::Msg(format!(
                    "{} is a merge commit. Rebasing across merges isn't supported — pick a newer starting commit.",
                    &f[0][..7]
                )));
            }
            steps.push(RebaseStep { hash: f[0].into(), subject: f[2].into(), action: RebaseAction::Pick, message: None, pushed: false });
        }
        if steps.is_empty() {
            return Err(Error::Msg("No commits to rebase here.".into()));
        }
        // Commits reachable from any remote branch are already shared.
        let mut args = vec!["rev-list", "--remotes"];
        let exclude = base.map(|b| format!("^{b}"));
        if let Some(e) = &exclude {
            args.push(e);
        }
        let on_remote: HashSet<String> = self.git.run_text(&args).unwrap_or_default().lines().map(str::to_string).collect();
        for s in &mut steps {
            s.pushed = on_remote.contains(&s.hash);
        }
        Ok(steps)
    }

    /// Run the plan. If it stops on a conflict the error says so and the repo
    /// is left in REBASING state; finish with `continue_operation`.
    pub fn run_rebase(&self, base: Option<&str>, steps: &[RebaseStep]) -> Result<String> {
        validate(steps)?;
        let dir = self.git_dir().join("gitty").join("rebase");
        fs::create_dir_all(&dir)?;
        let mut todo = String::new();
        for (i, s) in steps.iter().enumerate() {
            let subject = s.subject.replace('\n', " ");
            match s.action {
                RebaseAction::Reword => {
                    let msg_path = dir.join(format!("msg-{i}.txt"));
                    fs::write(&msg_path, s.message.as_deref().unwrap_or(&s.subject))?;
                    todo.push_str(&format!("pick {} {subject}\n", s.hash));
                    todo.push_str(&format!(
                        "exec git commit --amend --only --no-verify --allow-empty -F {}\n",
                        quote(&msg_path.to_string_lossy())
                    ));
                }
                a => todo.push_str(&format!("{} {} {subject}\n", a.label(), s.hash)),
            }
        }
        let todo_path = dir.join("todo");
        fs::write(&todo_path, &todo)?;

        let before = self.snapshot()?;
        let mut args = vec!["rebase", "-i", "--autostash"];
        let base_arg;
        match base {
            Some(b) => {
                base_arg = b.to_string();
                args.push(&base_arg);
            }
            None => args.push("--root"),
        }
        let kept = steps.iter().filter(|s| s.action != RebaseAction::Drop).count();
        let description = format!("Rebase {} commits", steps.len());
        let shown = display_command(&args);
        let out = self
            .git
            .command(&args)
            .env("GIT_SEQUENCE_EDITOR", format!("cp {}", quote(&todo_path.to_string_lossy())))
            .output()?;
        if out.status.success() {
            let after = self.snapshot()?;
            let _ = self.oplog_push(OpEntry {
                time: now(),
                kind: OpKind::Rebase,
                description,
                command: shown.clone(),
                before,
                after,
                backups: Vec::new(),
                stash: None,
            });
            return Ok(format!("{shown}  ({kept} commits kept)"));
        }
        let err = format!("{}\n{}", String::from_utf8_lossy(&out.stderr).trim(), String::from_utf8_lossy(&out.stdout).trim());
        if self.state() == RepoState::Rebasing {
            self.save_pending(OpKind::Rebase, &description, &shown, before);
            return Err(Error::Msg(format!(
                "The rebase stopped on a conflict.\nResolve the files in Changes, stage them, then press m to continue (or abort).\n\n{}",
                err.trim()
            )));
        }
        Err(Error::Command { command: shown, stderr: err.trim().to_string() })
    }

    /// Pull, remembering the start point if it stops on conflicts.
    pub fn pull_tracked(&self, on_line: impl FnMut(&str)) -> Result<String> {
        let before = self.snapshot()?;
        let args = ["pull", "--progress"];
        let shown = display_command(&args);
        match self.git.run_streaming(&args, on_line) {
            Ok(_) => {
                if let Ok(after) = self.snapshot() {
                    if after != before {
                        let _ = self.oplog_push(OpEntry {
                            time: now(),
                            kind: OpKind::Pull,
                            description: "Pull".into(),
                            command: shown.clone(),
                            before,
                            after,
                            backups: Vec::new(),
                            stash: None,
                        });
                    }
                }
                Ok(shown)
            }
            Err(e) => {
                if matches!(self.state(), RepoState::Merging | RepoState::Rebasing) {
                    self.save_pending(OpKind::Pull, "Pull", &shown, before);
                }
                Err(e)
            }
        }
    }

    pub fn continue_operation(&self) -> Result<String> {
        let args: &[&str] = match self.state() {
            RepoState::Rebasing => &["rebase", "--continue"],
            RepoState::Merging => &["commit", "--no-edit"],
            RepoState::CherryPicking => &["cherry-pick", "--continue"],
            RepoState::Reverting => &["revert", "--continue"],
            _ => return Err(Error::Msg("Nothing in progress to continue.".into())),
        };
        self.git.run(args)?;
        if self.state() == RepoState::Clean {
            self.finish_pending();
        }
        Ok(display_command(args))
    }

    pub fn abort_operation(&self) -> Result<String> {
        let args: &[&str] = match self.state() {
            RepoState::Rebasing => &["rebase", "--abort"],
            RepoState::Merging => &["merge", "--abort"],
            RepoState::CherryPicking => &["cherry-pick", "--abort"],
            RepoState::Reverting => &["revert", "--abort"],
            RepoState::Bisecting => &["bisect", "reset"],
            RepoState::Clean => return Err(Error::Msg("Nothing in progress to abort.".into())),
        };
        self.git.run(args)?;
        let _ = fs::remove_file(self.pending_path());
        Ok(display_command(args))
    }

    pub fn skip_operation(&self) -> Result<String> {
        if self.state() != RepoState::Rebasing {
            return Err(Error::Msg("Only a rebase can skip a commit.".into()));
        }
        self.git.run(&["rebase", "--skip"])?;
        if self.state() == RepoState::Clean {
            self.finish_pending();
        }
        Ok("git rebase --skip".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(s: &str, a: RebaseAction) -> RebaseStep {
        RebaseStep { hash: s.into(), subject: s.into(), action: a, message: None, pushed: false }
    }

    #[test]
    fn preview_and_validate() {
        use RebaseAction::*;
        let steps = vec![step("a", Pick), step("b", Fixup), step("c", Drop), step("d", Pick)];
        assert_eq!(preview(&steps), vec!["a (+ squashed)", "d"]);
        assert!(validate(&steps).is_ok());
        assert!(validate(&[step("a", Squash)]).is_err());
        assert!(validate(&[step("a", Drop), step("b", Fixup)]).is_err());
        assert!(validate(&[step("a", Drop)]).is_err());
    }
}
