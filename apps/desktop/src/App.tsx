import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Branch, OpResult, RepoInfo, RepoSummary, StatusDto } from "./api";
import { api, errorText, relativeTime } from "./api";
import { Changes, type Ctx } from "./components/Changes";
import { History } from "./components/History";
import { Insight } from "./components/Insight";

type View = "changes" | "history" | "insight";
interface Ask {
  title: string;
  message: string;
  confirm: string;
  resolve: (ok: boolean) => void;
}
interface Prompt {
  title: string;
  value: string;
  body?: string;
  multiline?: boolean;
  warn?: string;
  confirm: string;
  resolve: (value: string | null) => void;
}
interface PaletteItem {
  label: string;
  cmd: string;
  run: () => void;
}

export default function App() {
  const [repo, setRepo] = useState<RepoInfo | null | undefined>(undefined);
  const [st, setSt] = useState<StatusDto | null>(null);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [view, setView] = useState<View>("changes");
  const [busy, setBusy] = useState(0);
  const [progress, setProgress] = useState("");
  const [last, setLast] = useState("");
  const [toast, setToast] = useState<{ text: string; err: boolean } | null>(null);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [palette, setPalette] = useState(false);
  const [commitFocus, setCommitFocus] = useState(0);
  const [gotoHash, setGotoHash] = useState<string | null>(null);
  const toastTimer = useRef<number>(0);

  const notify = useCallback((text: string, err = false) => {
    setToast({ text, err });
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), err ? 8000 : 3000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const s = await api.status();
      setSt((old) => (old && JSON.stringify(old) === JSON.stringify(s) ? old : s));
    } catch (e) {
      notify(errorText(e), true);
    }
  }, [notify]);

  useEffect(() => {
    api.initialRepo().then(setRepo).catch(() => setRepo(null));
    api.startView().then((v) => v && ["changes", "history", "insight"].includes(v) && setView(v as View));
    const un = listen<string>("git-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!repo) return;
    setSt(null);
    refresh();
    const t = window.setInterval(() => document.visibilityState === "visible" && refresh(), 2500);
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearInterval(t);
      window.removeEventListener("focus", onFocus);
    };
  }, [repo, refresh]);

  useEffect(() => {
    if (repo) api.branches().then(setBranches).catch(() => setBranches([]));
  }, [repo, st?.fingerprint]);

  const run = useCallback(
    async (op: () => Promise<OpResult>) => {
      setBusy((b) => b + 1);
      try {
        const r = await op();
        setLast(r.command);
        notify(r.message);
        return true;
      } catch (e) {
        notify(errorText(e), true);
        return false;
      } finally {
        setBusy((b) => b - 1);
        setProgress("");
        refresh();
      }
    },
    [notify, refresh],
  );
  const askFn = useCallback(
    (title: string, message: string, confirm: string) => new Promise<boolean>((resolve) => setAsk({ title, message, confirm, resolve })),
    [],
  );
  const ctx: Ctx = useMemo(() => ({ run, ask: askFn }), [run, askFn]);
  const promptFn = (p: Omit<Prompt, "resolve">) => new Promise<string | null>((resolve) => setPrompt({ ...p, resolve }));

  const openFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Open a git repository" });
    if (typeof dir === "string") openRepo(dir);
  };
  const openRepo = async (path: string) => {
    try {
      setRepo(await api.openRepo(path));
      setView("changes");
    } catch (e) {
      notify(errorText(e), true);
    }
  };
  const reword = async (hash: string) => {
    try {
      const [msg, pushed] = await api.commitMessage(hash);
      const value = await promptFn({
        title: `Edit message of ${hash.slice(0, 7)}`,
        value: msg,
        multiline: true,
        warn: pushed ? "Already pushed — you'll need to force-push afterwards." : undefined,
        body: "Only the message changes; files stay as they are. Undo reverts it.",
        confirm: "Save message",
      });
      if (value && value.trim() && value.trim() !== msg.trim()) run(() => api.reword(hash, value.trim()));
    } catch (e) {
      notify(errorText(e), true);
    }
  };
  const newBranch = async () => {
    const name = await promptFn({ title: "New branch from HEAD", value: "", confirm: "Create and switch" });
    if (name?.trim()) run(() => api.createBranch(name.trim(), null));
  };
  const stash = async () => {
    const msg = await promptFn({ title: "Stash all changes (new files included)", value: "", body: "Optional message.", confirm: "Stash" });
    if (msg !== null) run(() => api.stashPush(msg.trim()));
  };

  const paletteItems: PaletteItem[] = useMemo(() => {
    const items: PaletteItem[] = [
      { label: "Go to Changes", cmd: "⌘1", run: () => setView("changes") },
      { label: "Go to History", cmd: "⌘2", run: () => setView("history") },
      { label: "Go to Insight", cmd: "⌘3", run: () => setView("insight") },
      { label: "Commit staged changes…", cmd: "git commit", run: () => { setView("changes"); setCommitFocus((n) => n + 1); } },
      { label: "Stage all changes", cmd: "git add -A", run: () => run(() => api.stageAll(true)) },
      { label: "Unstage everything", cmd: "git reset", run: () => run(() => api.stageAll(false)) },
      { label: "Undo last operation", cmd: st?.last_op ?? "nothing to undo", run: () => run(api.undo) },
      { label: "Fetch all remotes", cmd: "git fetch --all --prune", run: () => run(() => api.remote("fetch")) },
      { label: "Pull", cmd: "git pull", run: () => run(() => api.remote("pull")) },
      { label: "Push", cmd: "git push", run: () => run(() => api.remote("push")) },
      { label: "Stash changes…", cmd: "git stash push -u", run: stash },
      { label: "New branch…", cmd: "git switch -c <name>", run: newBranch },
      { label: "Open repository…", cmd: "", run: openFolder },
    ];
    for (const b of branches.filter((b) => !b.head)) {
      items.push({ label: `Switch to ${b.name}`, cmd: b.remote ? `git switch --track ${b.name}` : `git switch ${b.name}`, run: () => run(() => api.switchBranch(b.name, b.remote)) });
    }
    for (const s of st?.stashes ?? []) {
      items.push({ label: `Pop ${s.name}: ${s.message}`, cmd: `git stash pop ${s.name}`, run: () => run(() => api.stashApply(s.name, true)) });
    }
    return items;
  }, [branches, st, run]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const typing = !!(e.target as HTMLElement).closest("input, textarea");
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      } else if (mod && ["1", "2", "3"].includes(e.key)) {
        e.preventDefault();
        setView((["changes", "history", "insight"] as View[])[Number(e.key) - 1]);
      } else if (mod && e.key.toLowerCase() === "z" && !typing && !e.shiftKey) {
        e.preventDefault();
        if (st?.last_op) run(api.undo);
      } else if (e.key === "Escape") {
        setPalette(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [st, run]);

  if (repo === undefined) return <div className="welcome"><span className="spin" /></div>;
  if (repo === null) return <Welcome onOpen={openRepo} onBrowse={openFolder} />;

  const b = st?.status.branch;
  const changes = st?.status.entries.length ?? 0;
  return (
    <div className="app">
      <div className="body">
        <Sidebar repo={repo} st={st} branches={branches} onOpenRepo={openRepo} onBrowse={openFolder} run={run} onNewBranch={newBranch} />
        <div className="main">
          <div className="toolbar">
            <button className={`tab ${view === "changes" ? "on" : ""}`} onClick={() => setView("changes")}>
              Changes {changes > 0 && <span className="count">{changes}</span>}
            </button>
            <button className={`tab ${view === "history" ? "on" : ""}`} onClick={() => setView("history")}>History</button>
            <button className={`tab ${view === "insight" ? "on" : ""}`} onClick={() => setView("insight")}>Insight</button>
            <span className="spacer" />
            {st?.state_label && <span className="state">{st.state_label}</span>}
            {b && (
              <span className="pill">
                ⎇ {b.head ?? `detached ${b.oid?.slice(0, 7) ?? ""}`}
                {b.upstream ? <>{b.ahead > 0 && <span className="add">↑{b.ahead}</span>}{b.behind > 0 && <span className="del">↓{b.behind}</span>}</> : b.oid && <span className="muted">not pushed</span>}
              </span>
            )}
            <button className="search" onClick={() => setPalette(true)}>
              <span style={{ flex: 1, textAlign: "left" }}>Run a command…</span>
              <span className="kbd">⌘K</span>
            </button>
            <button className="btn" onClick={() => run(() => api.remote("fetch"))}>Fetch</button>
            <button className="btn" onClick={() => run(() => api.remote("pull"))} disabled={!b?.upstream}>Pull</button>
            <button className="btn" onClick={() => run(() => api.remote("push"))}>Push</button>
          </div>
          <div className="content">
            {!st ? (
              <div className="empty"><span className="spin" /> Loading {repo.name}…</div>
            ) : view === "changes" ? (
              <Changes st={st} ctx={ctx} focusCommit={commitFocus} />
            ) : view === "history" ? (
              <History key={gotoHash ?? "h"} fingerprint={st.fingerprint} onReword={reword} select={gotoHash} />
            ) : (
              <Insight ctx={ctx} fingerprint={st.fingerprint} onGotoCommit={(h) => { setGotoHash(h); setView("history"); }} />
            )}
          </div>
        </div>
      </div>
      <div className="sbar">
        {busy > 0 ? <><span className="spin" /><span className="last">{progress || "Working…"}</span></> : <span className="last">{last && <>› <b>{last}</b></>}</span>}
        <span>Every action shows the git command it ran</span>
        <button className="btn small" disabled={!st?.last_op || busy > 0} onClick={() => run(api.undo)} title={st?.last_op ? `Undo: ${st.last_op}` : "Nothing to undo"}>
          ⌘Z Undo
        </button>
      </div>
      {palette && <Palette items={paletteItems} onClose={() => setPalette(false)} />}
      {ask && (
        <div className="overlay" onClick={() => { ask.resolve(false); setAsk(null); }}>
          <div className="dialog" onClick={(e) => e.stopPropagation()} role="dialog">
            <div className="body2"><h3>{ask.title}</h3><p>{ask.message}</p></div>
            <div className="actions">
              <button className="btn" onClick={() => { ask.resolve(false); setAsk(null); }}>Cancel</button>
              <button className="btn primary" autoFocus onClick={() => { ask.resolve(true); setAsk(null); }}>{ask.confirm}</button>
            </div>
          </div>
        </div>
      )}
      {prompt && <PromptDialog p={prompt} onDone={(v) => { prompt.resolve(v); setPrompt(null); }} />}
      {toast && <div className={`toast ${toast.err ? "err" : ""}`}>{toast.text}</div>}
    </div>
  );
}

