// Interactive rebase: drag commits to reorder, pick an action per commit,
// preview the result, then run it.
import { useState } from "react";
import type { RebaseAction, RebaseStep } from "../api";

const ACTIONS: { a: RebaseAction; label: string; key: string; hint: string }[] = [
  { a: "Pick", label: "Pick", key: "p", hint: "keep as is" },
  { a: "Reword", label: "Reword", key: "r", hint: "keep, new message" },
  { a: "Squash", label: "Squash", key: "s", hint: "merge into the commit below, keep both messages" },
  { a: "Fixup", label: "Fixup", key: "f", hint: "merge into the commit below, drop this message" },
  { a: "Drop", label: "Drop", key: "d", hint: "remove the commit" },
];

/** What the branch looks like afterwards, newest first. */
function preview(steps: RebaseStep[]): string[] {
  const out: string[] = [];
  for (const s of steps) {
    if (s.action === "Drop") continue;
    if ((s.action === "Squash" || s.action === "Fixup") && out.length) {
      if (!out[out.length - 1].endsWith(" (+ squashed)")) out[out.length - 1] += " (+ squashed)";
      continue;
    }
    out.push(s.action === "Reword" && s.message ? s.message.split("\n")[0] : s.subject);
  }
  return out.reverse();
}

function problem(steps: RebaseStep[]): string | null {
  const first = steps.find((s) => s.action !== "Drop");
  if (!first) return "Every commit is dropped — that would empty the branch.";
  if (first.action === "Squash" || first.action === "Fixup") return "The oldest kept commit has nothing below it to squash into.";
  if (steps.some((s) => s.action === "Reword" && !s.message?.trim())) return "A reworded commit needs a message.";
  return null;
}

