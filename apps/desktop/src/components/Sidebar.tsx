// Sidebar: repository switcher, collapsible branch tree, remotes, stashes, tags.
import { useEffect, useMemo, useRef, useState } from "react";
import type { Branch, OpResult, RepoInfo, RepoSummary, StatusDto } from "../api";
import { api, relativeTime } from "../api";
import { BranchIcon, ChevronDown, ChevronRight, CloudIcon, DownloadIcon, FolderIcon, FolderOpenIcon, GearIcon, IgnoreIcon, PlusIcon, SearchIcon, StashIcon, TagIcon } from "./Icons";

function useStored(key: string, initial: boolean): [boolean, (v: boolean) => void] {
  const [v, setV] = useState<boolean>(() => {
    try {
      const s = localStorage.getItem(key);
      return s === null ? initial : s === "1";
    } catch {
      return initial;
    }
  });
  return [
    v,
    (next) => {
      setV(next);
      try {
        localStorage.setItem(key, next ? "1" : "0");
      } catch {
        /* not persisted */
      }
    },
  ];
}

function Section({ id, title, count, defaultOpen = true, children }: { id: string; title: string; count?: string | number; defaultOpen?: boolean; children: React.ReactNode }) {
  const [open, setOpen] = useStored(`gitty.sec.${id}`, defaultOpen);
  return (
    <div className="sec">
      <button className="sec-h" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="tw">{open ? <ChevronDown size={9} /> : <ChevronRight size={9} />}</span>
        {title}
        {count !== undefined && <span className="n">{count}</span>}
      </button>
      {open && <div className="sec-b">{children}</div>}
    </div>
  );
}

interface TreeNode {
  name: string;
  path: string;
  branch?: Branch;
  children: TreeNode[];
}

/** Group `feat/a`, `feat/b` under a `feat` folder. Single-entry folders stay flat. */
function branchTree(branches: Branch[], strip: (b: Branch) => string): TreeNode[] {
  const root: TreeNode = { name: "", path: "", children: [] };
  for (const b of branches) {
    const parts = strip(b).split("/");
    let node = root;
    parts.forEach((part, i) => {
      const last = i === parts.length - 1;
      let child = node.children.find((c) => c.name === part && !c.branch === !last);
      if (!child) {
        child = { name: part, path: parts.slice(0, i + 1).join("/"), children: [], branch: last ? b : undefined };
        node.children.push(child);
      }
      node = child;
    });
  }
  const flatten = (n: TreeNode): TreeNode => {
    n.children = n.children.map(flatten);
    if (!n.branch && n.children.length === 1 && n.children[0].branch) {
      const only = n.children[0];
      return { ...only, name: `${n.name}/${only.name}` };
    }
    return n;
  };
  root.children = root.children.map(flatten);
  // folders first, then branches, both alphabetical — except main/master/develop at the top
  const rank = (n: TreeNode) => (["main", "master", "develop", "dev"].includes(n.name) ? 0 : n.branch ? 2 : 1);
  const sort = (ns: TreeNode[]) => {
    ns.sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
    ns.forEach((n) => sort(n.children));
  };
  sort(root.children);
  return root.children;
}

function TreeView({ nodes, depth, onPick, current }: { nodes: TreeNode[]; depth: number; onPick: (b: Branch) => void; current?: string }) {
  return (
    <>
      {nodes.map((n) =>
        n.branch ? (
          <button
            key={n.path}
            className={`it ${n.branch.head ? "on" : ""}`}
            style={{ paddingLeft: 8 + depth * 14 }}
            onDoubleClick={() => onPick(n.branch!)}
            title={n.branch.head ? `${n.branch.name} (current)` : `Double-click to switch to ${n.branch.name}`}
          >
            <BranchIcon className="ico" />
            <span className="g">{n.name}</span>
            {n.branch.gone ? (
              <span className="m" title="Upstream deleted">gone</span>
            ) : n.branch.ahead || n.branch.behind ? (
              <span className="ab">
                {n.branch.ahead > 0 && <span className="add">↑{n.branch.ahead}</span>}
                {n.branch.behind > 0 && <span className="del">↓{n.branch.behind}</span>}
              </span>
            ) : (
              <span className="m">{relativeTime(n.branch.time)}</span>
            )}
          </button>
        ) : (
          <Folder key={n.path} node={n} depth={depth} onPick={onPick} current={current} />
        ),
      )}
    </>
  );
}