function Sidebar({ repo, st, branches, onOpenRepo, onBrowse, run, onNewBranch }: {
  repo: RepoInfo;
  st: StatusDto | null;
  branches: Branch[];
  onOpenRepo: (p: string) => void;
  onBrowse: () => void;
  run: (op: () => Promise<OpResult>) => Promise<boolean>;
  onNewBranch: () => void;
}) {
  const [repos, setRepos] = useState<RepoSummary[]>([]);
  useEffect(() => {
    api.repoList().then(setRepos).catch(() => setRepos([]));
  }, [repo.root]);
  const local = branches.filter((b) => !b.remote);
  const remote = branches.filter((b) => b.remote);
  return (
    <aside className="side">
      <div>
        <h3>Repositories <button onClick={onBrowse}>Open…</button></h3>
        {repos.slice(0, 12).map((r) => (
          <button key={r.path} className={`si ${r.path === repo.root ? "on" : ""}`} onClick={() => onOpenRepo(r.path)} title={r.path}>
            <span className={`dot ${r.conflicts ? "conflict" : r.changes ? "dirty" : "clean"}`} />
            <span className="grow">{r.name}</span>
            {r.changes > 0 && <span className="sm">{r.changes}</span>}
          </button>
        ))}
      </div>
      <div>
        <h3>Branches <button onClick={onNewBranch}>New</button></h3>
        {local.map((b) => (
          <button key={b.full} className={`si ${b.head ? "on" : ""}`} onDoubleClick={() => !b.head && run(() => api.switchBranch(b.name, false))} title="Double-click to switch">
            <span className="grow">⎇ {b.name}</span>
            {b.ahead > 0 && <span className="sm add">↑{b.ahead}</span>}
            {b.behind > 0 && <span className="sm del">↓{b.behind}</span>}
            {!b.ahead && !b.behind && <span className="sm">{relativeTime(b.time)}</span>}
          </button>
        ))}
      </div>
      {remote.length > 0 && (
        <div>
          <h3>Remote</h3>
          {remote.slice(0, 20).map((b) => (
            <button key={b.full} className="si" onDoubleClick={() => run(() => api.switchBranch(b.name, true))} title="Double-click to check out">
              <span className="grow muted">{b.name}</span>
            </button>
          ))}
        </div>
      )}
      {st && st.stashes.length > 0 && (
        <div>
          <h3>Stashes</h3>
          {st.stashes.map((s) => (
            <button key={s.oid} className="si" onDoubleClick={() => run(() => api.stashApply(s.name, true))} title="Double-click to pop">
              <span className="grow">{s.message}</span>
              <span className="sm">{relativeTime(s.time)}</span>
            </button>
          ))}
        </div>
      )}
    </aside>
  );
}

