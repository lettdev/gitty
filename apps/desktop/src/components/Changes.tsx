import { useEffect, useMemo, useState } from "react";
import type { DiffDto, OpResult, Section, StatusDto, StatusEntry } from "../api";
import { api, splitPath } from "../api";
import { DiffView } from "./DiffView";

export interface Ctx {
  run: (op: () => Promise<OpResult>) => Promise<boolean>;
  ask: (title: string, message: string, confirm: string) => Promise<boolean>;
}

interface Row {
  section: Section;
  entry: StatusEntry;
}

function letter(r: Row): string {
  const e = r.entry;
  if (r.section === "conflicts") return "C";
  if (e.kind === "Untracked") return "U";
  return r.section === "staged" ? e.index : e.worktree;
}

export function Changes({ st, ctx, focusCommit }: { st: StatusDto; ctx: Ctx; focusCommit: number }) {
  const rows = useMemo(() => {
    const es = st.status.entries;
    const conflicts = es.filter((e) => e.kind === "Unmerged").map((entry) => ({ section: "conflicts" as const, entry }));
    const unstaged = es
      .filter((e) => e.kind === "Untracked" || (e.kind !== "Unmerged" && e.worktree !== "."))
      .map((entry) => ({ section: "unstaged" as const, entry }));
    const staged = es
      .filter((e) => e.kind !== "Untracked" && e.kind !== "Unmerged" && e.index !== ".")
      .map((entry) => ({ section: "staged" as const, entry }));
    return { conflicts, unstaged, staged };
  }, [st]);
  const all = [...rows.conflicts, ...rows.unstaged, ...rows.staged];
  const [sel, setSel] = useState<{ section: Section; path: string } | null>(null);
  const current = all.find((r) => r.section === sel?.section && r.entry.path === sel?.path) ?? all[0] ?? null;

  const [dto, setDto] = useState<DiffDto | null>(null);
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    if (!current) {
      setDto(null);
      return;
    }
    let live = true;
    setLoading(true);
    api
      .fileDiff(current.entry.path, current.section, current.entry.orig_path)
      .then((d) => live && setDto(d))
      .catch((e) => live && setDto({ diff: null, summary: [], image: null, note: String(e), raw: null, patchable: false }))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
    // reload when the status changes, so edits on disk show up
  }, [current?.entry.path, current?.section, st.fingerprint, JSON.stringify(current?.entry)]);

  const toggle = (r: Row) =>
    ctx.run(() => {
      if (r.section === "staged") return api.unstage(r.entry.orig_path ? [r.entry.orig_path, r.entry.path] : [r.entry.path]);
      return api.stage([r.entry.path]);
    });

  const discard = async (r: Row) => {
    const what =
      r.entry.kind === "Untracked" ? `Delete the untracked ${r.entry.path}?` : r.section === "staged" ? `Reset ${r.entry.path} to the last commit?` : `Discard unstaged changes in ${r.entry.path}?`;
    if (await ctx.ask("Discard changes", `${what}\nA backup is kept — Undo brings it back.`, "Discard")) {
      ctx.run(() => api.discardFile(r.entry, r.section));
    }
  };

  // keyboard: j/k to move, space to stage
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest("input, textarea")) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const i = current ? all.indexOf(current) : -1;
      if (e.key === "j" || e.key === "ArrowDown") {
        const n = all[Math.min(all.length - 1, i + 1)];
        if (n) setSel({ section: n.section, path: n.entry.path });
      } else if (e.key === "k" || e.key === "ArrowUp") {
        const n = all[Math.max(0, i - 1)];
        if (n) setSel({ section: n.section, path: n.entry.path });
      } else if (e.key === " " && current) {
        e.preventDefault();
        toggle(current);
      } else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const group = (title: string, list: Row[], action?: { label: string; fn: () => void }) =>
    list.length > 0 && (
      <>
        <div className="gh">
          <span>
            {title} · {list.length}
          </span>
          {action && <button onClick={action.fn}>{action.label}</button>}
        </div>
        {list.map((r) => {
          const [name, dir] = splitPath(r.entry.path);
          const isSel = current === r;
          const l = letter(r);
          return (
            <div
              key={r.section + r.entry.path}
              className={`fi ${isSel ? "sel" : ""}`}
              onClick={() => setSel({ section: r.section, path: r.entry.path })}
              onContextMenu={(e) => {
                e.preventDefault();
                discard(r);
              }}
              title="Right-click to discard"
            >
              <button
                className={`cb ${r.section === "staged" ? "on" : ""}`}
                aria-label={r.section === "staged" ? `Unstage ${name}` : `Stage ${name}`}
                onClick={(e) => {
                  e.stopPropagation();
                  toggle(r);
                }}
              />
              <span className={`st st-${l}`}>{l}</span>
              <span className="fn">
                <b>
                  {name}
                  {r.entry.orig_path && <span className="muted"> ← {r.entry.orig_path}</span>}
                </b>
                <span>{dir}</span>
              </span>
            </div>
          );
        })}
      </>
    );

  const verb = current?.section === "staged" ? "Unstage" : "Stage";
  return (
    <div className="split" style={{ gridTemplateColumns: "320px 1fr" }}>
      <div className="panel">
        <div className="scroll">
          {all.length === 0 && (
            <div className="empty">
              <b>Working tree clean</b>Nothing to commit. Edit some files and they show up here.
            </div>
          )}
          {group("Conflicts", rows.conflicts)}
          {group("Unstaged", rows.unstaged, { label: "Stage all", fn: () => ctx.run(() => api.stageAll(true)) })}
          {group("Staged", rows.staged, { label: "Unstage all", fn: () => ctx.run(() => api.stageAll(false)) })}
        </div>
        <CommitBox st={st} ctx={ctx} focus={focusCommit} />
      </div>
      <DiffView
        title={current ? splitPath(current.entry.path)[0] : "Diff"}
        sub={current ? splitPath(current.entry.path)[1] : undefined}
        dto={dto}
        loading={loading}
        actions={
          current && current.section !== "conflicts"
            ? {
                verb,
                onApply: (lines) => ctx.run(() => api.applyLines(current.entry.path, current.section, lines, false)),
                onDiscard:
                  current.section === "unstaged"
                    ? async (lines) => {
                        if (await ctx.ask("Discard lines", `Discard ${lines.length} changed line(s)?\nUndo brings them back.`, "Discard")) {
                          ctx.run(() => api.applyLines(current.entry.path, "unstaged", lines, true));
                        }
                      }
                    : undefined,
              }
            : undefined
        }
      />
    </div>
  );
}

