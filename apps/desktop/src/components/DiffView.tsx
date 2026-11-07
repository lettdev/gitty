// Text diff with clickable line selection, image comparison, and the Unity summary.
import { useEffect, useMemo, useState } from "react";
import type { DiffDto, ImagePair } from "../api";
import { humanSize } from "../api";

export type LineKey = `${number}:${number}`;

interface Props {
  title: string;
  sub?: string;
  dto: DiffDto | null;
  loading?: boolean;
  /** When set, change lines are selectable and hunks get action buttons. */
  actions?: {
    verb: string;
    onApply: (lines: [number, number][]) => void;
    onDiscard?: (lines: [number, number][]) => void;
  };
}

export function DiffView({ title, sub, dto, loading, actions }: Props) {
  const [picked, setPicked] = useState<Set<LineKey>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  useEffect(() => {
    setPicked(new Set());
    setAnchor(null);
  }, [dto]);

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
  return (
    <div className="diff">
      <div className="dh">
        <span className="p">{sub}</span>
        <b>{title}</b>
        {loading && <span className="spin" />}
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
        {dto?.diff &&
          !dto.image &&
          dto.diff.hunks.map((h, hi) => (
            <div key={hi}>
              <div className="hunk">
                <span>{h.header}</span>
                {selectable && (
                  <span className="row">
                    {actions.onDiscard && (
                      <button className="btn small danger" onClick={() => actions.onDiscard!(hunkLines(hi))}>
                        Discard hunk
                      </button>
                    )}
                    <button className="btn small" onClick={() => actions.onApply(hunkLines(hi))}>
                      {actions.verb} hunk
                    </button>
                  </span>
                )}
              </div>
              {h.lines.map((l, li) => {
                const change = l.kind === "Add" || l.kind === "Del";
                const on = picked.has(`${hi}:${li}`);
                const cls = `dl ${l.kind === "Add" ? "add" : l.kind === "Del" ? "del" : ""} ${change && selectable ? "change" : ""} ${on ? "picked" : ""}`;
                return (
                  <div key={li} className={cls} onClick={(e) => change && selectable && toggle(hi, li, e.shiftKey)}>
                    <span className="bar" />
                    <span className="no">{l.old_no ?? ""}</span>
                    <span className="no">{l.new_no ?? ""}</span>
                    <span className="sg">{l.kind === "Add" ? "+" : l.kind === "Del" ? "−" : ""}</span>
                    <span>{l.text.replace(/\r$/, "")}</span>
                  </div>
                );
              })}
            </div>
          ))}
        {dto?.diff?.truncated && <div className="note">Diff truncated — the file is very large.</div>}
      </div>
      {selectable && picked.size > 0 && (
        <div className="selbar">
          <span className="muted">
            {picked.size} line{picked.size === 1 ? "" : "s"} selected · shift-click for a range
          </span>
          <span className="spacer" />
          <button className="btn small" onClick={() => setPicked(new Set())}>
            Clear
          </button>
          {actions.onDiscard && (
            <button className="btn small danger" onClick={() => actions.onDiscard!(pickedLines())}>
              Discard lines
            </button>
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