function Folder({ node, depth, onPick, current }: { node: TreeNode; depth: number; onPick: (b: Branch) => void; current?: string }) {
  const hasCurrent = (n: TreeNode): boolean => !!n.branch?.head || n.children.some(hasCurrent);
  const [open, setOpen] = useState(() => hasCurrent(node) || depth > 0);
  const count = (n: TreeNode): number => (n.branch ? 1 : n.children.reduce((s, c) => s + count(c), 0));
  return (
    <>
      <button className="it folder" style={{ paddingLeft: 8 + depth * 14 }} onClick={() => setOpen(!open)} aria-expanded={open}>
        <FolderIcon className="ico" />
        <span className="g">{node.name}</span>
        <span className="m">{count(node)}</span>
      </button>
      {open && <TreeView nodes={node.children} depth={depth + 1} onPick={onPick} current={current} />}
    </>
  );
}

export function RepoSwitcher({ repo, project, onOpenRepo, onBrowse, onClone, onNew }: {
  repo: RepoInfo;
  project?: string;
  onOpenRepo: (p: string) => void;
  onBrowse: () => void;
  onClone: () => void;
  onNew: () => void;
}) {
  const [open, setOpen] = useState(() => !!(window as unknown as { __gittyDev?: string[] }).__gittyDev?.includes("repopop"));
  const [repos, setRepos] = useState<RepoSummary[]>([]);
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open) api.repoList().then(setRepos).catch(() => setRepos([]));
  }, [open]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "o") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  const shown = repos.filter((r) => (r.name + " " + r.path).toLowerCase().includes(q.toLowerCase()));
  useEffect(() => setSel(0), [q]);
  const pick = (r?: RepoSummary) => {
    if (!r) return;
    setOpen(false);
    setQ("");
    if (r.path !== repo.root) onOpenRepo(r.path);
  };
  const initials = repo.name.replace(/[^A-Za-z0-9]/g, " ").split(/\s+/).filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase() || "G";
  const where = repo.root.replace(/^\/Users\/[^/]+/, "~").split("/").slice(0, -1).join("/");
  return (
    <div className="repo-wrap" ref={ref}>
      <button className="repo-btn" onClick={() => setOpen(!open)} aria-haspopup="dialog" aria-expanded={open} title="Switch repository (⌘O)">
        <span className="repo-ic">{initials}</span>
        <span className="t">
          <b>{repo.name}</b>
          <span>{[where, project].filter(Boolean).join(" · ")}</span>
        </span>
        <ChevronDown className="chev" />
      </button>
      {open && (
        <div className="pop repo-pop" role="dialog" aria-label="Switch repository">
          <div className="srch">
            <SearchIcon className="ico" />
            <input
              autoFocus
              placeholder="Find a repository…"
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(shown.length - 1, s + 1)); }
                else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
                else if (e.key === "Enter") pick(shown[sel]);
                else if (e.key === "Escape") setOpen(false);
              }}
            />
            <span className="kbd">⌘O</span>
          </div>
          <div className="lab">Recent and nearby</div>
          <div className="rlist">
            {shown.length === 0 && <div className="empty" style={{ padding: 12 }}>No matching repository.</div>}
            {shown.map((r, i) => (
              <button key={r.path} className={`rr ${i === sel ? "on" : ""}`} onMouseEnter={() => setSel(i)} onClick={() => pick(r)} title={r.path}>
                <span className={`dot ${r.conflicts ? "conflict" : r.changes ? "dirty" : "clean"}`} />
                <span className="nm">{r.path === repo.root ? <b>{r.name}</b> : r.name}</span>
                <span className="b">{r.branch ?? ""}</span>
                <span className="b">{r.changes || ""}</span>
              </button>
            ))}
          </div>
          <div className="foot">
            <button className="btn" onClick={() => { setOpen(false); onClone(); }}><DownloadIcon className="ico" />Clone…</button>
            <button className="btn" onClick={() => { setOpen(false); onBrowse(); }}><FolderOpenIcon className="ico" />Open…</button>
            <button className="btn" onClick={() => { setOpen(false); onNew(); }}><PlusIcon className="ico" />New…</button>
          </div>
        </div>
      )}
    </div>
  );
}

