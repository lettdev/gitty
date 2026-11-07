// Draws one history row from gitty-core's graph cells (the same box-drawing
// grid the terminal UI prints) as smooth SVG lines.
import type { Cell } from "../api";

export const CELL_W = 8;
export const ROW_H = 30;
const LANES = ["var(--accent)", "var(--violet)", "var(--orange)", "var(--info)", "var(--cyan)", "var(--del)"];

const UP = new Set(["│", "●", "╯", "╰", "┤", "├", "┼", "┴"]);
const DOWN = new Set(["│", "●", "╮", "╭", "┤", "├", "┼", "┬"]);
const SIDE = new Set(["─", "╮", "╯", "╭", "╰", "┤", "├", "┼", "┴", "┬"]);

export function GraphRow({ cells, prev, next, head, width }: { cells: Cell[]; prev?: Cell[]; next?: Cell[]; head: boolean; width: number }) {
  const h = ROW_H;
  const m = h / 2;
  const paths: { d: string; c: string }[] = [];
  let dot: { x: number; c: string } | null = null;
  cells.forEach((cell, i) => {
    const x0 = i * CELL_W;
    const x1 = x0 + CELL_W;
    const cx = x0 + CELL_W / 2;
    const c = LANES[cell.lane % LANES.length];
    const v = `M${cx} 0 L${cx} ${h}`;
    const hz = `M${x0} ${m} L${x1} ${m}`;
    switch (cell.ch) {
      case "│": paths.push({ d: v, c }); break;
      case "─": paths.push({ d: hz, c }); break;
      case "╮": paths.push({ d: `M${x0} ${m} Q${cx} ${m} ${cx} ${h}`, c }); break;
      case "╯": paths.push({ d: `M${cx} 0 Q${cx} ${m} ${x0} ${m}`, c }); break;
      case "╭": paths.push({ d: `M${x1} ${m} Q${cx} ${m} ${cx} ${h}`, c }); break;
      case "╰": paths.push({ d: `M${cx} 0 Q${cx} ${m} ${x1} ${m}`, c }); break;
      case "┤": paths.push({ d: v, c }, { d: `M${x0} ${m} L${cx} ${m}`, c }); break;
      case "├": paths.push({ d: v, c }, { d: `M${cx} ${m} L${x1} ${m}`, c }); break;
      case "┼": paths.push({ d: v, c }, { d: hz, c }); break;
      case "┴": paths.push({ d: hz, c }, { d: `M${cx} 0 L${cx} ${m}`, c }); break;
      case "┬": paths.push({ d: hz, c }, { d: `M${cx} ${m} L${cx} ${h}`, c }); break;
      case "●": {
        if (prev && DOWN.has(prev[i]?.ch)) paths.push({ d: `M${cx} 0 L${cx} ${m}`, c });
        if (next && UP.has(next[i]?.ch)) paths.push({ d: `M${cx} ${m} L${cx} ${h}`, c });
        if (SIDE.has(cells[i + 1]?.ch)) paths.push({ d: `M${cx} ${m} L${x1} ${m}`, c: LANES[cells[i + 1].lane % LANES.length] });
        if (SIDE.has(cells[i - 1]?.ch)) paths.push({ d: `M${x0} ${m} L${cx} ${m}`, c: LANES[cells[i - 1].lane % LANES.length] });
        dot = { x: cx, c };
        break;
      }
    }
  });
  const d = dot as { x: number; c: string } | null;
  return (
    <svg width={width} height={h} style={{ flex: "none", display: "block" }} aria-hidden="true">
      {paths.map((p, i) => (
        <path key={i} d={p.d} stroke={p.c} strokeWidth={2} fill="none" strokeLinecap="round" />
      ))}
      {d &&
        (head ? (
          <circle cx={d.x} cy={m} r={5.5} fill="var(--bg)" stroke={d.c} strokeWidth={2.5} />
        ) : (
          <circle cx={d.x} cy={m} r={4.5} fill={d.c} stroke="var(--bg)" strokeWidth={1.5} />
        ))}
    </svg>
  );
}