export function Rebase({ initial, onCancel, onRun }: { initial: RebaseStep[]; onCancel: () => void; onRun: (steps: RebaseStep[]) => void }) {
  // shown newest first; git wants oldest first
  const [rows, setRows] = useState<RebaseStep[]>(() => initial.slice().reverse());
  const [sel, setSel] = useState(0);
  const [drag, setDrag] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const [editing, setEditing] = useState<number | null>(null);

  const steps = rows.slice().reverse();
  const changed = rows.some((r, i) => r.action !== "Pick" || r.hash !== initial[initial.length - 1 - i].hash);
  const issue = problem(steps);
  const result = preview(steps);
  const pushed = rows.filter((r) => r.pushed).length;

  const set = (i: number, patch: Partial<RebaseStep>) => setRows((rs) => rs.map((r, k) => (k === i ? { ...r, ...patch } : r)));
  const move = (from: number, to: number) => {
    if (from === to || to < 0 || to >= rows.length) return;
    setRows((rs) => {
      const next = rs.slice();
      const [item] = next.splice(from, 1);
      next.splice(to, 0, item);
      return next;
    });
    setSel(to);
  };

  const onKey = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).closest("input, textarea")) return;
    const k = e.key;
    if (k === "ArrowDown" || k === "j") {
      if (e.shiftKey) move(sel, sel + 1);
      else setSel((s) => Math.min(rows.length - 1, s + 1));
    } else if (k === "ArrowUp" || k === "k") {
      if (e.shiftKey) move(sel, sel - 1);
      else setSel((s) => Math.max(0, s - 1));
    } else if (k === "J") move(sel, sel + 1);
    else if (k === "K") move(sel, sel - 1);
    else {
      const act = ACTIONS.find((a) => a.key === k);
      if (!act) return;
      set(sel, { action: act.a, message: act.a === "Reword" ? rows[sel].message ?? rows[sel].subject : rows[sel].message });
      if (act.a === "Reword") setEditing(sel);
    }
    e.preventDefault();
  };

  return (
    <div className="overlay" onClick={onCancel}>
      <div className="dialog rebase" onClick={(e) => e.stopPropagation()} onKeyDown={onKey} tabIndex={-1} ref={(el) => el?.focus()} role="dialog" aria-label="Interactive rebase">
        <div className="body2">
          <h3>Interactive rebase · {rows.length} commits</h3>
          <p>Drag commits to reorder them (newest at the top). Choose what happens to each one.</p>
        </div>
        <div className="rb-grid">
          <div className="rb-list">
            {rows.map((r, i) => (
              <div
                key={r.hash}
                className={`rb-row ${i === sel ? "sel" : ""} ${drag === i ? "dragging" : ""} ${over === i && drag !== null && drag !== i ? (drag < i ? "over-below" : "over-above") : ""} a-${r.action}`}
                draggable
                onDragStart={(e) => {
                  setDrag(i);
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", String(i));
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  setOver(i);
                }}
                onDragLeave={() => setOver((o) => (o === i ? null : o))}
                onDrop={(e) => {
                  e.preventDefault();
                  if (drag !== null) move(drag, i);
                  setDrag(null);
                  setOver(null);
                }}
                onDragEnd={() => {
                  setDrag(null);
                  setOver(null);
                }}
                onClick={() => setSel(i)}
              >
                <span className="grip" aria-hidden="true">⠿</span>
                <select
                  value={r.action}
                  onChange={(e) => {
                    const a = e.target.value as RebaseAction;
                    set(i, { action: a, message: a === "Reword" ? r.message ?? r.subject : r.message });
                    if (a === "Reword") setEditing(i);
                  }}
                  aria-label={`Action for ${r.hash.slice(0, 7)}`}
                >
                  {ACTIONS.map((a) => (
                    <option key={a.a} value={a.a} title={a.hint}>
                      {a.label}
                    </option>
                  ))}
                </select>
                <span className="mono muted">{r.hash.slice(0, 7)}</span>
                {editing === i ? (
                  <input
                    className="field"
                    autoFocus
                    value={r.message ?? ""}
                    onChange={(e) => set(i, { message: e.target.value })}
                    onBlur={() => setEditing(null)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === "Escape") setEditing(null);
                      e.stopPropagation();
                    }}
                  />
                ) : (
                  <span className="rb-subject" onDoubleClick={() => r.action === "Reword" && setEditing(i)}>
                    {r.action === "Reword" && r.message ? r.message : r.subject}
                    {r.action === "Reword" && r.message && r.message !== r.subject && <span className="muted"> (was: {r.subject})</span>}
                  </span>
                )}
                {r.pushed && <span className="tag warn">pushed</span>}
              </div>
            ))}
          </div>
          <div className="rb-preview">
            <div className="gh" style={{ padding: "0 0 6px" }}>
              <span>After · {result.length} commit{result.length === 1 ? "" : "s"}</span>
            </div>
            {result.map((s, i) => (
              <div key={i} className="rb-after">
                <span className="dotc" />
                {s}
              </div>
            ))}
            <div className="muted" style={{ fontSize: 12, marginTop: 10 }}>
              Keys: ↑↓ select · J/K or ⇧↑↓ move · p r s f d set action
            </div>
          </div>
        </div>
        {(issue || pushed > 0) && (
          <div className="body2" style={{ paddingTop: 0 }}>
            {issue && <p style={{ color: "var(--del)" }}>{issue}</p>}
            {pushed > 0 && <p style={{ color: "var(--orange)" }}>{pushed} commit(s) are already pushed — you'll need to force-push afterwards.</p>}
          </div>
        )}
        <div className="actions">
          <span className="muted" style={{ marginRight: "auto", fontSize: 12 }}>
            Uncommitted changes are stashed and restored. Undo reverts the whole rebase.
          </span>
          <button className="btn" onClick={onCancel}>Cancel</button>
          <button className="btn primary" disabled={!!issue || !changed} onClick={() => onRun(steps)}>
            Rebase
          </button>
        </div>
      </div>
    </div>
  );
}
