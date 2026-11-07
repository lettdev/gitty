import { useEffect, useMemo, useState } from "react";
import type { DiffDto, OpResult, Section, StatusDto, StatusEntry } from "../api";
import { api, splitPath } from "../api";
import { Conflict, OperationBanner } from "./Conflict";
import { DiffView } from "./DiffView";
import { ChevronDown, ChevronRight, CommitIcon, ListIcon, TreeIcon } from "./Icons";

export interface Ctx {
  run: (op: () => Promise<OpResult>) => Promise<boolean>;
  ask: (title: string, message: string, confirm: string) => Promise<boolean>;
  /** Open the ignore rules dialog, optionally answering "why is this path ignored?". */
  openIgnore: (checkPath?: string) => void;
}

interface IgnoreChoice {
  pattern: string;
  label: string;
  untrack: string[];
}

/** Ways to ignore a path, most specific first (mirrors the terminal app). */
function ignoreChoices(path: string, tracked: boolean): IgnoreChoice[] {
  const trimmed = path.replace(/\/$/, "");
  const i = trimmed.lastIndexOf("/");
  const dir = i >= 0 ? trimmed.slice(0, i) : null;
  const name = i >= 0 ? trimmed.slice(i + 1) : trimmed;
  const un = (spec: string) => (tracked ? [spec] : []);
  if (path.endsWith("/")) {
    return [
      { pattern: `/${trimmed}/`, label: "This folder", untrack: un(trimmed) },
      { pattern: `${name}/`, label: `Every folder named ${name}`, untrack: un(`:(glob)**/${name}`) },
    ];
  }
  const out: IgnoreChoice[] = [{ pattern: `/${trimmed}`, label: "Only this file", untrack: un(trimmed) }];
  const dot = name.lastIndexOf(".");
  if (dot > 0) out.push({ pattern: `*${name.slice(dot)}`, label: `Every ${name.slice(dot)} file`, untrack: un(`:(glob)**/*${name.slice(dot)}`) });
  if (dir) out.push({ pattern: `/${dir}/`, label: `The whole ${dir}/ folder`, untrack: un(dir) });
  out.push({ pattern: name, label: `Every file named ${name}`, untrack: un(`:(glob)**/${name}`) });
  return out;
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

  const [menu, setMenu] = useState<{ x: number; y: number; row: Row } | null>(null);
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);
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

  const [asTree, setAsTreeState] = useState<boolean>(() => {
    try {
      return localStorage.getItem("gitty.filesTree") !== "0";
    } catch {
      return true;
    }
  });
  const setAsTree = (v: boolean) => {
    setAsTreeState(v);
    try {
      localStorage.setItem("gitty.filesTree", v ? "1" : "0");
    } catch {
      /* not persisted */
    }
  };
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  const fileRow = (r: Row, inTree: boolean) => {
    const [name, dir] = splitPath(r.entry.path);
    const isSel = current === r;
    const l = letter(r);
    return (
      <div
        key={r.section + r.entry.path}
        className={`fi ${isSel ? "sel" : ""} ${inTree && dir ? "ind" : ""}`}
        onClick={() => setSel({ section: r.section, path: r.entry.path })}
        onContextMenu={(e) => {
          e.preventDefault();
          setSel({ section: r.section, path: r.entry.path });
          setMenu({ x: Math.min(e.clientX, window.innerWidth - 300), y: Math.min(e.clientY, window.innerHeight - 320), row: r });
        }}
        title={r.entry.path}
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
          {name}
          {r.entry.orig_path && <span className="d"> ← {r.entry.orig_path}</span>}
          {(!inTree || !dir) && dir && <span className="d">{dir}</span>}
        </span>
      </div>
    );
  };

  const group = (title: string, list: Row[], action?: { label: string; fn: () => void }) => {
    if (list.length === 0) return null;
    const section = list[0].section;
    let body: React.ReactNode;
    if (!asTree || section === "conflicts") {
      body = list.map((r) => fileRow(r, false));
    } else {
      // compact folders: one row per directory, files under it
      const byDir = new Map<string, Row[]>();
      for (const r of list) {
        const dir = splitPath(r.entry.path)[1];
        byDir.set(dir, [...(byDir.get(dir) ?? []), r]);
      }
      const dirs = [...byDir.keys()].sort((a, b) => (a === "" ? -1 : b === "" ? 1 : a.localeCompare(b)));
      body = dirs.map((dir) => {
        const rs = byDir.get(dir)!;
        if (!dir) return rs.map((r) => fileRow(r, true));
        const key = section + ":" + dir;
        const open = !collapsed.has(key);
        const paths = rs.flatMap((r) => (r.entry.orig_path && section === "staged" ? [r.entry.orig_path, r.entry.path] : [r.entry.path]));
        return (
          <div key={key}>
            <div
              className="fi dir"
              onClick={() => {
                const next = new Set(collapsed);
                if (open) next.add(key);
                else next.delete(key);
                setCollapsed(next);
              }}
            >
              <button
                className={`cb ${section === "staged" ? "on" : ""}`}
                aria-label={section === "staged" ? `Unstage ${dir}` : `Stage ${dir}`}
                title={section === "staged" ? "Unstage this folder" : "Stage this folder"}
                onClick={(e) => {
                  e.stopPropagation();
                  ctx.run(() => (section === "staged" ? api.unstage(paths) : api.stage(paths)));
                }}
              />
              <span className="tw">{open ? <ChevronDown size={9} /> : <ChevronRight size={9} />}</span>
              <span className="fn">{dir}</span>
              <span className="d">{rs.length}</span>
            </div>
            {open && rs.map((r) => fileRow(r, true))}
          </div>
        );
      });
    }
    return (
      <>
        <div className="gh">
          <span>
            {title} · {list.length}
          </span>
          {action && <button onClick={action.fn}>{action.label}</button>}
        </div>
        {body}
      </>
    );
  };

  const verb = current?.section === "staged" ? "Unstage" : "Stage";
  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
    {st.state_label && <OperationBanner label={st.state_label} conflicts={rows.conflicts.length} ctx={ctx} />}
    <div className="split" style={{ gridTemplateColumns: "320px 1fr", flex: 1 }}>
      <div className="panel">
        <div className="files-h">
          <span className="t">{all.length === 0 ? "No changes" : `${all.length} change${all.length === 1 ? "" : "s"}`}</span>
          <div className="mini-seg" role="group" aria-label="File list layout">
            <button className={asTree ? "on" : ""} onClick={() => setAsTree(true)} title="Group by folder"><TreeIcon /></button>
            <button className={!asTree ? "on" : ""} onClick={() => setAsTree(false)} title="Flat list"><ListIcon /></button>
          </div>
        </div>
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
      {menu && (
        <div className="ctxmenu" style={{ left: menu.x, top: menu.y }} onClick={(e) => e.stopPropagation()} role="menu">
          {menu.row.section !== "conflicts" && (
            <button onClick={() => { setMenu(null); toggle(menu.row); }}>
              {menu.row.section === "staged" ? "Unstage" : "Stage"} <code>space</code>
            </button>
          )}
          {menu.row.section !== "conflicts" && (
            <button className="danger" onClick={() => { setMenu(null); discard(menu.row); }}>Discard changes…</button>
          )}
          <hr />
          <div className="label">Ignore</div>
          {ignoreChoices(menu.row.entry.path, menu.row.entry.kind !== "Untracked").map((c) => (
            <button
              key={c.pattern}
              onClick={() => {
                setMenu(null);
                ctx.run(() => api.addIgnore(".gitignore", c.pattern, c.untrack));
              }}
            >
              {c.label} <code>{c.pattern}</code>
            </button>
          ))}
          <button onClick={() => { setMenu(null); ctx.openIgnore(menu.row.entry.path); }}>Ignore rules…</button>
          <hr />
          <button onClick={() => { setMenu(null); navigator.clipboard?.writeText(menu.row.entry.path).catch(() => {}); }}>Copy path</button>
        </div>
      )}
      {current?.section === "conflicts" ? (
        <Conflict entry={current.entry} ctx={ctx} fingerprint={st.fingerprint + JSON.stringify(current.entry)} />
      ) : (
      <DiffView
        title={current ? splitPath(current.entry.path)[0] : "Diff"}
        sub={current ? splitPath(current.entry.path)[1] : undefined}
        path={current?.entry.path}
        dto={dto}
        loading={loading}
        actions={
          current
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
      )}
    </div>
    </div>
  );
}

