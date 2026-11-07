// Text diff with syntax colors, intra-line highlights, per-line checkboxes,
// unified or split layout, image comparison, and the Unity summary.
import { useEffect, useMemo, useState } from "react";
import type { DiffDto, DiffLine, Hunk, ImagePair } from "../api";
import { humanSize } from "../api";
import { changedSpan, langOf, tokenize, withSpan } from "../highlight";

export type LineKey = `${number}:${number}`;
type Mode = "unified" | "split";

interface Props {
  title: string;
  sub?: string;
  /** File path, for picking the syntax colors. */
  path?: string;
  dto: DiffDto | null;
  loading?: boolean;
  /** When set, change lines are selectable and hunks get action buttons. */
  actions?: {
    verb: string;
    onApply: (lines: [number, number][]) => void;
    onDiscard?: (lines: [number, number][]) => void;
  };
}

function loadMode(): Mode {
  if ((window as unknown as { __gittyDev?: string[] }).__gittyDev?.includes("split")) return "split";
  try {
    return localStorage.getItem("gitty.diffMode") === "split" ? "split" : "unified";
  } catch {
    return "unified";
  }
}

/** For each changed line, the span that differs from its counterpart (del run paired with the add run after it). */
function wordSpans(h: Hunk): Map<number, [number, number] | null> {
  const spans = new Map<number, [number, number] | null>();
  let i = 0;
  while (i < h.lines.length) {
    if (h.lines[i].kind !== "Del") {
      i++;
      continue;
    }
    const dels: number[] = [];
    while (i < h.lines.length && h.lines[i].kind === "Del") dels.push(i++);
    const adds: number[] = [];
    while (i < h.lines.length && h.lines[i].kind === "Add") adds.push(i++);
    dels.forEach((d, k) => {
      const a = adds[k];
      if (a === undefined) return;
      const dt = h.lines[d].text.replace(/\r$/, "");
      const at = h.lines[a].text.replace(/\r$/, "");
      spans.set(d, changedSpan(dt, at));
      spans.set(a, changedSpan(at, dt));
    });
  }
  return spans;
}

/** Rows for the split layout: context on both sides, change runs side by side. */
function splitRows(h: Hunk): { left: number | null; right: number | null }[] {
  const rows: { left: number | null; right: number | null }[] = [];
  let i = 0;
  while (i < h.lines.length) {
    const k = h.lines[i].kind;
    if (k === "Context") {
      rows.push({ left: i, right: i });
      i++;
    } else if (k === "NoNewline") {
      i++;
    } else {
      const dels: number[] = [];
      while (i < h.lines.length && h.lines[i].kind === "Del") dels.push(i++);
      const adds: number[] = [];
      while (i < h.lines.length && h.lines[i].kind === "Add") adds.push(i++);
      for (let r = 0; r < Math.max(dels.length, adds.length); r++) rows.push({ left: dels[r] ?? null, right: adds[r] ?? null });
    }
  }
  return rows;
}

