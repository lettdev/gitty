import { useEffect, useMemo, useState } from "react";
import type { Blob, FileType, Fix, Health, Insight as InsightData, SizeNode } from "../api";
import { api, errorText, humanSize } from "../api";
import type { Ctx } from "./Changes";

const TYPE_COLORS: Record<FileType, string> = {
  Code: "var(--info)",
  Images: "var(--orange)",
  Audio: "var(--violet)",
  Video: "var(--del)",
  Models: "var(--cyan)",
  EngineAssets: "var(--accent)",
  Fonts: "var(--mod)",
  Docs: "#6b8fb5",
  Binaries: "#b5566b",
  Other: "var(--muted)",
};
const TYPE_NAMES: Record<FileType, string> = {
  Code: "Code",
  Images: "Images & textures",
  Audio: "Audio",
  Video: "Video",
  Models: "3D models",
  EngineAssets: "Scenes & engine assets",
  Fonts: "Fonts",
  Docs: "Docs & data",
  Binaries: "Binaries & archives",
  Other: "Other",
};
// tile colors for the treemap (solid, readable with white text in both themes)
const TILE: Record<FileType, string> = {
  Code: "#3b6fb0",
  Images: "#b05a2a",
  Audio: "#6a4fb3",
  Video: "#a33a3a",
  Models: "#2f7f8a",
  EngineAssets: "#2f7d57",
  Fonts: "#8a6d1f",
  Docs: "#4d6680",
  Binaries: "#7d3f56",
  Other: "#56605b",
};
const EXT_TYPE: [FileType, string[]][] = [
  ["Code", ["cs", "rs", "ts", "tsx", "js", "jsx", "py", "go", "gd", "cpp", "cc", "h", "hpp", "c", "swift", "kt", "java", "dart", "shader", "hlsl", "cginc", "compute", "glsl", "lua", "rb", "php", "vue", "svelte", "html", "css", "scss", "sh", "sql"]],
  ["Images", ["png", "jpg", "jpeg", "psd", "psb", "tga", "tif", "tiff", "exr", "hdr", "bmp", "gif", "webp", "svg", "kra", "dds", "ico"]],
  ["Audio", ["wav", "mp3", "ogg", "aif", "aiff", "flac", "m4a", "bank", "wem"]],
  ["Video", ["mp4", "mov", "webm", "avi", "mkv"]],
  ["Models", ["fbx", "obj", "blend", "max", "ma", "mb", "glb", "gltf", "dae", "usd", "abc"]],
  ["EngineAssets", ["unity", "prefab", "asset", "mat", "anim", "controller", "meta", "tscn", "tres", "umap", "uasset"]],
  ["Fonts", ["ttf", "otf", "woff", "woff2"]],
  ["Docs", ["md", "txt", "json", "yaml", "yml", "xml", "csv", "toml", "ini", "cfg", "pdf", "lock"]],
  ["Binaries", ["dll", "so", "dylib", "exe", "a", "lib", "apk", "zip", "7z", "rar", "jar", "bin", "pak", "wasm"]],
];
function typeOf(path: string): FileType {
  const ext = path.split("/").pop()?.split(".").pop()?.toLowerCase() ?? "";
  return EXT_TYPE.find(([, exts]) => exts.includes(ext))?.[0] ?? "Other";
}
/** The type that dominates a folder, for coloring its tile. */
function dominant(node: SizeNode): FileType {
  const acc = new Map<FileType, number>();
  const walk = (n: SizeNode, depth: number) => {
    if (n.children.length === 0) acc.set(typeOf(n.path), (acc.get(typeOf(n.path)) ?? 0) + n.size);
    else if (depth < 6) n.children.forEach((c) => walk(c, depth + 1));
  };
  walk(node, 0);
  return [...acc.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "Other";
}

interface Rect {
  node: SizeNode;
  x: number;
  y: number;
  w: number;
  h: number;
}
function squarify(nodes: SizeNode[], W: number, H: number): Rect[] {
  const items = nodes.filter((n) => n.size > 0);
  const out: Rect[] = [];
  let [x, y, w, h] = [0, 0, W, H];
  let rest = items.slice();
  while (rest.length) {
    const total = rest.reduce((s, n) => s + n.size, 0);
    const scale = (w * h) / total;
    const short = Math.min(w, h);
    const worst = (row: SizeNode[]) => {
      const areas = row.map((n) => n.size * scale);
      const s = areas.reduce((a, b) => a + b, 0);
      return Math.max((short * short * Math.max(...areas)) / (s * s), (s * s) / (short * short * Math.min(...areas)));
    };
    const row = [rest[0]];
    let i = 1;
    while (i < rest.length && worst([...row, rest[i]]) <= worst(row)) row.push(rest[i++]);
    const s = row.reduce((a, n) => a + n.size * scale, 0);
    if (w >= h) {
      const cw = s / h;
      let cy = y;
      row.forEach((n) => {
        const ch = (n.size * scale) / cw;
        out.push({ node: n, x, y: cy, w: cw, h: ch });
        cy += ch;
      });
      x += cw;
      w -= cw;
    } else {
      const ch = s / w;
      let cx = x;
      row.forEach((n) => {
        const cw = (n.size * scale) / ch;
        out.push({ node: n, x: cx, y, w: cw, h: ch });
        cx += cw;
      });
      y += ch;
      h -= ch;
    }
    rest = rest.slice(i);
  }
  return out;
}

function find(node: SizeNode, path: string): SizeNode | null {
  if (node.path === path) return node;
  for (const c of node.children) if (path.startsWith(c.path) && c.path) {
    const f = find(c, path);
    if (f) return f;
  }
  return null;
}

export function Insight({ ctx, fingerprint, onGotoCommit }: { ctx: Ctx; fingerprint: string; onGotoCommit: (hash: string) => void }) {
  const [data, setData] = useState<InsightData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [blobs, setBlobs] = useState<Blob[] | null>(null);
  const [path, setPath] = useState("");
  const load = (force = false) => {
    api.insight().then(setData).catch((e) => setError(errorText(e)));
    setBlobs(null);
    api.largestBlobs(force).then(setBlobs).catch(() => setBlobs([]));
  };
  useEffect(() => load(), [fingerprint]);

  const node = useMemo(() => (data ? find(data.tree, path) ?? data.tree : null), [data, path]);
  const tiles = useMemo(() => (node ? squarify(node.children.slice(0, 60), 160, 90) : []), [node]);

  if (error) return <div className="empty"><b>Insight failed</b>{error}</div>;
  if (!data) return <div className="empty"><span className="spin" /> Measuring the repository…</div>;

  const typesTotal = Math.max(1, data.types.reduce((s, t) => s + t.bytes, 0));
  const crumbs = path ? path.replace(/\/$/, "").split("/") : [];
  const fix = async (h: Health) => {
    if (!h.fix) return;
    const [kind, body] = Object.entries(h.fix)[0] as [keyof Fix & string, Record<string, string[] | string>];
    if (kind === "LfsMigrate") {
      await ctx.ask("Rewrite history", "Moving old versions into LFS rewrites every branch. Run it from the terminal app (gitty → Insight → Health) so you can see the progress.", "OK");
      return;
    }
    const msg = kind === "Ignore" ? `Add ${body.pattern} to .gitignore and stop tracking matching files?` : `Track ${(body.patterns as string[]).join(" ")} with LFS and convert the current files?`;
    if (await ctx.ask(h.title, `${msg}\nThe change is staged for you to commit. Undo reverts it.`, "Fix")) {
      if (await ctx.run(() => api.applyFix(body))) load();
    }
  };

  return (
    <div className="insight">
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
        <div className="card">
          <h4>Project</h4>
          {data.projects.length === 0 && <span className="muted">No known project type detected.</span>}
          {data.projects.slice(0, 3).map((p) => (
            <div key={p.marker}>
              <div>
                <b style={{ color: "var(--accent)" }}>{p.kind}</b> {p.version && <b>{p.version}</b>}{" "}
                {p.dir && <span className="muted">in {p.dir}</span>}
              </div>
              <div className="muted" style={{ fontSize: 12 }}>{p.details.join(" · ")}</div>
            </div>
          ))}
          <div className="stats">
            <div className="stat"><span className="v">{humanSize(data.size.worktree)}</span><span className="l">Working tree</span></div>
            <div className="stat"><span className="v">{humanSize(data.size.git_dir)}</span><span className="l">.git</span></div>
            <div className="stat"><span className="v">{data.size.worktree_files.toLocaleString()}</span><span className="l">Files</span></div>
          </div>
        </div>
        <div className="card">
          <h4>Composition <small>by file type</small></h4>
          <div className="stacked">
            {data.types.map((t) => (
              <span key={t.kind ?? "x"} title={`${TYPE_NAMES[t.kind ?? "Other"]} · ${humanSize(t.bytes)}`} style={{ width: `${(t.bytes / typesTotal) * 100}%`, background: TYPE_COLORS[t.kind ?? "Other"] }} />
            ))}
          </div>
          <div className="legend">
            {data.types.map((t) => {
              const pct = (t.bytes / typesTotal) * 100;
              return (
                <div key={t.kind ?? "x"} style={{ display: "contents" }}>
                  <i style={{ background: TYPE_COLORS[t.kind ?? "Other"] }} />
                  <span>
                    {TYPE_NAMES[t.kind ?? "Other"]} <span className="muted">{t.top.map(([e]) => `.${e}`).join(" ")}</span>
                  </span>
                  <span className="n">{humanSize(t.bytes)}</span>
                  <span className="n muted">{pct < 1 && t.bytes > 0 ? "<1%" : `${pct.toFixed(0)}%`}</span>
                </div>
              );
            })}
          </div>
        </div>
        <div className="card">
          <h4>Health <small>{data.health.filter((h) => h.severity === "Warn").length} to fix</small></h4>
          <div className="health">
            {data.health.length === 0 && <span className="muted">Nothing to report.</span>}
            {data.health.map((h) => (
              <div key={h.title} className="hi">
                <span className={`ic ${h.severity}`}>{h.severity === "Warn" ? "!" : h.severity === "Info" ? "i" : "✓"}</span>
                <div>
                  <div className="row">
                    <b style={{ flex: 1, fontWeight: 500 }}>{h.title}</b>
                    {h.fix && !("LfsMigrate" in h.fix) && <button className="btn small" onClick={() => fix(h)}>Fix</button>}
                  </div>
                  {h.detail && <p>{h.detail}</p>}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
        <div className="card">
          <h4>
            Size
            <span className="crumbs">
              <button onClick={() => setPath("")}>{node === data.tree ? "repo root" : "root"}</button>
              {crumbs.map((c, i) => (
                <span key={i}>
                  {" / "}
                  <button onClick={() => setPath(crumbs.slice(0, i + 1).join("/") + "/")}>{c}</button>
                </span>
              ))}
            </span>
          </h4>
          <div className="tm">
            {tiles.map((r) => {
              const isDir = r.node.children.length > 0;
              const t = isDir ? dominant(r.node) : typeOf(r.node.path);
              const big = r.w * r.h > 120;
              return (
                <div
                  key={r.node.path}
                  className="tmc"
                  title={`${r.node.path} · ${humanSize(r.node.size)}${isDir ? ` · ${r.node.files} files` : ""}`}
                  onClick={() => isDir && setPath(r.node.path)}
                  style={{ left: `${(r.x / 160) * 100}%`, top: `${(r.y / 90) * 100}%`, width: `${(r.w / 160) * 100}%`, height: `${(r.h / 90) * 100}%`, background: TILE[t], cursor: isDir ? "pointer" : "default" }}
                >
                  {big && <><b>{r.node.name}{isDir ? "/" : ""}</b><span>{humanSize(r.node.size)}</span></>}
                </div>
              );
            })}
          </div>
          <span className="muted" style={{ fontSize: 12 }}>Click a folder to look inside. Colors follow the file type that dominates each tile.</span>
        </div>
        <div className="card">
          <h4>
            Largest in history <small>{blobs === null ? "scanning…" : "including deleted files"}</small>
            <button className="btn small" onClick={() => load(true)}>Rescan</button>
          </h4>
          {blobs === null ? (
            <span className="muted"><span className="spin" /> Reading every version of every file…</span>
          ) : (
            <table className="tbl">
              <thead><tr><th>File</th><th>Size</th><th>Commit</th><th>LFS</th></tr></thead>
              <tbody>
                {blobs.slice(0, 15).map((b) => (
                  <tr key={b.oid}>
                    <td className="m">{b.path}{b.versions > 1 && <span className="muted"> ×{b.versions}</span>}</td>
                    <td>{humanSize(b.size)}</td>
                    <td>{b.commit && <button className="link" onClick={() => onGotoCommit(b.commit!)}>{b.commit.slice(0, 7)}</button>}</td>
                    <td>{b.lfs ? <span className="tag ok">LFS</span> : b.size > 1024 * 1024 ? <span className="tag warn">no</span> : ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div className="card">
          <h4>Hot files <small>most changed in 90 days</small></h4>
          {data.hot.length === 0 && <span className="muted">No commits in the last 90 days.</span>}
          <table className="tbl">
            <tbody>
              {data.hot.slice(0, 10).map((h) => (
                <tr key={h.path}>
                  <td style={{ width: 120 }}>
                    <div style={{ height: 6, borderRadius: 3, background: "var(--orange)", width: `${(h.commits / data.hot[0].commits) * 100}%` }} />
                  </td>
                  <td className="n" style={{ width: 30 }}>{h.commits}</td>
                  <td className="m">{h.path}</td>
                  <td className="muted">{h.last_author}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
