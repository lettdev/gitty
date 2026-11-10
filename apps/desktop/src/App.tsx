import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CloneDialog, IgnoreDialog, SettingsDialog, loadSettings, saveSettings, type Settings } from "./components/Dialogs";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Branch, FolderPreview, OpResult, RepoInfo, RepoSummary, StatusDto } from "./api";
import { api, errorText, humanSize, isNotARepo, isUntrusted, relativeTime } from "./api";
import { Changes, type Ctx } from "./components/Changes";
import { History } from "./components/History";
import { Insight } from "./components/Insight";
import { Rebase } from "./components/Rebase";
import { Sidebar } from "./components/Sidebar";
import { BranchSwitcher, SyncButton } from "./components/Toolbar";
import { SearchIcon } from "./components/Icons";
import type { RebaseStep } from "./api";

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

const mac = /Mac/.test(navigator.userAgent);

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
  const [rebase, setRebase] = useState<{ base: string | null; steps: RebaseStep[] } | null>(null);
  const toastTimer = useRef<number>(0);
  const pendingRebase = useRef(false);
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const [showSettings, setShowSettings] = useState(false);
  const [ignore, setIgnore] = useState<{ check?: string } | null>(null);
  const [cloning, setCloning] = useState(false);
  // folder waiting on "start tracking with git?"; `fresh` = picked via New repository
  const [initAsk, setInitAsk] = useState<{ path: string; name: string; warning: string | null; fresh: boolean } | null>(null);
  // a repository git won't open because the folder belongs to another account
  const [trustAsk, setTrustAsk] = useState<{ path: string; name: string; trustPath: string } | null>(null);

  // apply the theme to the page and to the native window frame
  useEffect(() => {
    saveSettings(settings);
    const root = document.documentElement;
    if (settings.theme === "system") delete root.dataset.theme;
    else root.dataset.theme = settings.theme;
    root.style.setProperty("--diff-size", `${settings.diffSize}px`);
    getCurrentWindow()
      .setTheme(settings.theme === "system" ? null : settings.theme)
      .catch(() => {});
  }, [settings]);

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
    // dev aid: GITTY_VIEW=history,light opens a view/dialog with a theme, without saving it
    api.startView().then((spec) => {
      const parts = (spec ?? "").split(",");
      (window as unknown as { __gittyDev?: string[] }).__gittyDev = parts;
      const v = parts.find((p) => ["changes", "history", "insight", "rebase"].includes(p)) ?? null;
      const theme = parts.find((p) => p === "light" || p === "dark");
      if (theme) document.documentElement.dataset.theme = theme;
      if (parts.includes("settings")) setShowSettings(true);
      if (parts.includes("ignore")) setIgnore({});
      if (parts.includes("clone")) setCloning(true);
      if (v && ["changes", "history", "insight"].includes(v)) setView(v as View);
      if (v === "rebase") {
        setView("history");
        pendingRebase.current = true;
      }
    });
    const un = listen<string>("git-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!repo) return;
    setSt(null);
    refresh();
    // dev aid: GITTY_VIEW=rebase opens the rebase editor for the last 3 commits
    if (pendingRebase.current) {
      pendingRebase.current = false;
      api.log(false, 3).then((rows) => {
        if (rows.length) api.rebasePlan(rows[rows.length - 1].commit.hash).then(setRebase).catch(() => {});
      });
    }
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
  const ctx: Ctx = useMemo(() => ({ run, ask: askFn, openIgnore: (check?: string) => setIgnore({ check }) }), [run, askFn]);
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
      if (isNotARepo(e)) setInitAsk({ path: e.path, name: e.name, warning: e.warning, fresh: false });
      else if (isUntrusted(e)) setTrustAsk({ path: e.path, name: e.name, trustPath: e.trust_path });
      else notify(errorText(e), true);
    }
  };
  const trustAndOpen = async (path: string, trustPath: string) => {
    setTrustAsk(null);
    try {
      await api.trustFolder(trustPath);
      await openRepo(path);
    } catch (e) {
      notify(errorText(e), true);
    }
  };
  const initHere = async (path: string, name: string, ignoreRules: string[] = [], lfsPatterns: string[] = []) => {
    setInitAsk(null);
    try {
      setRepo(await api.initRepo(path, ignoreRules, lfsPatterns));
      setView("changes");
      notify(`${name} is now a git repository`);
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
  const openRebase = async (hash: string) => {
    try {
      setRebase(await api.rebasePlan(hash));
    } catch (e) {
      notify(errorText(e), true);
    }
  };
  const newBranch = async (initial?: string) => {
    const name = await promptFn({ title: "New branch from HEAD", value: initial ?? "", confirm: "Create and switch" });
    if (name?.trim()) run(() => api.createBranch(name.trim(), null));
  };
  const newRepo = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Choose a folder for the new repository" });
    if (typeof dir !== "string") return;
    const name = dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;
    const warning = await api.initWarning(dir).catch(() => null);
    // always ask: the dialog shows what's in the folder and the rules it would add
    setInitAsk({ path: dir, name, warning, fresh: true });
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
      { label: "Sync (pull, then push)", cmd: "git pull && git push", run: () => run(() => api.remote("sync")) },
      { label: "Force push (safe)", cmd: "git push --force-with-lease", run: () => run(() => api.remote("push-force")) },
      { label: "New repository…", cmd: "git init", run: newRepo },
      { label: "Stash changes…", cmd: "git stash push -u", run: stash },
      { label: "New branch…", cmd: "git switch -c <name>", run: () => newBranch() },
      { label: "Open repository…", cmd: "", run: openFolder },
      { label: "Clone repository…", cmd: "git clone <url>", run: () => setCloning(true) },
      { label: "Ignore rules…", cmd: ".gitignore", run: () => setIgnore({}) },
      { label: "Settings…", cmd: "⌘,", run: () => setShowSettings(true) },
      { label: "Theme: light", cmd: "", run: () => setSettings((s) => ({ ...s, theme: "light" })) },
      { label: "Theme: dark", cmd: "", run: () => setSettings((s) => ({ ...s, theme: "dark" })) },
      { label: "Theme: follow the system", cmd: "", run: () => setSettings((s) => ({ ...s, theme: "system" })) },
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
      } else if (mod && e.key === ",") {
        e.preventDefault();
        setShowSettings(true);
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
  const dialogs = (
    <>
      {showSettings && <SettingsDialog settings={settings} onChange={setSettings} onClose={() => setShowSettings(false)} />}
      {cloning && (
        <CloneDialog
          onClose={() => setCloning(false)}
          onDone={(r) => {
            setCloning(false);
            setRepo(r);
            setView("changes");
            notify(`Cloned ${r.name}`);
          }}
        />
      )}
      {trustAsk && (
        <TrustDialog
          ask={trustAsk}
          onTrust={() => trustAndOpen(trustAsk.path, trustAsk.trustPath)}
          onClose={() => setTrustAsk(null)}
        />
      )}
      {initAsk && (
        <InitDialog
          ask={initAsk}
          onInit={(rules, lfs) => initHere(initAsk.path, initAsk.name, rules, lfs)}
          onBrowse={() => { setInitAsk(null); openFolder(); }}
          onClose={() => setInitAsk(null)}
        />
      )}
    </>
  );
  if (repo === null)
    return (
      <>
        <Welcome onOpen={openRepo} onBrowse={openFolder} onClone={() => setCloning(true)} />
        {dialogs}
        {toast && <div className={`toast ${toast.err ? "err" : ""}`}>{toast.text}</div>}
      </>
    );

  const b = st?.status.branch;
  const changes = st?.status.entries.length ?? 0;
  return (
    <div className={`app ${mac ? "mac" : ""}`}>
      <div className="body">
        <Sidebar
          repo={repo}
          st={st}
          branches={branches}
          mac={mac}
          onOpenRepo={openRepo}
          onBrowse={openFolder}
          onClone={() => setCloning(true)}
          onNew={newRepo}
          run={run}
          onNewBranch={() => newBranch()}
          onIgnore={() => setIgnore({})}
          onSettings={() => setShowSettings(true)}
        />
        <div className="main">
          <div className="toolbar" data-tauri-drag-region>
            <div className="tabs" role="tablist">
              <button role="tab" aria-selected={view === "changes"} className={`tab ${view === "changes" ? "on" : ""}`} onClick={() => setView("changes")} title="⌘1">
                Changes {changes > 0 && <span className="c">{changes}</span>}
              </button>
              <button role="tab" aria-selected={view === "history"} className={`tab ${view === "history" ? "on" : ""}`} onClick={() => setView("history")} title="⌘2">History</button>
              <button role="tab" aria-selected={view === "insight"} className={`tab ${view === "insight" ? "on" : ""}`} onClick={() => setView("insight")} title="⌘3">Insight</button>
            </div>
            <span className="spacer" data-tauri-drag-region />
            {st?.state_label && <span className="state">{st.state_label}</span>}
            <BranchSwitcher branch={b} branches={branches} run={run} onNewBranch={newBranch} />
            <SyncButton branch={b} run={run} remotes={st?.remotes ?? []} />
            <button className="cmdk" onClick={() => setPalette(true)}>
              <SearchIcon className="ico" />
              <span>Search or run…</span>
              <span className="kbd">⌘K</span>
            </button>
          </div>
          <div className="content">
            {!st ? (
              <div className="empty"><span className="spin" /> Loading {repo.name}…</div>
            ) : view === "changes" ? (
              <Changes st={st} ctx={ctx} focusCommit={commitFocus} />
            ) : view === "history" ? (
              <History key={(gotoHash ?? "h") + settings.allBranches} fingerprint={st.fingerprint} onReword={reword} onRebase={openRebase} select={gotoHash} allByDefault={settings.allBranches} />
            ) : (
              <Insight ctx={ctx} fingerprint={st.fingerprint} onGotoCommit={(h) => { setGotoHash(h); setView("history"); }} />
            )}
          </div>
          <div className="sbar">
            {busy > 0 ? (
              <>
                <span className="spin" />
                <span className="last">{progress || "Working…"}</span>
              </>
            ) : (
              <>
                {last && <span className="last">› <b>{last}</b></span>}
                {st?.last_op && (
                  <button className="undo" onClick={() => run(api.undo)} title={`Undo: ${st.last_op}`}>
                    Undo ⌘Z
                  </button>
                )}
              </>
            )}
            <span className="spacer" />
            <span>{b?.head ?? "detached"}{b?.upstream ? ` · ${b.upstream}` : ""}</span>
          </div>
        </div>
      </div>
      {rebase && (
        <Rebase
          initial={rebase.steps}
          onCancel={() => setRebase(null)}
          onRun={async (steps) => {
            const base = rebase.base;
            setRebase(null);
            const ok = await run(() => api.runRebase(base, steps));
            if (!ok) setView("changes");
          }}
        />
      )}
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
      {dialogs}
      {ignore && <IgnoreDialog ctx={ctx} checkPath={ignore.check} onClose={() => setIgnore(null)} />}
      {prompt && <PromptDialog p={prompt} onDone={(v) => { prompt.resolve(v); setPrompt(null); }} />}
      {toast && <div className={`toast ${toast.err ? "err" : ""}`}>{toast.text}</div>}
    </div>
  );
}

function TrustDialog({ ask, onTrust, onClose }: {
  ask: { path: string; name: string; trustPath: string };
  onTrust: () => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Trust this repository">
        <div className="body2">
          <h3>Git doesn't trust “{ask.name}” yet</h3>
          <p>
            This is a git repository, but the folder belongs to a different user account (common for external drives, folders created as
            Administrator, or repositories copied from another computer). Git won't work in it until you trust it.
          </p>
          <p>Only trust folders you know. Trusting lets this repository's settings and hooks run under your account.</p>
          <p className="mono" style={{ fontSize: 12 }}>git config --global --add safe.directory {ask.trustPath}</p>
        </div>
        <div className="actions">
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" onClick={onTrust}>Trust this folder</button>
        </div>
      </div>
    </div>
  );
}

function InitDialog({ ask, onInit, onBrowse, onClose }: {
  ask: { path: string; name: string; warning: string | null; fresh: boolean };
  onInit: (ignoreRules: string[], lfsPatterns: string[]) => void;
  onBrowse: () => void;
  onClose: () => void;
}) {
  const [preview, setPreview] = useState<FolderPreview | null>(null);
  const [failed, setFailed] = useState(false);
  const [useRules, setUseRules] = useState(true);
  const [useLfs, setUseLfs] = useState(true);
  useEffect(() => {
    let live = true;
    api.folderPreview(ask.path).then((p) => live && setPreview(p)).catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [ask.path]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const canLfs = !!preview && preview.lfs_available && preview.lfs_patterns.length > 0;
  const atLeast = preview?.truncated ? "at least " : "";
  const n = (x: number) => x.toLocaleString();
  const go = () => onInit(preview && useRules ? preview.ignore_rules : [], preview && canLfs && useLfs ? preview.lfs_patterns : []);
  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Start a repository">
        <div className="body2">
          <h3>{ask.fresh ? `Create a repository in “${ask.name}”?` : `“${ask.name}” isn't a git repository yet`}</h3>
          <p className="mono" style={{ fontSize: 12 }}>{ask.path}</p>
          {ask.warning && <p style={{ color: "var(--orange)" }}>{ask.warning}</p>}
          {!preview && !failed && <p><span className="spin" /> Looking through the folder…</p>}
          {preview && preview.files === 0 && <p>Empty folder.</p>}
          {preview && preview.files > 0 && (
            <div className="preview">
              <p>
                <b>{preview.kind ? `${preview.kind} project` : "No project type recognised"}</b> · {atLeast}
                {n(preview.files)} files · {humanSize(preview.bytes)}
              </p>
              <table className="ptable">
                <tbody>
                  {preview.top.slice(0, 6).map((e) => (
                    <tr key={e.name} className={e.ignored_by ? "muted" : ""}>
                      <td className="mono">{e.name}{e.is_dir ? "/" : ""}</td>
                      <td className="num">{humanSize(e.bytes)}</td>
                      <td>{e.ignored_by ? `left out (${e.ignored_by})` : ""}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {preview.top.length > 6 && <p className="muted">… {preview.top.length - 6} more</p>}
              {preview.kept_bytes !== preview.bytes && (
                <p>With the recommended rules, git would track {atLeast}{humanSize(preview.kept_bytes)} in {n(preview.kept_files)} files.</p>
              )}
              {preview.big.length > 0 && (
                <p style={{ color: "var(--orange)" }}>
                  {preview.big.length} large files ({humanSize(preview.big.reduce((s, b) => s + b[1], 0))}) are better kept in Git LFS:{" "}
                  {preview.lfs_patterns.join(" ")}
                  {!preview.lfs_available && " — Git LFS isn't installed (git-lfs.com)."}
                </p>
              )}
            </div>
          )}
          {preview && (preview.ignore_rules.length > 0 || canLfs) && (
            <div className="checks">
              {preview.ignore_rules.length > 0 && (
                <label>
                  <input type="checkbox" checked={useRules} onChange={(e) => setUseRules(e.target.checked)} /> Add {preview.kind ?? ""} .gitignore rules{" "}
                  <span className="muted mono">({preview.ignore_rules.slice(0, 4).join(" ")}{preview.ignore_rules.length > 4 ? " …" : ""})</span>
                </label>
              )}
              {canLfs && (
                <label>
                  <input type="checkbox" checked={useLfs} onChange={(e) => setUseLfs(e.target.checked)} /> Track{" "}
                  <span className="mono">{preview.lfs_patterns.slice(0, 5).join(" ")}</span> with Git LFS
                </label>
              )}
            </div>
          )}
          <p className="muted">Your files stay as they are; nothing is staged or committed. You'll see everything in Changes.</p>
        </div>
        <div className="actions">
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn" onClick={onBrowse}>Choose another folder…</button>
          <button className={`btn ${ask.warning ? "" : "primary"}`} autoFocus={!ask.warning} onClick={go}>
            {ask.warning ? "Create it anyway" : "Initialize repository"}
          </button>
        </div>
      </div>
    </div>
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

/** The app icon's cat, for the welcome screen. */
function CatMark() {
  return (
    <svg className="catmark" viewBox="92 92 840 840" width="56" height="56" aria-hidden="true">
      <defs>
        <clipPath id="catmark-clip"><rect x="92" y="92" width="840" height="840" rx="200" /></clipPath>
      </defs>
      <rect x="92" y="92" width="840" height="840" rx="200" fill="#141a17" />
      <g clipPath="url(#catmark-clip)">
        <g transform="translate(92 92) scale(0.8203125)">
          <path d="M180 1100 L180 560 C180 520 190 470 210 430 L250 250 L400 360 Q512 335 624 360 L774 250 L814 430 C834 470 844 520 844 560 L844 1100 Z" fill="#4cc79b" />
          <circle cx="410" cy="600" r="46" fill="#0f1412" />
          <circle cx="614" cy="600" r="46" fill="#0f1412" />
          <path d="M488 700 L536 700 L512 726 Z" fill="#0f1412" stroke="#0f1412" strokeWidth="14" strokeLinejoin="round" />
        </g>
      </g>
    </svg>
  );
}

function Welcome({ onOpen, onBrowse, onClone }: { onOpen: (p: string) => void; onBrowse: () => void; onClone: () => void }) {
  const [repos, setRepos] = useState<RepoSummary[] | null>(null);
  useEffect(() => {
    api.repoList().then(setRepos).catch(() => setRepos([]));
  }, []);
  return (
    <div className="welcome">
      <div className="box">
        <CatMark />
        <h1>Open a repository</h1>
        <p className="muted" style={{ margin: 0 }}>Pick a project folder (Gitty can set up git in it if needed), clone one from a URL, or reopen a recent one.</p>
        <div className="row">
          <button className="btn primary" onClick={onBrowse}>Choose folder…</button>
          <button className="btn" onClick={onClone}>Clone from URL…</button>
        </div>
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
