import { useEffect, useMemo, useRef, useState } from "react";
import type { CommitDetail, DiffDto, LogRow } from "../api";
import { api, relativeTime, splitPath } from "../api";
import { DiffView } from "./DiffView";
import { CELL_W, GraphRow, ROW_H } from "./Graph";

const PAGE = 600;
const MAX_CELLS = 24;

export function History({ fingerprint, onReword, select }: { fingerprint: string; onReword: (hash: string) => void; select?: string | null }) {
  const [all, setAll] = useState(true);
  const [limit, setLimit] = useState(PAGE);
  const [rows, setRows] = useState<LogRow[]>([]);
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

  const selected = rows.find((r) => r.commit.hash === sel) ?? rows[0];
  // scroll a commit chosen from elsewhere (Insight) into view once rows arrive
  useEffect(() => {
    if (!select || !listRef.current) return;
    const i = rows.findIndex((r) => r.commit.hash === select);
    if (i >= 0) listRef.current.scrollTop = Math.max(0, i * ROW_H - 120);
  }, [rows.length > 0]);
  useEffect(() => {
    if (!selected) return;
    let live = true;
    const t = setTimeout(() => {
      api.commitDetail(selected.commit.hash).then((d) => {
        if (!live) return;
        setDetail(d);
        setFile(0);
      });
    }, 60);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [selected?.commit.hash]);

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
      const i = rows.findIndex((r) => r === selected);
      let n = i;
      if (e.key === "j" || e.key === "ArrowDown") n = Math.min(rows.length - 1, i + 1);
      else if (e.key === "k" || e.key === "ArrowUp") n = Math.max(0, i - 1);
      else if (e.key === "e" && selected) return onReword(selected.commit.hash);
      else return;
      e.preventDefault();
      setSel(rows[n].commit.hash);
      const el = listRef.current;
      if (el) {
        const top = n * ROW_H;
        if (top < el.scrollTop) el.scrollTop = top;
        else if (top + ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW_H - el.clientHeight;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const cells = useMemo(() => Math.min(MAX_CELLS, Math.max(2, ...rows.map((r) => r.cells.length))), [rows]);
  const graphW = cells * CELL_W + 8;
  const first = Math.max(0, Math.floor(scroll / ROW_H) - 10);
  const last = Math.min(rows.length, Math.ceil((scroll + height) / ROW_H) + 10);
  const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    setScroll(el.scrollTop);
    if (el.scrollTop + el.clientHeight > el.scrollHeight - ROW_H * 40 && rows.length >= limit) setLimit((l) => l + PAGE);
  };

  const f = detail?.files[file];
  return (
    <div className="split" style={{ gridTemplateColumns: "minmax(420px, 1.2fr) minmax(380px, 1fr)" }}>
      <div className="panel" style={{ borderRight: 0 }}>
        <div className="gh">
          <span>
            {rows.length}
            {rows.length >= limit ? "+" : ""} commits · {all ? "all branches" : "current branch"}
          </span>
          <button onClick={() => setAll(!all)}>{all ? "Current branch only" : "All branches"}</button>
        </div>
        <div className="hlist" ref={listRef} onScroll={onScroll} style={{ flex: 1 }}>
          <div style={{ height: rows.length * ROW_H, position: "relative" }}>
            {rows.slice(first, last).map((r, k) => {
              const i = first + k;
              const c = r.commit;
              return (
                <div
                  key={c.hash}
                  className={`hrow ${r === selected ? "sel" : ""}`}
                  style={{ position: "absolute", top: i * ROW_H, left: 0, right: 0, gridTemplateColumns: `${graphW}px 1fr 110px 54px 70px` }}
                  onClick={() => setSel(c.hash)}
                  onDoubleClick={() => onReword(c.hash)}
                >
                  <GraphRow
                    cells={r.cells.slice(0, MAX_CELLS)}
                    prev={rows[i - 1]?.cells}
                    next={rows[i + 1]?.cells}
                    head={c.refs.some((x) => x.kind === "Head")}
                    width={graphW}
                  />
                  <span className="msg">
                    {c.refs.map((x) => (
                      <span key={x.kind + x.name} className={`ref ${x.kind}`}>
                        {x.kind === "Head" && x.name !== "HEAD" ? `HEAD → ${x.name}` : x.name}
                      </span>
                    ))}
                    <span>{c.subject}</span>
                  </span>
                  <span className="au">{c.author}</span>
                  <span className="dt">{relativeTime(c.time)}</span>
                  <span className="hs">{c.hash.slice(0, 7)}</span>
                </div>
              );
            })}
          </div>
        </div>
      </div>
      <div className="detail">
        <div className="meta">
          {detail ? (
            <>
              <div className="row">
                <h2 style={{ flex: 1 }}>{detail.message.split("\n")[0]}</h2>
                <button className="btn small" onClick={() => onReword(detail.hash)} title="Edit message (e)">
                  Edit message
                </button>
              </div>
              <span className="muted mono" style={{ fontSize: 12 }}>
                {detail.hash.slice(0, 12)} · {detail.author} · {detail.date}
                {detail.parents.length > 1 && ` · merge of ${detail.parents.map((p) => p.slice(0, 7)).join(" + ")}`}
              </span>
              {detail.message.includes("\n") && <pre>{detail.message.split("\n").slice(1).join("\n").trim()}</pre>}
            </>
          ) : (
            <span className="muted">{rows.length === 0 ? "No commits yet." : "Loading…"}</span>
          )}
        </div>
        <div className="cfiles">
          {detail?.files.map((cf, i) => {
            const [name, dir] = splitPath(cf.path);
            return (
              <div key={cf.path} className={`cf ${i === file ? "sel" : ""}`} onClick={() => setFile(i)}>
                <span className={`st st-${cf.status}`}>{cf.status}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {name} <span className="muted">{dir}</span>
                </span>
                <span className="mono" style={{ fontSize: 11 }}>
                  {cf.adds === null ? <span className="muted">bin</span> : <><span className="add">+{cf.adds}</span> <span className="del">−{cf.dels}</span></>}
                </span>
              </div>
            );
          })}
        </div>
        <DiffView title={f ? splitPath(f.path)[0] : "Diff"} sub={f ? splitPath(f.path)[1] : undefined} dto={diff} />
      </div>
    </div>
  );
}
