// Settings, ignore rules and clone dialogs.
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { IgnoreSource, RepoInfo } from "../api";
import { api, errorText } from "../api";
import type { Ctx } from "./Changes";

// ------------------------------------------------------------------ settings

export type Theme = "system" | "light" | "dark";
export interface Settings {
  theme: Theme;
  diffSize: number;
  allBranches: boolean;
}
export const DEFAULT_SETTINGS: Settings = { theme: "system", diffSize: 12.5, allBranches: true };

export function loadSettings(): Settings {
  try {
    return { ...DEFAULT_SETTINGS, ...JSON.parse(localStorage.getItem("gitty.settings") ?? "{}") };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(s: Settings) {
  try {
    localStorage.setItem("gitty.settings", JSON.stringify(s));
  } catch {
    /* private mode: settings just won't persist */
  }
}

function Segmented<T extends string | number>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div className="modes">
      {options.map(([v, label]) => (
        <button key={String(v)} className={value === v ? "on" : ""} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function SettingsDialog({ settings, onChange, onClose }: { settings: Settings; onChange: (s: Settings) => void; onClose: () => void }) {
  const set = (patch: Partial<Settings>) => onChange({ ...settings, ...patch });
  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Settings">
        <div className="body2">
          <h3>Settings</h3>
          <div className="setting">
            <span className="l">Appearance</span>
            <Segmented value={settings.theme} options={[["system", "System"], ["light", "Light"], ["dark", "Dark"]]} onChange={(theme) => set({ theme })} />
          </div>
          <div className="setting">
            <span className="l">Diff text size</span>
            <Segmented value={settings.diffSize} options={[[11.5, "Small"], [12.5, "Medium"], [14, "Large"]]} onChange={(diffSize) => set({ diffSize })} />
          </div>
          <div className="setting">
            <span className="l">
              History shows
              <small>You can still switch from the History tab</small>
            </span>
            <Segmented value={settings.allBranches ? "all" : "current"} options={[["all", "All branches"], ["current", "Current branch"]]} onChange={(v) => set({ allBranches: v === "all" })} />
          </div>
        </div>
        <div className="actions">
          <button className="btn primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ ignore rules

export function IgnoreDialog({ ctx, onClose, checkPath }: { ctx: Ctx; onClose: () => void; checkPath?: string }) {
  const [sources, setSources] = useState<IgnoreSource[]>([]);
  const [sel, setSel] = useState(0);
  const [adding, setAdding] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [why, setWhy] = useState(checkPath ?? "");
  const [answer, setAnswer] = useState<{ text: string; ignored: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = async (first = false) => {
    try {
      const s = await api.ignoreSources();
      setSources(s);
      if (first) {
        // start on the root .gitignore, the file people edit most
        const root = s.findIndex((x) => x.label === ".gitignore");
        setSel(root >= 0 ? root : 0);
      } else {
        setSel((i) => Math.min(i, s.length - 1));
      }
    } catch (e) {
      setError(errorText(e));
    }
  };
  useEffect(() => {
    load(true);
    if (checkPath) check(checkPath);
  }, []);

  const src = sources[sel];
  const editable = !!src?.rel;
  const check = async (path: string) => {
    if (!path.trim()) return;
    try {
      const w = await api.whyIgnored(path.trim());
      setAnswer(
        !w
          ? { text: `${path} is not ignored by any rule.`, ignored: false }
          : w.negated
            ? { text: `${path} is NOT ignored — ${w.pattern} (line ${w.line} of ${w.source}) re-includes it.`, ignored: false }
            : { text: `${path} is ignored by ${w.pattern} — line ${w.line} of ${w.source}.`, ignored: true },
      );
    } catch (e) {
      setAnswer({ text: errorText(e), ignored: false });
    }
  };
  const add = async () => {
    if (!adding.trim() || !src?.rel) return;
    if (await ctx.run(() => api.addIgnore(src.rel!, adding.trim(), []))) {
      setAdding("");
      load();
    }
  };
  const startEdit = async () => {
    if (!src?.rel) return;
    setEditing(await api.readTextFile(src.rel));
  };
  const saveEdit = async () => {
    if (editing === null || !src?.rel) return;
    if (await ctx.run(() => api.writeIgnoreFile(src.rel!, editing))) {
      setEditing(null);
      load();
    }
  };

  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog wide" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Ignore rules">
        <div className="body2">
          <h3>Ignore rules</h3>
          <p>Every file that decides what git ignores here. Later files win; rules starting with ! bring files back.</p>
          {error && <p style={{ color: "var(--del)" }}>{error}</p>}
        </div>
        <div className="ig">
          <div className="ig-src">
            {sources.map((s, i) => (
              <button key={s.abs} className={`si ${i === sel ? "on" : ""}`} onClick={() => { setSel(i); setEditing(null); }} title={s.abs}>
                <span className="grow">{s.label}</span>
                <span className="sm">{s.exists ? s.rules.filter((r) => !r.comment).length : "new"}</span>
              </button>
            ))}
          </div>
          <div className="ig-main">
            {editing !== null ? (
              <textarea className="ig-edit" autoFocus value={editing} onChange={(e) => setEditing(e.target.value)} spellCheck={false} />
            ) : (
              <div className="ig-rules">
                {src && !src.exists && <div className="note">This file doesn't exist yet. Add a rule or edit it to create it.</div>}
                {src?.rules.map((r) => (
                  <div key={r.line} className={`ig-rule ${r.comment ? "comment" : r.text.trim().startsWith("!") ? "neg" : ""}`}>
                    <span className="no">{r.line}</span>
                    <span>{r.text}</span>
                    {editable && !r.comment && (
                      <button className="btn small danger" onClick={async () => { if (await ctx.run(() => api.removeIgnore(src.rel!, r.line, r.text))) load(); }}>
                        Remove
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
            <div className="ig-bar">
              {editing !== null ? (
                <>
                  <span className="muted" style={{ flex: 1, fontSize: 12 }}>Editing {src?.rel}. Undo reverts the save.</span>
                  <button className="btn" onClick={() => setEditing(null)}>Cancel</button>
                  <button className="btn primary" onClick={saveEdit}>Save</button>
                </>
              ) : editable ? (
                <>
                  <input className="field mono" placeholder="New rule, e.g. *.log or /Builds/" value={adding} onChange={(e) => setAdding(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} />
                  <button className="btn" disabled={!adding.trim()} onClick={add}>Add</button>
                  <button className="btn" onClick={startEdit}>Edit file</button>
                </>
              ) : (
                <span className="muted" style={{ fontSize: 12 }}>This file is outside the repository ({src?.abs}). Edit it with any text editor.</span>
              )}
            </div>
          </div>
        </div>
        <div className="why">
          <input className="field mono" style={{ maxWidth: 360 }} placeholder="Why is this path ignored? e.g. Library/cache.bin" value={why} onChange={(e) => setWhy(e.target.value)} onKeyDown={(e) => e.key === "Enter" && check(why)} />
          <button className="btn" onClick={() => check(why)}>Check</button>
          {answer && <span className="res" style={{ color: answer.ignored ? "var(--orange)" : "var(--accent)" }}>{answer.text}</span>}
        </div>
        <div className="actions">
          <button className="btn primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ clone

export function CloneDialog({ onDone, onClose }: { onDone: (r: RepoInfo) => void; onClose: () => void }) {
  const [url, setUrl] = useState("");
  const [parent, setParent] = useState("");
  const [name, setName] = useState("");
  const [nameTouched, setNameTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.cloneDefaults().then((d) => setParent(d.parent));
    const un = listen<string>("git-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
  useEffect(() => {
    if (!nameTouched && url.trim()) api.cloneName(url).then(setName);
  }, [url, nameTouched]);

  const choose = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Clone into this folder", defaultPath: parent || undefined });
    if (typeof dir === "string") setParent(dir);
  };
  const sep = parent.includes("\\") ? "\\" : "/";
  const target = parent && name ? `${parent.replace(/[\\/]$/, "")}${sep}${name}` : "";
  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      onDone(await api.cloneRepo(url.trim(), parent, name.trim()));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
      setProgress("");
    }
  };
  return (
    <div className="overlay" onClick={() => !busy && onClose()}>
      <div className="dialog" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Clone a repository">
        <div className="body2">
          <h3>Clone a repository</h3>
          <div className="setting">
            <span className="l">Repository URL</span>
            <input className="field mono" autoFocus placeholder="https://github.com/owner/repo.git or git@…" value={url} onChange={(e) => setUrl(e.target.value)} onKeyDown={(e) => e.key === "Enter" && url && parent && name && go()} disabled={busy} />
          </div>
          <div className="setting">
            <span className="l">Into folder</span>
            <div className="row">
              <input className="field mono" value={parent} onChange={(e) => setParent(e.target.value)} disabled={busy} />
              <button className="btn" onClick={choose} disabled={busy}>Choose…</button>
            </div>
          </div>
          <div className="setting">
            <span className="l">Folder name</span>
            <input className="field mono" value={name} onChange={(e) => { setName(e.target.value); setNameTouched(true); }} disabled={busy} />
          </div>
          {target && <p className="mono" style={{ fontSize: 12 }}>Clones to {target}</p>}
          {busy && <div className="row"><span className="spin" /><span className="progress">{progress || "Connecting…"}</span></div>}
          {error && <p style={{ color: "var(--del)" }}>{error}</p>}
        </div>
        <div className="actions">
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn primary" disabled={busy || !url.trim() || !parent || !name.trim()} onClick={go}>
            {busy ? "Cloning…" : "Clone"}
          </button>
        </div>
      </div>
    </div>
  );
}
