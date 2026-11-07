// Resolve conflict markers region by region, or take a whole side.
import { useEffect, useState } from "react";
import type { ConflictFile, StatusEntry } from "../api";
import { api, errorText } from "../api";
import type { Ctx } from "./Changes";

export function Conflict({ entry, ctx, fingerprint }: { entry: StatusEntry; ctx: Ctx; fingerprint: string }) {
  const [cf, setCf] = useState<ConflictFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const deleted = entry.index === "D" || entry.worktree === "D";
  const load = () => {
    if (deleted) return;
    api
      .readConflict(entry.path)
      .then((c) => {
        setCf(c);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  };
  useEffect(load, [entry.path, fingerprint]);

  const act = async (op: () => Promise<{ command: string; message: string }>) => {
    if (await ctx.run(op)) load();
  };
  const whole = (side: "ours" | "theirs") => {
    const deletes = (side === "ours" && entry.index === "D") || (side === "theirs" && entry.worktree === "D");
    act(() => api.conflictFileOp(entry.path, deletes ? "delete" : side));
  };
  const remaining = cf?.regions.length ?? 0;

  return (
    <div className="diff">
      <div className="dh">
        <b>{entry.path}</b>
        <span className="p">{deleted ? "deleted on one side" : `${remaining} conflict${remaining === 1 ? "" : "s"} left`}</span>
      </div>
      <div className="ctools">
        <button className="btn small" onClick={() => whole("ours")}>Keep ours for the file</button>
        <button className="btn small" onClick={() => whole("theirs")}>Keep theirs for the file</button>
        {!deleted && cf && cf.regions.some((r) => r.base === null) && (
          <button
            className="btn small"
            title="Re-create the markers with the original version between the sides"
            onClick={async () => {
              if (await ctx.ask("Show the original", "Re-create the conflict markers with the common ancestor shown? Manual edits to this file are replaced (Undo brings them back).", "Show original")) {
                act(() => api.conflictFileOp(entry.path, "base"));
              }
            }}
          >
            Show original
          </button>
        )}
        <button className="btn small primary" disabled={remaining > 0 && !deleted} onClick={() => act(() => api.stage([entry.path]))}>
          Mark resolved
        </button>
      </div>
      <div className="dscroll" style={{ fontFamily: "var(--ui)" }}>
        {deleted && (
          <div className="note">
            This file was deleted by {entry.index === "D" ? "us (HEAD)" : "them"} and changed on the other side. Keep {entry.index === "D" ? "their changed" : "our changed"} version, or delete it.
          </div>
        )}
        {error && <div className="note">{error}</div>}
        {cf && remaining === 0 && !deleted && <div className="note">No conflict markers left. Press “Mark resolved” to stage the file.</div>}
        {cf?.regions.map((r, i) => {
          const ctxBefore = cf.lines.slice(Math.max(0, r.start - 3), r.start);
          const ctxAfter = cf.lines.slice(r.end + 1, r.end + 4);
          const col = (label: string, lines: string[], cls: string, pick: "ours" | "theirs" | "base") => (
            <div className={`cside ${cls}`}>
              <div className="clabel">
                <span>{label}</span>
                <button className="btn small" onClick={() => act(() => api.resolveRegion(entry.path, i, pick))}>Use this</button>
              </div>
              <pre>{lines.length ? lines.join("") : <span className="muted">(nothing)</span>}</pre>
            </div>
          );
          return (
            <div key={i} className="cregion">
              <div className="row" style={{ justifyContent: "space-between" }}>
                <b>Conflict {i + 1} of {remaining}</b>
                <button className="btn small" onClick={() => act(() => api.resolveRegion(entry.path, i, "both"))}>Use both (ours first)</button>
              </div>
              {ctxBefore.length > 0 && <pre className="cctx">{ctxBefore.join("")}</pre>}
              <div className="ccols" style={{ gridTemplateColumns: r.base ? "1fr 1fr 1fr" : "1fr 1fr" }}>
                {col(`Ours · ${r.ours_label || "HEAD"}`, r.ours, "ours", "ours")}
                {r.base && col("Original", r.base, "base", "base")}
                {col(`Theirs · ${r.theirs_label || "incoming"}`, r.theirs, "theirs", "theirs")}
              </div>
              {ctxAfter.length > 0 && <pre className="cctx">{ctxAfter.join("")}</pre>}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Shown above Changes while a merge or rebase is stopped. */
export function OperationBanner({ label, conflicts, ctx }: { label: string; conflicts: number; ctx: Ctx }) {
  const rebasing = label === "REBASING";
  return (
    <div className="opbanner">
      <b>{label.charAt(0) + label.slice(1).toLowerCase()} in progress</b>
      <span className="muted">
        {conflicts > 0 ? `${conflicts} file(s) still conflicted — resolve and mark them, then continue.` : "No conflicts left — continue when ready."}
      </span>
      <span className="spacer" />
      <button
        className="btn small"
        onClick={async () => {
          if (await ctx.ask("Abort", "Stop and go back to where you started? Resolutions made so far are lost.", "Abort")) ctx.run(() => api.operation("abort"));
        }}
      >
        Abort
      </button>
      {rebasing && <button className="btn small" onClick={() => ctx.run(() => api.operation("skip"))}>Skip commit</button>}
      <button className="btn small primary" disabled={conflicts > 0} onClick={() => ctx.run(() => api.operation("continue"))}>
        Continue
      </button>
    </div>
  );
}