export function DiffView({ title, sub, path, dto, loading, actions }: Props) {
  const [picked, setPicked] = useState<Set<LineKey>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [mode, setModeState] = useState<Mode>(loadMode);
  const setMode = (m: Mode) => {
    setModeState(m);
    try {
      localStorage.setItem("gitty.diffMode", m);
    } catch {
      /* not persisted */
    }
  };
  useEffect(() => {
    setPicked(new Set());
    setAnchor(null);
  }, [dto]);

  const lang = useMemo(() => langOf(path ?? title), [path, title]);
  const spans = useMemo(() => dto?.diff?.hunks.map(wordSpans) ?? [], [dto]);
  // flat list of change lines, for shift-click ranges
  const flat = useMemo(() => {
    const out: [number, number][] = [];
    dto?.diff?.hunks.forEach((h, hi) => h.lines.forEach((l, li) => (l.kind === "Add" || l.kind === "Del") && out.push([hi, li])));
    return out;
  }, [dto]);

  const toggle = (hi: number, li: number, shift: boolean) => {
    const idx = flat.findIndex(([h, l]) => h === hi && l === li);
    const next = new Set(picked);
    if (shift && anchor !== null) {
      const [a, b] = [Math.min(anchor, idx), Math.max(anchor, idx)];
      for (let i = a; i <= b; i++) next.add(`${flat[i][0]}:${flat[i][1]}`);
    } else {
      const k: LineKey = `${hi}:${li}`;
      if (next.has(k)) next.delete(k);
      else next.add(k);
      setAnchor(idx);
    }
    setPicked(next);
  };
  const hunkLines = (hi: number): [number, number][] =>
    (dto?.diff?.hunks[hi].lines ?? []).flatMap((l, li) => (l.kind === "Add" || l.kind === "Del" ? [[hi, li] as [number, number]] : []));
  const pickedLines = (): [number, number][] => [...picked].map((k) => k.split(":").map(Number) as [number, number]);

  const selectable = !!actions && !!dto?.patchable;
  const text = (l: DiffLine, span: [number, number] | null | undefined) =>
    withSpan(tokenize(l.text.replace(/\r$/, ""), lang), span ?? null).map((p, i) => (
      <span key={i} className={`${p.k !== "plain" ? "t-" + p.k : ""} ${p.hl ? (l.kind === "Add" ? "w-add" : "w-del") : ""}`}>
        {p.t}
      </span>
    ));
  const box = (hi: number, li: number, l: DiffLine) => {
    const change = l.kind === "Add" || l.kind === "Del";
    if (!change || !selectable) return <span className="gut" />;
    const on = picked.has(`${hi}:${li}`);
    return (
      <span className="gut">
        <span className={`lbox ${on ? "on" : ""}`} />
      </span>
    );
  };
  const adds = dto?.diff?.hunks.reduce((s, h) => s + h.lines.filter((l) => l.kind === "Add").length, 0) ?? 0;
  const dels = dto?.diff?.hunks.reduce((s, h) => s + h.lines.filter((l) => l.kind === "Del").length, 0) ?? 0;
  const textual = !!dto?.diff && !dto.image && !dto.diff.binary && dto.diff.hunks.length > 0;

  return (
    <div className="diff">
      <div className="dh">
        <span className="path">
          {sub && <span className="p">{sub}/</span>}
          {title}
        </span>
        {textual && (
          <span className="dstat">
            <span className="add">+{adds}</span> <span className="del">−{dels}</span>
          </span>
        )}
        {loading && <span className="spin" />}
        <span className="spacer" />
        {textual && (
          <div className="mini-seg" role="group" aria-label="Diff layout">
            <button className={mode === "unified" ? "on" : ""} onClick={() => setMode("unified")}>Unified</button>
            <button className={mode === "split" ? "on" : ""} onClick={() => setMode("split")}>Split</button>
          </div>
        )}
      </div>
      <div className="dscroll">
        {!dto && !loading && <div className="note">Select a file to see its changes.</div>}
        {dto?.summary && dto.summary.length > 0 && (
          <div className="banner">
            <b>Unity changes</b>
            <div>{dto.summary.join("\n")}</div>
          </div>
        )}
        {dto?.note && <div className="note">{dto.note}</div>}
        {dto?.raw && <pre style={{ margin: 0, padding: "10px 14px" }}>{dto.raw}</pre>}
        {dto?.image && <ImageDiff pair={dto.image} />}
        {dto?.diff && !dto.image && dto.diff.binary && <div className="note">Binary file — no text diff.</div>}
        {dto?.diff && !dto.diff.binary && dto.diff.hunks.length === 0 && !dto.image && (
          <div className="note">No text changes (mode or permission change only).</div>
        )}
        {textual &&
          dto.diff!.hunks.map((h, hi) => {
            const section = h.header.replace(/^@@[^@]*@@\s?/, "");
            return (
              <div key={hi}>
                <div className="hunk">
                  <span>{h.header.match(/^@@[^@]*@@/)?.[0]}</span>
                  {section && <span className="fnc">{section}</span>}
                  {selectable && (
                    <span className="acts">
                      {actions.onDiscard && (
                        <button className="btn small danger" onClick={() => actions.onDiscard!(hunkLines(hi))}>Discard</button>
                      )}
                      <button className="btn small" onClick={() => actions.onApply(hunkLines(hi))}>{actions.verb} hunk</button>
                    </span>
                  )}
                </div>
                {mode === "unified"
                  ? h.lines.map((l, li) => {
                      const change = l.kind === "Add" || l.kind === "Del";
                      const on = picked.has(`${hi}:${li}`);
                      const cls = `dl ${l.kind === "Add" ? "add" : l.kind === "Del" ? "del" : ""} ${change && selectable ? "change" : ""} ${on ? "picked" : ""}`;
                      return (
                        <div key={li} className={cls} onClick={(e) => change && selectable && toggle(hi, li, e.shiftKey)}>
                          {box(hi, li, l)}
                          <span className="no">{l.old_no ?? ""}</span>
                          <span className="no">{l.new_no ?? ""}</span>
                          <span className="sg">{l.kind === "Add" ? "+" : l.kind === "Del" ? "−" : ""}</span>
                          <span className="code">{l.kind === "NoNewline" ? <span className="t-com">{l.text}</span> : text(l, spans[hi]?.get(li))}</span>
                        </div>
                      );
                    })
                  : splitRows(h).map((r, k) => (
                      <div key={k} className="srow">
                        {[r.left, r.right].map((li, side) => {
                          if (li === null) return <div key={side} className="sl sl-gap" />;
                          const l = h.lines[li];
                          const change = l.kind !== "Context";
                          const on = picked.has(`${hi}:${li}`);
                          const cls = `sl ${l.kind === "Add" ? "add" : l.kind === "Del" ? "del" : ""} ${change && selectable ? "change" : ""} ${on ? "picked" : ""}`;
                          return (
                            <div key={side} className={cls} onClick={(e) => change && selectable && toggle(hi, li, e.shiftKey)}>
                              {box(hi, li, l)}
                              <span className="no">{(side === 0 ? l.old_no : l.new_no) ?? ""}</span>
                              <span className="code">{text(l, spans[hi]?.get(li))}</span>
                            </div>
                          );
                        })}
                      </div>
                    ))}
              </div>
            );
          })}
        {dto?.diff?.truncated && <div className="note">Diff truncated — the file is very large.</div>}
      </div>
      {selectable && picked.size > 0 && (
        <div className="selbar">
          <span className="muted">
            {picked.size} line{picked.size === 1 ? "" : "s"} selected · shift-click for a range
          </span>
          <span className="spacer" />
          <button className="btn small" onClick={() => setPicked(new Set())}>Clear</button>
          {actions.onDiscard && (
            <button className="btn small danger" onClick={() => actions.onDiscard!(pickedLines())}>Discard lines</button>
          )}
          <button className="btn small primary" onClick={() => actions.onApply(pickedLines())}>
            {actions.verb} {picked.size} line{picked.size === 1 ? "" : "s"}
          </button>
        </div>
      )}
    </div>
  );
}