function CommitBox({ st, ctx, focus }: { st: StatusDto; ctx: Ctx; focus: number }) {
  const [summary, setSummary] = useState("");
  const [body, setBody] = useState("");
  const [amend, setAmend] = useState(false);
  const staged = st.status.entries.filter((e) => e.kind !== "Untracked" && e.kind !== "Unmerged" && e.index !== ".").length;
  const branch = st.status.branch.head ?? "HEAD";
  useEffect(() => {
    if (focus > 0) document.getElementById("commit-summary")?.focus();
  }, [focus]);
  useEffect(() => {
    if (!amend) return;
    api.lastMessage().then((m) => {
      const [s, ...rest] = m.split("\n");
      setSummary(s);
      setBody(rest.join("\n").trim());
    });
  }, [amend]);
  const canCommit = summary.trim().length > 0 && (staged > 0 || amend || st.state === "Merging");
  const submit = async () => {
    if (!canCommit) return;
    const message = body.trim() ? `${summary.trim()}\n\n${body.trim()}` : summary.trim();
    if (await ctx.run(() => api.commit(message, amend))) {
      setSummary("");
      setBody("");
      setAmend(false);
    }
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    }
  };
  return (
    <div className="commitbox">
      <input
        id="commit-summary"
        placeholder="Summary"
        value={summary}
        onChange={(e) => setSummary(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            submit();
          }
        }}
      />
      <textarea placeholder="Description (optional)" value={body} onChange={(e) => setBody(e.target.value)} onKeyDown={onKey} />
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={amend} onChange={(e) => setAmend(e.target.checked)} /> Amend
        </label>
        <span className="spacer" />
        <span className={summary.length > 72 ? "mod" : "muted"} style={{ fontSize: 11 }}>
          {summary.length}/72
        </span>
      </div>
      <button className="btn primary" disabled={!canCommit} onClick={submit}>
        {amend ? "Amend last commit" : `Commit ${staged} file${staged === 1 ? "" : "s"} to ${branch}`}
      </button>
    </div>
  );
}
