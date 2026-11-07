import { useEffect, useMemo, useRef, useState } from "react";
import type { Cell, Commit, CommitDetail, DiffDto, LogRow } from "../api";
import { api, relativeTime, splitPath } from "../api";
import { DiffView } from "./DiffView";
import { CELL_W, GRAPH_PAD, GraphRow, ROW_H } from "./Graph";
import { SearchIcon } from "./Icons";

const PAGE = 600;
const MAX_CELLS = 24;
const AVATAR_COLORS = ["#2f7d57", "#7a5cc4", "#c4561d", "#2f6f94", "#a33a5b", "#8a6d1f", "#3d7f8a", "#5b6bb5"];

function hash(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return Math.abs(h);
}

export function Avatar({ name, email, size = 22 }: { name: string; email: string; size?: number }) {
  const initials = name.split(/\s+/).filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase() || "?";
  return (
    <span className="av" style={{ width: size, height: size, fontSize: size * 0.42, background: AVATAR_COLORS[hash(email || name) % AVATAR_COLORS.length] }} title={`${name} <${email}>`}>
      {initials}
    </span>
  );
}

function dayLabel(ts: number): string {
  const d = new Date(ts * 1000);
  const now = new Date();
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return "This week";
  if (days < 14) return "Last week";
  return d.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

const UP = new Set(["│", "●", "╯", "╰", "┤", "├", "┼", "┴"]);
const DOWN = new Set(["│", "●", "╮", "╭", "┤", "├", "┼", "┬"]);
/** Lanes that run straight through a day header placed between two rows. */
function passThrough(prev: Cell[] | undefined, next: Cell[] | undefined): Cell[] {
  if (!prev || !next) return [];
  const n = Math.max(prev.length, next.length);
  const out: Cell[] = [];
  for (let i = 0; i < n; i += 1) {
    const ok = i % 2 === 0 && DOWN.has(prev[i]?.ch) && UP.has(next[i]?.ch);
    out.push(ok ? { ch: "│", lane: next[i].lane } : { ch: " ", lane: 0 });
  }
  return out;
}

type Item = { kind: "day"; label: string; at: number } | { kind: "row"; i: number };

export function History({ fingerprint, onReword, onRebase, select, allByDefault }: { fingerprint: string; onReword: (hash: string) => void; onRebase: (hash: string) => void; select?: string | null; allByDefault: boolean }) {
  const [all, setAll] = useState(allByDefault);
  const [limit, setLimit] = useState(PAGE);
  const [rows, setRows] = useState<LogRow[]>([]);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Commit[] | null>(null);
  const [sel, setSel] = useState<string | null>(select ?? null);
  const [detail, setDetail] = useState<CommitDetail | null>(null);
  const [file, setFile] = useState(0);
  const [diff, setDiff] = useState<DiffDto | null>(null);
  const [scroll, setScroll] = useState(0);
  const [height, setHeight] = useState(600);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.log(all, limit).then(setRows).catch(() => setRows([]));
  }, [all, limit, fingerprint]);

  // search the whole history, not only what is loaded
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResults(null);
      return;
    }
    const t = setTimeout(() => api.searchLog(q).then(setResults).catch(() => setResults([])), 250);
    return () => clearTimeout(t);
  }, [query, fingerprint]);
  const searching = results !== null;

  const commits: Commit[] = searching ? results! : rows.map((r) => r.commit);
  const selectedHash = commits.find((c) => c.hash === sel)?.hash ?? commits[0]?.hash;

  // rows interleaved with day headers (all fixed height, for simple virtualization)
  const items: Item[] = useMemo(() => {
    // The graph is in branch order, not time order, so only start a new day
    // group when a commit is older than everything above it; commits that are
    // out of order stay in the group they appear in.
    const out: Item[] = [];
    let last = "";
    let oldest = Infinity;
    commits.forEach((c, i) => {
      const label = dayLabel(c.time);
      if (label !== last && c.time < oldest) {
        out.push({ kind: "day", label, at: i });
        last = label;
      }
      oldest = Math.min(oldest, c.time);
      out.push({ kind: "row", i });
    });
    return out;
  }, [commits]);

  useEffect(() => {
    if (!select || !listRef.current) return;
    const i = items.findIndex((it) => it.kind === "row" && commits[it.i].hash === select);
    if (i >= 0) listRef.current.scrollTop = Math.max(0, i * ROW_H - 120);
  }, [rows.length > 0]);

  useEffect(() => {
    if (!selectedHash) return;
    let live = true;
    const t = setTimeout(() => {
      api.commitDetail(selectedHash).then((d) => {
        if (!live) return;
        setDetail(d);
        setFile(0);
      });
    }, 60);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [selectedHash]);

  useEffect(() => {
    const f = detail?.files[file];
    if (!detail || !f) {
      setDiff(null);
      return;
    }
    let live = true;
    api.commitFileDiff(detail.hash, detail.parents[0] ?? null, f).then((d) => live && setDiff(d));
    return () => {
      live = false;
    };
  }, [detail, file]);

  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // keyboard navigation
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest("input, textarea") || e.metaKey || e.ctrlKey) return;
      const i = commits.findIndex((c) => c.hash === selectedHash);
      let n = i;
      if (e.key === "j" || e.key === "ArrowDown") n = Math.min(commits.length - 1, i + 1);
      else if (e.key === "k" || e.key === "ArrowUp") n = Math.max(0, i - 1);
      else if (e.key === "e" && selectedHash) return onReword(selectedHash);
      else if (e.key === "r" && selectedHash && !searching) return onRebase(selectedHash);
      else if (e.key === "/") {
        e.preventDefault();
        document.getElementById("history-search")?.focus();
        return;
      } else return;
      e.preventDefault();
      setSel(commits[n].hash);
      const el = listRef.current;
      const pos = items.findIndex((it) => it.kind === "row" && it.i === n);
      if (el && pos >= 0) {
        const top = pos * ROW_H;
        if (top < el.scrollTop + ROW_H) el.scrollTop = Math.max(0, top - ROW_H);
        else if (top + ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW_H - el.clientHeight;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const cells = useMemo(() => Math.min(MAX_CELLS, Math.max(2, ...rows.map((r) => r.cells.length))), [rows]);
  const graphW = searching ? 0 : cells * CELL_W + GRAPH_PAD * 2;
  const first = Math.max(0, Math.floor(scroll / ROW_H) - 10);
  const last = Math.min(items.length, Math.ceil((scroll + height) / ROW_H) + 10);
  const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    setScroll(el.scrollTop);
    if (!searching && el.scrollTop + el.clientHeight > el.scrollHeight - ROW_H * 40 && rows.length >= limit) setLimit((l) => l + PAGE);
  };

  const f = detail?.files[file];
  const cols = searching ? "22px 1fr auto" : `${graphW}px 22px 1fr auto`;
  return (
    <div className="split" style={{ gridTemplateColumns: "minmax(440px, 1.25fr) minmax(380px, 1fr)" }}>
      <div className="panel" style={{ borderRight: 0 }}>
        <div className="hl-h">
          <label className="search">
            <SearchIcon className="ico" />
            <input
              id="history-search"
              placeholder="Search messages, authors, hashes…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setQuery("")}
            />
            {!query && <span className="kbd">/</span>}
          </label>
          {!searching && (
            <div className="mini-seg" role="group" aria-label="Branches shown">
              <button className={all ? "on" : ""} onClick={() => setAll(true)}>All branches</button>
              <button className={!all ? "on" : ""} onClick={() => setAll(false)}>This branch</button>
            </div>
          )}
        </div>
        {searching && (
          <div className="gh" style={{ paddingTop: 6 }}>
            <span>{results!.length === 0 ? "No matches" : `${results!.length}${results!.length >= 300 ? "+" : ""} matching commits`}</span>
            <button onClick={() => setQuery("")}>Clear</button>
          </div>
        )}
        <div className="hlist" ref={listRef} onScroll={onScroll} style={{ flex: 1 }}>
          <div style={{ height: items.length * ROW_H, position: "relative" }}>
            {items.slice(first, last).map((it, k) => {
              const top = (first + k) * ROW_H;
              if (it.kind === "day") {
                const pass = searching ? [] : passThrough(rows[it.at - 1]?.cells, rows[it.at]?.cells).slice(0, MAX_CELLS);
                return (
                  <div key={"d" + it.label + it.at} className="hday" style={{ top, gridTemplateColumns: searching ? "1fr" : `${graphW}px 1fr` }}>
                    {!searching && <GraphRow cells={pass} head={false} width={graphW} />}
                    <span>{it.label}</span>
                  </div>
                );
              }
              const c = commits[it.i];
              const r = searching ? null : rows[it.i];
              return (
                <div
                  key={c.hash}
                  className={`hrow ${c.hash === selectedHash ? "sel" : ""}`}
                  style={{ top, gridTemplateColumns: cols }}
                  onClick={() => setSel(c.hash)}
                  onDoubleClick={() => onReword(c.hash)}
                >
                  {r && (
                    <GraphRow
                      cells={r.cells.slice(0, MAX_CELLS)}
                      prev={rows[it.i - 1]?.cells}
                      next={rows[it.i + 1]?.cells}
                      head={c.refs.some((x) => x.kind === "Head")}
                      width={graphW}
                    />
                  )}
                  <Avatar name={c.author} email={c.email} />
                  <span className="msg">
                    {c.refs.map((x) => (
                      <span key={x.kind + x.name} className={`ref ${x.kind}`}>
                        {x.kind === "Head" && x.name !== "HEAD" ? `HEAD → ${x.name}` : x.name}
                      </span>
                    ))}
                    <span className="subj">{c.subject}</span>
                  </span>
                  <span className="meta">
                    {c.author} · {relativeTime(c.time)} · <span className="mono">{c.hash.slice(0, 7)}</span>
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      </div>
      <div className="detail">
        <div className="meta-box">
          {detail ? (
            <>
              <h2>{detail.message.split("\n")[0]}</h2>
              <div className="who">
                <Avatar name={detail.author} email={detail.email} size={20} />
                <b>{detail.author}</b> · {detail.date} · <span className="mono">{detail.hash.slice(0, 10)}</span>
                {detail.parents.length > 1 && <span> · merge of {detail.parents.map((p) => p.slice(0, 7)).join(" + ")}</span>}
              </div>
              {detail.message.includes("\n") && <pre>{detail.message.split("\n").slice(1).join("\n").trim()}</pre>}
              <div className="row" style={{ gap: 6 }}>
                <button className="btn small" onClick={() => onReword(detail.hash)} title="Edit message (e)">Edit message</button>
                <button className="btn small" onClick={() => onRebase(detail.hash)} disabled={searching} title="Reorder, squash or drop commits from here up (r)">Rebase from here…</button>
                <button className="btn small ghost" onClick={() => navigator.clipboard?.writeText(detail.hash).catch(() => {})}>Copy hash</button>
              </div>
            </>
          ) : (
            <span className="muted">{commits.length === 0 ? (searching ? "No commit matches your search." : "No commits yet.") : "Loading…"}</span>
          )}
        </div>
        <div className="cfiles">
          {detail?.files.map((cf, i) => {
            const [name, dir] = splitPath(cf.path);
            return (
              <div key={cf.path} className={`cf ${i === file ? "sel" : ""}`} onClick={() => setFile(i)}>
                <span className={`st st-${cf.status}`}>{cf.status}</span>
                <span className="fn">
                  {name} <span className="d">{dir}</span>
                </span>
                <span className="mono" style={{ fontSize: 11 }}>
                  {cf.adds === null ? <span className="muted">bin</span> : <><span className="add">+{cf.adds}</span> <span className="del">−{cf.dels}</span></>}
                </span>
              </div>
            );
          })}
        </div>
        <DiffView title={f ? splitPath(f.path)[0] : "Diff"} sub={f ? splitPath(f.path)[1] : undefined} path={f?.path} dto={diff} />
      </div>
    </div>
  );
}