function CommitBox({ st, ctx, focus }: { st: StatusDto; ctx: Ctx; focus: number }) {
  const [summary, setSummary] = useState("");
  const [body, setBody] = useState("");
  const [amend, setAmend] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const staged = st.status.entries.filter((e) => e.kind !== "Untracked" && e.kind !== "Unmerged" && e.index !== ".").length;
  const branch = st.status.branch.head ?? "HEAD";
  useEffect(() => {
    if (focus > 0) setExpanded(true);
  }, [focus]);
  useEffect(() => {
    if (expanded) document.getElementById("commit-summary")?.focus();
  }, [expanded, focus]);
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
      setExpanded(false);
    }
  };
  const collapseIfEmpty = () => {
    if (!summary.trim() && !body.trim() && !amend) setExpanded(false);
  };
  if (!expanded) {
    return (
      <div className="commitbox">
        <button className="commit-collapsed" onClick={() => setExpanded(true)}>
          <CommitIcon className="ico" />
          <span>{staged > 0 ? `Write a message to commit ${staged} file${staged === 1 ? "" : "s"}…` : "Stage files, then write a commit message…"}</span>
          <span className="kbd">⌘↵</span>
        </button>
      </div>
    );
  }
  return (
    <div
      className="commitbox"
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) collapseIfEmpty();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          (document.activeElement as HTMLElement)?.blur();
          collapseIfEmpty();
        }
      }}
    >
      <div className="commit-card">
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
        <textarea
          placeholder="Description (optional)"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              submit();
            }
          }}
        />
      </div>
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
        <span className="kbd on-primary">⌘↵</span>
      </button>
    </div>
  );
}
