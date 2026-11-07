// Toolbar controls: branch switcher and the sync split button.
import { useEffect, useRef, useState } from "react";
import type { Branch, BranchStatus, OpResult } from "../api";
import { api, relativeTime } from "../api";
import { BranchIcon, ChevronDown, PlusIcon, SearchIcon, SyncIcon } from "./Icons";

function usePopover(initial = false) {
  const [open, setOpen] = useState(initial);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);
  return { open, setOpen, ref };
}

export function BranchSwitcher({ branch, branches, run, onNewBranch }: {
  branch: BranchStatus | undefined;
  branches: Branch[];
  run: (op: () => Promise<OpResult>) => Promise<boolean>;
  onNewBranch: (name?: string) => void;
}) {
  const { open, setOpen, ref } = usePopover(!!(window as unknown as { __gittyDev?: string[] }).__gittyDev?.includes("branchpop"));
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const label = branch?.head ?? (branch?.oid ? `detached ${branch.oid.slice(0, 7)}` : "no branch");
  const match = (b: Branch) => b.name.toLowerCase().includes(q.toLowerCase());
  const local = branches.filter((b) => !b.remote && match(b));
  // remote branches that have no local counterpart yet
  const remote = branches.filter((b) => b.remote && match(b) && !branches.some((l) => !l.remote && l.name === b.name.split("/").slice(1).join("/")));
  const all = [...local, ...remote];
  const exact = branches.some((b) => b.name === q.trim());
  useEffect(() => setSel(0), [q]);
  const pick = (b?: Branch) => {
    if (!b) {
      if (q.trim() && !exact) {
        setOpen(false);
        onNewBranch(q.trim());
      }
      return;
    }
    setOpen(false);
    setQ("");
    if (!b.head) run(() => api.switchBranch(b.name, b.remote));
  };
  const row = (b: Branch, i: number) => (
    <button key={b.full} className={`rr ${i === sel ? "on" : ""}`} onMouseEnter={() => setSel(i)} onClick={() => pick(b)}>
      <span className={`dot ${b.head ? "clean" : ""}`} />
      <span className="nm mono">{b.head ? <b>{b.name}</b> : b.name}</span>
      <span className="b">
        {b.ahead > 0 && <span className="add">↑{b.ahead}</span>}
        {b.behind > 0 && <span className="del">↓{b.behind}</span>}
      </span>
      <span className="b">{relativeTime(b.time)}</span>
    </button>
  );
  return (
    <div className="pop-wrap" ref={ref}>
      <button className="tb mono" onClick={() => setOpen(!open)} aria-haspopup="dialog" aria-expanded={open} title="Switch branch">
        <BranchIcon className="ico" />
        <span className="ellip">{label}</span>
        <ChevronDown className="chev" />
      </button>
      {open && (
        <div className="pop branch-pop" role="dialog" aria-label="Switch branch">
          <div className="srch">
            <SearchIcon className="ico" />
            <input
              autoFocus
              placeholder="Find or create a branch…"
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(all.length - 1, s + 1)); }
                else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
                else if (e.key === "Enter") pick(all[sel]);
              }}
            />
          </div>
          <div className="rlist">
            {local.length > 0 && <div className="lab">Local</div>}
            {local.map((b, i) => row(b, i))}
            {remote.length > 0 && <div className="lab">Remote</div>}
            {remote.map((b, i) => row(b, local.length + i))}
            {all.length === 0 && <div className="empty" style={{ padding: 12 }}>No branch named “{q}”.</div>}
          </div>
          <div className="foot">
            <button className="btn" onClick={() => { setOpen(false); onNewBranch(q.trim() && !exact ? q.trim() : undefined); }}>
              <PlusIcon className="ico" />
              {q.trim() && !exact ? `Create “${q.trim()}”` : "New branch from here"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export function SyncButton({ branch, run, remotes }: { branch: BranchStatus | undefined; run: (op: () => Promise<OpResult>) => Promise<boolean>; remotes: string[] }) {
  const { open, setOpen, ref } = usePopover();
  const ahead = branch?.ahead ?? 0;
  const behind = branch?.behind ?? 0;
  const tracked = !!branch?.upstream;
  const noRemote = remotes.length === 0;
  const label = noRemote ? "No remote" : !tracked ? "Publish" : "Sync";
  const title = noRemote
    ? "Add a remote first"
    : !tracked
      ? `Push ${branch?.head ?? "this branch"} and track it`
      : ahead || behind
        ? `Pull ${behind} and push ${ahead} commit(s)`
        : "Fetch and check for new commits";
  const item = (text: string, cmd: string, op: Parameters<typeof api.remote>[0], disabled = false) => (
    <button className="mi" disabled={disabled} onClick={() => { setOpen(false); run(() => api.remote(op)); }}>
      {text}
      <code>{cmd}</code>
    </button>
  );
  return (
    <div className="pop-wrap split-btn" ref={ref}>
      <button className="tb" disabled={noRemote} onClick={() => run(() => api.remote(tracked ? "sync" : "push"))} title={title}>
        <SyncIcon className="ico" />
        {label}
        {tracked && (ahead > 0 || behind > 0) && (
          <span className="ab">
            {ahead > 0 && <span className="add">↑{ahead}</span>}
            {behind > 0 && <span className="del">↓{behind}</span>}
          </span>
        )}
      </button>
      <button className="tb" onClick={() => setOpen(!open)} aria-label="More sync options" aria-expanded={open}>
        <ChevronDown />
      </button>
      {open && (
        <div className="pop sync-pop" role="menu">
          {item("Fetch all remotes", "git fetch --all", "fetch", noRemote)}
          {item(behind ? `Pull ${behind} commit${behind === 1 ? "" : "s"}` : "Pull", "git pull", "pull", !tracked)}
          {item(ahead ? `Push ${ahead} commit${ahead === 1 ? "" : "s"}` : tracked ? "Push" : "Publish branch", tracked ? "git push" : "git push -u", "push", noRemote)}
          <div className="sep" />
          {item("Force push (safe)", "--force-with-lease", "push-force", !tracked)}
        </div>
      )}
    </div>
  );
}
