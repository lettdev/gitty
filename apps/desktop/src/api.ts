// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

export type EntryKind = "Ordinary" | "Renamed" | "Unmerged" | "Untracked";
export interface StatusEntry {
  path: string;
  orig_path: string | null;
  index: string;
  worktree: string;
  kind: EntryKind;
}
export interface BranchStatus {
  head: string | null;
  oid: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
}
export interface Stash {
  name: string;
  oid: string;
  time: number;
  message: string;
}
export interface StatusDto {
  status: { branch: BranchStatus; entries: StatusEntry[] };
  state: string;
  state_label: string | null;
  stashes: Stash[];
  last_op: string | null;
  fingerprint: string;
  remotes: string[];
}

export type LineKind = "Context" | "Add" | "Del" | "NoNewline";
export interface DiffLine {
  kind: LineKind;
  text: string;
  old_no: number | null;
  new_no: number | null;
}
export interface Hunk {
  header: string;
  old_start: number;
  new_start: number;
  lines: DiffLine[];
}
export interface FileDiff {
  header: string[];
  hunks: Hunk[];
  binary: boolean;
  truncated: boolean;
}
export interface ImagePair {
  before: string | null;
  after: string | null;
  before_size: number | null;
  after_size: number | null;
}
export interface DiffDto {
  diff: FileDiff | null;
  summary: string[];
  image: ImagePair | null;
  note: string | null;
  raw: string | null;
  patchable: boolean;
}

export type RefKind = "Head" | "Local" | "Remote" | "Tag";
export interface Commit {
  hash: string;
  parents: string[];
  author: string;
  email: string;
  time: number;
  refs: { kind: RefKind; name: string }[];
  subject: string;
}
export interface Cell {
  ch: string;
  lane: number;
}
export interface LogRow {
  commit: Commit;
  cells: Cell[];
}
export interface CommitFile {
  status: string;
  path: string;
  orig: string | null;
  adds: number | null;
  dels: number | null;
}
export interface CommitDetail {
  hash: string;
  parents: string[];
  author: string;
  email: string;
  date: string;
  message: string;
  files: CommitFile[];
}
export interface Branch {
  name: string;
  full: string;
  remote: boolean;
  head: boolean;
  oid: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  gone: boolean;
  time: number;
  subject: string;
}

export type FileType =
  | "Code" | "Images" | "Audio" | "Video" | "Models" | "EngineAssets" | "Fonts" | "Docs" | "Binaries" | "Other";
export interface SizeNode {
  name: string;
  path: string;
  size: number;
  files: number;
  children: SizeNode[];
}
export type Fix =
  | { Ignore: { pattern: string; untrack: string[] } }
  | { LfsTrack: { patterns: string[]; convert: string[] } }
  | { LfsMigrate: { patterns: string[] } };
export interface Health {
  severity: "Warn" | "Info" | "Ok";
  title: string;
  detail: string;
  fix: Fix | null;
}
export interface Insight {
  projects: { kind: string; dir: string; version: string | null; details: string[]; marker: string }[];
  languages: { name: string; files: number; bytes: number }[];
  types: { kind: FileType | null; bytes: number; files: number; top: [string, number][] }[];
  size: { worktree: number; worktree_files: number; git_dir: number; packed: number; loose: number; lfs_store: number };
  tree: SizeNode;
  health: Health[];
  hot: { path: string; commits: number; last_author: string }[];
  owners: { dir: string; author: string; share: number; commits: number }[];
  lfs_installed: boolean;
}
export interface Blob {
  oid: string;
  path: string;
  size: number;
  versions: number;
  lfs: boolean;
  commit: string | null;
}
export interface RepoInfo {
  name: string;
  root: string;
}
export interface RepoSummary {
  path: string;
  name: string;
  branch: string | null;
  changes: number;
  conflicts: number;
  upstream: boolean;
  ahead: number;
  behind: number;
  last_commit: number | null;
  error: string | null;
}
export interface OpResult {
  command: string;
  message: string;
}

export type RebaseAction = "Pick" | "Reword" | "Squash" | "Fixup" | "Drop";
export interface RebaseStep {
  hash: string;
  subject: string;
  action: RebaseAction;
  message: string | null;
  pushed: boolean;
}
export interface ConflictRegion {
  start: number;
  end: number;
  ours: string[];
  base: string[] | null;
  theirs: string[];
  ours_label: string;
  theirs_label: string;
}
export interface ConflictFile {
  lines: string[];
  regions: ConflictRegion[];
}

export interface IgnoreRule {
  line: number;
  text: string;
  comment: boolean;
}
export interface IgnoreSource {
  label: string;
  rel: string | null;
  abs: string;
  exists: boolean;
  rules: IgnoreRule[];
}
export interface WhyIgnored {
  source: string;
  line: number;
  pattern: string;
  negated: boolean;
}

export type Section = "conflicts" | "unstaged" | "staged";