function ImageDiff({ pair }: { pair: ImagePair }) {
  const both = !!pair.before && !!pair.after;
  const [mode, setMode] = useState<"side" | "swipe" | "onion">("side");
  const [pos, setPos] = useState(50);
  const size = (n: number | null) => (n === null ? "—" : humanSize(n));
  if (!pair.before && !pair.after) {
    return <div className="note">Image too large to preview ({size(pair.after_size ?? pair.before_size)}).</div>;
  }
  return (
    <div className="imgdiff">
      {both && (
        <div className="modes" role="tablist">
          {(["side", "swipe", "onion"] as const).map((m) => (
            <button key={m} className={mode === m ? "on" : ""} onClick={() => setMode(m)}>
              {m === "side" ? "Side by side" : m === "swipe" ? "Swipe" : "Onion skin"}
            </button>
          ))}
        </div>
      )}
      {(!both || mode === "side") && (
        <div className="sbs" style={both ? undefined : { gridTemplateColumns: "1fr" }}>
          {pair.before && (
            <figure>
              <div className="checker">
                <img src={pair.before} alt="Before" />
              </div>
              <span className="caption">
                <b>Before</b> · {size(pair.before_size)}
              </span>
            </figure>
          )}
          {pair.after && (
            <figure>
              <div className="checker">
                <img src={pair.after} alt="After" />
              </div>
              <span className="caption">
                <b>{pair.before ? "After" : "New file"}</b> · {size(pair.after_size)}
              </span>
            </figure>
          )}
        </div>
      )}
      {both && mode !== "side" && (
        <>
          <div className="checker stack">
            <img src={pair.before!} alt="Before" />
            <img
              src={pair.after!}
              alt="After"
              style={mode === "swipe" ? { clipPath: `inset(0 0 0 ${pos}%)` } : { opacity: pos / 100 }}
            />
            {mode === "swipe" && (
              <div style={{ position: "absolute", top: 0, bottom: 0, left: `${pos}%`, width: 2, background: "var(--accent)" }} />
            )}
          </div>
          <label className="row caption">
            <span>{mode === "swipe" ? "Before" : "Before"}</span>
            <input type="range" min={0} max={100} value={pos} onChange={(e) => setPos(Number(e.target.value))} style={{ flex: 1 }} />
            <span>After</span>
          </label>
        </>
      )}
    </div>
  );
}
