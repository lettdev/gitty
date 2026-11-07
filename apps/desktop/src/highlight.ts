// Lightweight syntax highlighting and intra-line ("word") diff for the diff view.
// Deliberately small: a per-language keyword list plus a shared tokenizer is
// enough to make diffs readable without shipping a full grammar engine.

export type TokKind = "kw" | "ty" | "str" | "num" | "com" | "fn" | "plain";
export interface Tok {
  t: string;
  k: TokKind;
}

const KW: Record<string, string[]> = {
  c: ["abstract", "as", "async", "await", "base", "break", "case", "catch", "class", "const", "continue", "default", "delegate", "do", "else", "enum", "event", "explicit", "extern", "false", "finally", "fixed", "for", "foreach", "get", "goto", "if", "implicit", "in", "interface", "internal", "is", "lock", "namespace", "new", "null", "operator", "out", "override", "params", "private", "protected", "public", "readonly", "ref", "return", "sealed", "set", "sizeof", "static", "struct", "switch", "this", "throw", "true", "try", "typeof", "unsafe", "using", "var", "virtual", "void", "volatile", "while", "yield", "int", "float", "double", "bool", "string", "char", "byte", "long", "short", "uint", "object", "decimal", "include", "define", "auto", "nullptr", "template", "typename", "export", "import", "from", "function", "let", "of", "type", "extends", "implements", "package", "func", "go", "defer", "chan", "map", "range", "select", "fn", "mut", "impl", "trait", "pub", "use", "mod", "crate", "self", "Self", "match", "loop", "where", "dyn", "move", "unsafe", "def", "elif", "lambda", "pass", "raise", "with", "yield", "not", "and", "or", "None", "True", "False", "extends", "signal", "onready", "export", "func", "var", "const", "preload"],
};
const LANG_BY_EXT: Record<string, string> = {};
for (const e of ["cs", "java", "kt", "js", "jsx", "ts", "tsx", "mjs", "cjs", "go", "rs", "c", "cc", "cpp", "h", "hpp", "swift", "dart", "py", "gd", "shader", "hlsl", "cginc", "glsl", "php", "lua", "rb", "scala"]) LANG_BY_EXT[e] = "c";
const HASH_COMMENT = new Set(["py", "gd", "rb", "sh", "bash", "zsh", "yaml", "yml", "toml", "ini", "cfg", "gitignore", "dockerfile", "make", "mk"]);
const DATA = new Set(["json", "yaml", "yml", "toml", "ini", "cfg", "xml", "unity", "prefab", "asset", "mat", "meta"]);

export function langOf(path: string): { kw: Set<string> | null; hash: boolean; data: boolean } {
  const name = path.split("/").pop() ?? path;
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : name.toLowerCase();
  const kwList = LANG_BY_EXT[ext] ? KW[LANG_BY_EXT[ext]] : null;
  return { kw: kwList ? new Set(kwList) : null, hash: HASH_COMMENT.has(ext) || name === ".gitignore", data: DATA.has(ext) };
}

/** Tokenize one line. State across lines (block comments) is ignored on purpose. */
export function tokenize(line: string, lang: ReturnType<typeof langOf>): Tok[] {
  if (!lang.kw && !lang.hash && !lang.data) return [{ t: line, k: "plain" }];
  const out: Tok[] = [];
  let i = 0;
  const push = (t: string, k: TokKind) => {
    if (!t) return;
    const last = out[out.length - 1];
    if (last && last.k === k) last.t += t;
    else out.push({ t, k });
  };
  while (i < line.length) {
    const rest = line.slice(i);
    // comments
    if ((lang.kw && rest.startsWith("//")) || (lang.hash && rest.startsWith("#")) || rest.startsWith("/*")) {
      push(rest, "com");
      break;
    }
    const ch = line[i];
    // strings
    if (ch === '"' || ch === "'" || ch === "`") {
      let j = i + 1;
      while (j < line.length && line[j] !== ch) j += line[j] === "\\" ? 2 : 1;
      push(line.slice(i, j + 1), "str");
      i = j + 1;
      continue;
    }
    // numbers
    const num = /^(0x[0-9a-fA-F]+|\d+(\.\d+)?[fFdDmMuUlL]?)/.exec(rest);
    if (num && !/[A-Za-z_]/.test(line[i - 1] ?? "")) {
      push(num[0], "num");
      i += num[0].length;
      continue;
    }
    // identifiers
    const id = /^[A-Za-z_][A-Za-z0-9_]*/.exec(rest);
    if (id) {
      const w = id[0];
      const after = line.slice(i + w.length).trimStart();
      let k: TokKind = "plain";
      if (lang.data) {
        // YAML/TOML keys: `name:` or `name =`
        if (/^[:=]/.test(after) && out.every((t) => !t.t.trim() || t.k === "plain")) k = "kw";
        else if (w === "true" || w === "false" || w === "null") k = "num";
      } else if (lang.kw?.has(w)) k = "kw";
      else if (after.startsWith("(")) k = "fn";
      else if (/^[A-Z]/.test(w)) k = "ty";
      push(w, k);
      i += w.length;
      continue;
    }
    push(ch, "plain");
    i++;
  }
  return out;
}

/** Changed span of `a` vs `b`: [start, endExclusive] after trimming the common prefix and suffix. */
export function changedSpan(a: string, b: string): [number, number] | null {
  if (a === b) return null;
  let p = 0;
  const n = Math.min(a.length, b.length);
  while (p < n && a[p] === b[p]) p++;
  let s = 0;
  while (s < n - p && a[a.length - 1 - s] === b[b.length - 1 - s]) s++;
  // expand to word boundaries so highlights don't cut words in half
  const word = /[A-Za-z0-9_]/;
  while (p > 0 && word.test(a[p - 1]) && word.test(a[p] ?? "")) p--;
  let end = a.length - s;
  while (end < a.length && word.test(a[end]) && word.test(a[end - 1] ?? "")) end++;
  // a mostly-different line isn't worth highlighting inside
  if (end - p > a.length * 0.7 && a.length > 12) return null;
  return [p, end];
}

/** Split tokens so the range [start, end) can be wrapped in a highlight. */
export function withSpan(toks: Tok[], span: [number, number] | null): { t: string; k: TokKind; hl: boolean }[] {
  const out: { t: string; k: TokKind; hl: boolean }[] = [];
  let pos = 0;
  for (const tok of toks) {
    const a = pos;
    const b = pos + tok.t.length;
    pos = b;
    if (!span || b <= span[0] || a >= span[1]) {
      out.push({ ...tok, hl: false });
      continue;
    }
    const cut1 = Math.max(span[0], a) - a;
    const cut2 = Math.min(span[1], b) - a;
    if (cut1 > 0) out.push({ t: tok.t.slice(0, cut1), k: tok.k, hl: false });
    out.push({ t: tok.t.slice(cut1, cut2), k: tok.k, hl: true });
    if (cut2 < tok.t.length) out.push({ t: tok.t.slice(cut2), k: tok.k, hl: false });
  }
  return out;
}