export const api = {
  initialRepo: () => invoke<RepoInfo | null>("initial_repo"),
  startView: () => invoke<string | null>("start_view"),
  openRepo: (path: string) => invoke<RepoInfo>("open_repo", { path }),
  repoList: () => invoke<RepoSummary[]>("repo_list"),
  status: () => invoke<StatusDto>("status"),
  fileDiff: (path: string, section: Section, orig: string | null) => invoke<DiffDto>("file_diff", { path, section, orig }),
  commitFileDiff: (hash: string, parent: string | null, file: CommitFile) =>
    invoke<DiffDto>("commit_file_diff", { hash, parent, path: file.path, orig: file.orig, status: file.status }),
  stage: (paths: string[]) => invoke<OpResult>("stage", { paths }),
  unstage: (paths: string[]) => invoke<OpResult>("unstage", { paths }),
  stageAll: (stage: boolean) => invoke<OpResult>("stage_all", { stage }),
  applyLines: (path: string, section: Section, lines: [number, number][], discard: boolean) =>
    invoke<OpResult>("apply_lines", { path, section, lines, discard }),
  discardFile: (e: StatusEntry, section: Section) =>
    invoke<OpResult>("discard_file", { path: e.path, section, orig: e.orig_path, indexStatus: e.index }),
  commit: (message: string, amend: boolean) => invoke<OpResult>("commit", { message, amend }),
  lastMessage: () => invoke<string>("last_message"),
  undo: () => invoke<OpResult>("undo"),
  reword: (hash: string, message: string) => invoke<OpResult>("reword", { hash, message }),
  log: (all: boolean, limit: number) => invoke<LogRow[]>("log", { all, limit }),
  commitDetail: (hash: string) => invoke<CommitDetail>("commit_detail", { hash }),
  commitMessage: (hash: string) => invoke<[string, boolean]>("commit_message", { hash }),
  branches: () => invoke<Branch[]>("branches"),
  switchBranch: (name: string, remote: boolean) => invoke<OpResult>("switch_branch", { name, remote }),
  createBranch: (name: string, start: string | null) => invoke<OpResult>("create_branch", { name, start }),
  stashPush: (message: string) => invoke<OpResult>("stash_push", { message }),
  stashApply: (name: string, pop: boolean) => invoke<OpResult>("stash_apply", { name, pop }),
  remote: (op: "fetch" | "pull" | "push" | "push-force" | "sync") => invoke<OpResult>("remote_op", { op }),
  insight: () => invoke<Insight>("insight"),
  largestBlobs: (force: boolean) => invoke<Blob[]>("largest_blobs", { force }),
  applyFix: (fix: object) => invoke<OpResult>("apply_fix", { fix }),
  rebasePlan: (hash: string) => invoke<{ base: string | null; steps: RebaseStep[] }>("rebase_plan", { hash }),
  runRebase: (base: string | null, steps: RebaseStep[]) => invoke<OpResult>("run_rebase", { base, steps }),
  readConflict: (path: string) => invoke<ConflictFile>("read_conflict", { path }),
  resolveRegion: (path: string, region: number, pick: "ours" | "theirs" | "both" | "base") =>
    invoke<OpResult>("resolve_region", { path, region, pick }),
  conflictFileOp: (path: string, op: "ours" | "theirs" | "delete" | "base") => invoke<OpResult>("conflict_file_op", { path, op }),
  ignoreSources: () => invoke<IgnoreSource[]>("ignore_sources"),
  whyIgnored: (path: string) => invoke<WhyIgnored | null>("why_ignored", { path }),
  addIgnore: (rel: string, pattern: string, untrack: string[]) => invoke<OpResult>("add_ignore", { rel, pattern, untrack }),
  removeIgnore: (rel: string, line: number, text: string) => invoke<OpResult>("remove_ignore", { rel, line, text }),
  readTextFile: (rel: string) => invoke<string>("read_text_file", { rel }),
  writeIgnoreFile: (rel: string, text: string) => invoke<OpResult>("write_ignore_file", { rel, text }),
  cloneDefaults: () => invoke<{ parent: string }>("clone_defaults"),
  cloneName: (url: string) => invoke<string>("clone_name", { url }),
  cloneRepo: (url: string, parent: string, name: string) => invoke<RepoInfo>("clone_repo", { url, parent, name }),
  tags: () => invoke<[string, string, number][]>("tags"),
  searchLog: (query: string) => invoke<Commit[]>("search_log", { query }),
  initRepo: (path: string) => invoke<RepoInfo>("init_repo", { path }),
  initWarning: (path: string) => invoke<string | null>("init_warning", { path }),
  operation: (op: "continue" | "abort" | "skip") => invoke<OpResult>("operation", { op }),
};

export function humanSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  if (u === 0) return `${bytes} B`;
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[u]}`;
}

export function relativeTime(ts: number): string {
  const d = Math.max(0, Date.now() / 1000 - ts);
  if (d < 60) return "now";
  if (d < 3600) return `${Math.floor(d / 60)}m`;
  if (d < 86400) return `${Math.floor(d / 3600)}h`;
  if (d < 604800) return `${Math.floor(d / 86400)}d`;
  if (d < 2592000) return `${Math.floor(d / 604800)}w`;
  if (d < 31536000) return `${Math.floor(d / 2592000)}mo`;
  return `${Math.floor(d / 31536000)}y`;
}

export function splitPath(path: string): [string, string] {
  const trimmed = path.replace(/\/$/, "");
  const i = trimmed.lastIndexOf("/");
  const slash = path.endsWith("/") ? "/" : "";
  return i < 0 ? [trimmed + slash, ""] : [trimmed.slice(i + 1) + slash, trimmed.slice(0, i)];
}

/** `open_repo` rejects with this when the folder exists but has no repository yet. */
export interface NotARepo {
  kind: "not_a_repo";
  path: string;
  name: string;
  warning: string | null;
}

export function isNotARepo(e: unknown): e is NotARepo {
  return typeof e === "object" && e !== null && (e as { kind?: string }).kind === "not_a_repo";
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  if (typeof e === "object" && e !== null && typeof (e as { message?: unknown }).message === "string") return (e as { message: string }).message;
  return JSON.stringify(e);
}