function Palette({ items, onClose }: { items: PaletteItem[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const shown = items.filter((i) => (i.label + " " + i.cmd).toLowerCase().includes(q.toLowerCase()));
  useEffect(() => setSel(0), [q]);
  const go = (i: PaletteItem | undefined) => {
    if (!i) return;
    onClose();
    i.run();
  };
  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog pal" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Command palette">
        <input
          autoFocus
          placeholder="Type a command or branch name…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(shown.length - 1, s + 1)); }
            else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
            else if (e.key === "Enter") go(shown[sel]);
            else if (e.key === "Escape") onClose();
          }}
        />
        <div className="pl">
          {shown.length === 0 && <div className="empty">No matching command.</div>}
          {shown.map((i, k) => (
            <button key={i.label} className={`po ${k === sel ? "on" : ""}`} onMouseEnter={() => setSel(k)} onClick={() => go(i)}>
              <span>{i.label}</span>
              <code>{i.cmd}</code>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function PromptDialog({ p, onDone }: { p: Prompt; onDone: (v: string | null) => void }) {
  const [v, setV] = useState(p.value);
  return (
    <div className="overlay" onClick={() => onDone(null)}>
      <div className="dialog" onClick={(e) => e.stopPropagation()} role="dialog">
        <div className="body2">
          <h3>{p.title}</h3>
          {p.multiline ? (
            <textarea className="field" autoFocus rows={8} value={v} onChange={(e) => setV(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onDone(v); if (e.key === "Escape") onDone(null); }} />
          ) : (
            <input className="field" autoFocus value={v} onChange={(e) => setV(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") onDone(v); if (e.key === "Escape") onDone(null); }} />
          )}
          {p.warn && <p style={{ color: "var(--orange)" }}>{p.warn}</p>}
          {p.body && <p>{p.body}</p>}
        </div>
        <div className="actions">
          <button className="btn" onClick={() => onDone(null)}>Cancel</button>
          <button className="btn primary" onClick={() => onDone(v)}>{p.confirm}{p.multiline ? " (⌘↵)" : ""}</button>
        </div>
      </div>
    </div>
  );
}

function Welcome({ onOpen, onBrowse }: { onOpen: (p: string) => void; onBrowse: () => void }) {
  const [repos, setRepos] = useState<RepoSummary[] | null>(null);
  useEffect(() => {
    api.repoList().then(setRepos).catch(() => setRepos([]));
  }, []);
  return (
    <div className="welcome">
      <div className="box">
        <h1>Open a repository</h1>
        <p className="muted" style={{ margin: 0 }}>Pick a folder that contains a git repository, or one you opened before.</p>
        <div><button className="btn primary" onClick={onBrowse}>Choose folder…</button></div>
        {repos && repos.length > 0 && (
          <div className="list">
            {repos.map((r) => (
              <button key={r.path} className="rr" onClick={() => onOpen(r.path)}>
                <span className={`dot ${r.changes ? "dirty" : "clean"}`} />
                <span><b>{r.name}</b> <span className="muted mono" style={{ fontSize: 11 }}>{r.path}</span></span>
                <span className="muted">⎇ {r.branch ?? "-"}</span>
                <span className="muted">{r.last_commit ? relativeTime(r.last_commit) : ""}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