export function Sidebar(props: {
  repo: RepoInfo;
  st: StatusDto | null;
  branches: Branch[];
  project?: string;
  mac: boolean;
  onOpenRepo: (p: string) => void;
  onBrowse: () => void;
  onClone: () => void;
  onNew: () => void;
  run: (op: () => Promise<OpResult>) => Promise<boolean>;
  onNewBranch: () => void;
  onIgnore: () => void;
  onSettings: () => void;
}) {
  const { st, branches, run } = props;
  const [filter, setFilter] = useState("");
  const [tags, setTags] = useState<[string, string, number][]>([]);
  useEffect(() => {
    api.tags().then(setTags).catch(() => setTags([]));
  }, [props.repo.root, st?.fingerprint]);
  const f = filter.toLowerCase();
  const local = useMemo(() => branchTree(branches.filter((b) => !b.remote && b.name.toLowerCase().includes(f)), (b) => b.name), [branches, f]);
  const remotes = useMemo(() => {
    const by = new Map<string, Branch[]>();
    for (const b of branches.filter((b) => b.remote && b.name.toLowerCase().includes(f))) {
      const r = b.name.split("/")[0];
      by.set(r, [...(by.get(r) ?? []), b]);
    }
    return [...by.entries()];
  }, [branches, f]);
  const localCount = branches.filter((b) => !b.remote).length;
  const remoteCount = branches.filter((b) => b.remote).length;
  const switchTo = (b: Branch) => !b.head && run(() => api.switchBranch(b.name, b.remote));

  return (
    <aside className="side">
      <div className="side-top" data-tauri-drag-region style={{ height: props.mac ? 46 : 10 }} />
      <RepoSwitcher repo={props.repo} project={props.project} onOpenRepo={props.onOpenRepo} onBrowse={props.onBrowse} onClone={props.onClone} onNew={props.onNew} />
      <div className="side-scroll">
        <Section id="branches" title="Branches" count={localCount}>
          <div className="filter">
            <SearchIcon className="ico" />
            <input placeholder="Filter branches…" value={filter} onChange={(e) => setFilter(e.target.value)} />
            <button className="icon-btn" title="New branch" onClick={props.onNewBranch}><PlusIcon /></button>
          </div>
          <TreeView nodes={local} depth={0} onPick={switchTo} />
          {local.length === 0 && filter && <div className="it muted">No branch matches “{filter}”.</div>}
        </Section>
        {remoteCount > 0 && (
          <Section id="remotes" title="Remotes" count={remotes.map(([r]) => r).join(", ") || remoteCount} defaultOpen={false}>
            {remotes.map(([r, bs]) => (
              <RemoteGroup key={r} name={r} branches={bs} onPick={switchTo} />
            ))}
          </Section>
        )}
        {st && st.stashes.length > 0 && (
          <Section id="stashes" title="Stashes" count={st.stashes.length}>
            {st.stashes.map((s) => (
              <button key={s.oid} className="it" onDoubleClick={() => run(() => api.stashApply(s.name, true))} title={`${s.message} — double-click to pop`}>
                <StashIcon className="ico" />
                <span className="g">{s.message.replace(/^(On|WIP on) [^:]+: /, "")}</span>
                <span className="m">{relativeTime(s.time)}</span>
              </button>
            ))}
          </Section>
        )}
        {tags.length > 0 && (
          <Section id="tags" title="Tags" count={tags.length} defaultOpen={false}>
            {tags.slice(0, 50).map(([name, oid, time]) => (
              <div key={name} className="it" title={oid}>
                <TagIcon className="ico" />
                <span className="g">{name}</span>
                <span className="m">{relativeTime(time)}</span>
              </div>
            ))}
          </Section>
        )}
      </div>
      <div className="side-foot">
        <button onClick={props.onIgnore} title="Ignore rules"><IgnoreIcon className="ico" />Ignore</button>
        <button onClick={props.onSettings} title="Settings (⌘,)"><GearIcon className="ico" />Settings</button>
      </div>
    </aside>
  );
}

function RemoteGroup({ name, branches, onPick }: { name: string; branches: Branch[]; onPick: (b: Branch) => void }) {
  const [open, setOpen] = useState(true);
  const tree = useMemo(() => branchTree(branches, (b) => b.name.slice(name.length + 1)), [branches, name]);
  return (
    <>
      <button className="it folder" onClick={() => setOpen(!open)} aria-expanded={open}>
        <CloudIcon className="ico" />
        <span className="g">{name}</span>
        <span className="m">{branches.length}</span>
      </button>
      {open && <TreeView nodes={tree} depth={1} onPick={onPick} />}
    </>
  );
}
