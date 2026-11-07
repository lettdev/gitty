//! Repository insight: sizes, largest blobs in history, LFS and .gitignore
//! health, and recent activity.

use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::cmp::Reverse;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Stdio;

use serde::{Deserialize, Serialize};

use crate::git::{Error, Result};
use crate::project::{self, LanguageStat, Project, ProjectKind};
use crate::repo::Repo;

/// Tracked files at or above this size belong in LFS.
pub const LFS_THRESHOLD: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct SizeNode {
    pub name: String,
    /// Path relative to the repo root; directories end with `/`.
    pub path: String,
    pub size: u64,
    pub files: u64,
    pub children: Vec<SizeNode>,
}

impl SizeNode {
    pub fn is_dir(&self) -> bool {
        self.path.is_empty() || self.path.ends_with('/')
    }

    pub fn find(&self, path: &str) -> Option<&SizeNode> {
        if self.path == path {
            return Some(self);
        }
        self.children.iter().filter(|c| path.starts_with(c.path.as_str())).find_map(|c| c.find(path))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct RepoSize {
    pub worktree: u64,
    pub worktree_files: u64,
    pub git_dir: u64,
    pub packed: u64,
    pub loose: u64,
    pub lfs_store: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum Severity {
    Warn,
    Info,
    Ok,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Fix {
    /// Append `pattern` to .gitignore and stop tracking `untrack`.
    Ignore { pattern: String, untrack: Vec<String> },
    /// Track patterns with LFS and convert the listed current files.
    LfsTrack { patterns: Vec<String>, convert: Vec<String> },
    /// Rewrite history so old versions move to LFS (interactive, destructive).
    LfsMigrate { patterns: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Health {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub fix: Option<Fix>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    pub oid: String,
    pub path: String,
    pub size: u64,
    /// How many versions of this path are in history.
    pub versions: usize,
    pub lfs: bool,
    /// The commit that introduced this version.
    pub commit: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct HotFile {
    pub path: String,
    pub commits: u32,
    pub last_author: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Owner {
    pub dir: String,
    pub author: String,
    pub share: u32,
    pub commits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub enum FileType {
    Code,
    Images,
    Audio,
    Video,
    Models,
    EngineAssets,
    Fonts,
    Docs,
    Binaries,
    Other,
}

impl FileType {
    pub const ALL: [FileType; 10] = [
        FileType::Code,
        FileType::Images,
        FileType::Audio,
        FileType::Video,
        FileType::Models,
        FileType::EngineAssets,
        FileType::Fonts,
        FileType::Docs,
        FileType::Binaries,
        FileType::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FileType::Code => "Code",
            FileType::Images => "Images & textures",
            FileType::Audio => "Audio",
            FileType::Video => "Video",
            FileType::Models => "3D models",
            FileType::EngineAssets => "Scenes & engine assets",
            FileType::Fonts => "Fonts",
            FileType::Docs => "Docs & data",
            FileType::Binaries => "Binaries & archives",
            FileType::Other => "Other",
        }
    }

    pub fn of(path: &str) -> FileType {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "cs" | "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go" | "gd" | "cpp" | "cc" | "cxx" | "h"
            | "hpp" | "c" | "m" | "mm" | "swift" | "kt" | "kts" | "java" | "dart" | "shader" | "hlsl" | "cginc"
            | "compute" | "glsl" | "gdshader" | "usf" | "ush" | "lua" | "rb" | "php" | "vue" | "svelte" | "html"
            | "css" | "scss" | "sass" | "less" | "sh" | "bash" | "ps1" | "sql" => FileType::Code,
            "png" | "jpg" | "jpeg" | "psd" | "psb" | "tga" | "tif" | "tiff" | "exr" | "hdr" | "bmp" | "gif" | "webp"
            | "svg" | "kra" | "ase" | "aseprite" | "dds" | "ico" | "ktx" | "astc" => FileType::Images,
            "wav" | "mp3" | "ogg" | "aif" | "aiff" | "flac" | "m4a" | "bank" | "fsb" | "wem" | "bnk" => FileType::Audio,
            "mp4" | "mov" | "webm" | "avi" | "mkv" | "m4v" => FileType::Video,
            "fbx" | "obj" | "blend" | "max" | "ma" | "mb" | "glb" | "gltf" | "dae" | "3ds" | "usd" | "usdz" | "abc" => {
                FileType::Models
            }
            "unity" | "prefab" | "asset" | "mat" | "anim" | "controller" | "overridecontroller" | "meta" | "tscn"
            | "tres" | "scn" | "res" | "umap" | "uasset" | "spriteatlas" | "playable" | "mixer" | "lighting" => {
                FileType::EngineAssets
            }
            "ttf" | "otf" | "woff" | "woff2" | "fnt" => FileType::Fonts,
            "md" | "txt" | "json" | "yaml" | "yml" | "xml" | "csv" | "toml" | "ini" | "cfg" | "pdf" | "lock" | "rst" => {
                FileType::Docs
            }
            "dll" | "so" | "dylib" | "exe" | "a" | "lib" | "apk" | "aab" | "ipa" | "zip" | "7z" | "rar" | "tar" | "gz"
            | "jar" | "bin" | "pak" | "unitypackage" | "wasm" => FileType::Binaries,
            _ => FileType::Other,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct TypeStat {
    pub kind: Option<FileType>,
    pub bytes: u64,
    pub files: usize,
    /// Largest extensions in this group, e.g. `[("psd", 812 MB), ("png", 90 MB)]`.
    pub top: Vec<(String, u64)>,
}

/// Size per file type, largest first.
pub fn file_types(files: &[(String, u64)]) -> Vec<TypeStat> {
    let mut by: HashMap<FileType, (u64, usize, HashMap<String, u64>)> = HashMap::new();
    for (path, size) in files {
        let kind = FileType::of(path);
        let e = by.entry(kind).or_default();
        e.0 += size;
        e.1 += 1;
        let ext = path.rsplit('/').next().and_then(|n| n.rsplit_once('.')).map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_else(|| "(none)".into());
        *e.2.entry(ext).or_default() += size;
    }
    let mut out: Vec<TypeStat> = by
        .into_iter()
        .map(|(kind, (bytes, files, exts))| {
            let mut top: Vec<(String, u64)> = exts.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            top.truncate(3);
            TypeStat { kind: Some(kind), bytes, files, top }
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.kind.cmp(&b.kind)));
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Insight {
    pub projects: Vec<Project>,
    pub languages: Vec<LanguageStat>,
    pub types: Vec<TypeStat>,
    pub size: RepoSize,
    pub tree: SizeNode,
    pub health: Vec<Health>,
    pub hot: Vec<HotFile>,
    pub owners: Vec<Owner>,
    pub lfs_installed: bool,
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn build_tree(files: &[(String, u64)]) -> SizeNode {
    #[derive(Default)]
    struct Tmp {
        size: u64,
        files: u64,
        dirs: BTreeMap<String, Tmp>,
        leaves: Vec<(String, u64)>,
    }
    let mut root = Tmp::default();
    for (path, size) in files {
        let mut node = &mut root;
        node.size += size;
        node.files += 1;
        let mut parts: Vec<&str> = path.split('/').collect();
        let leaf = parts.pop().unwrap_or_default();
        for part in parts {
            node = node.dirs.entry(part.to_string()).or_default();
            node.size += size;
            node.files += 1;
        }
        node.leaves.push((leaf.to_string(), *size));
    }
    fn convert(name: String, path: String, t: Tmp) -> SizeNode {
        let mut children: Vec<SizeNode> = t
            .dirs
            .into_iter()
            .map(|(n, c)| {
                let p = format!("{path}{n}/");
                convert(n, p, c)
            })
            .collect();
        children.extend(t.leaves.into_iter().map(|(n, s)| SizeNode {
            path: format!("{path}{n}"),
            name: n,
            size: s,
            files: 1,
            children: Vec::new(),
        }));
        children.sort_by(|a, b| b.size.cmp(&a.size).then(a.name.cmp(&b.name)));
        SizeNode { name, path, size: t.size, files: t.files, children }
    }
    convert(String::new(), String::new(), root)
}

fn ext_pattern(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 10 {
        return None;
    }
    Some(format!("*.{}", ext.to_ascii_lowercase()))
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[derive(Serialize, Deserialize)]
struct BlobCache {
    fingerprint: String,
    blobs: Vec<Blob>,
}

impl Repo {
    fn ls_files(&self, extra: &[&str]) -> Result<Vec<String>> {
        let mut args = vec!["ls-files", "-z"];
        args.extend_from_slice(extra);
        let out = self.git.run(&args)?;
        let mut seen = HashSet::new();
        Ok(String::from_utf8_lossy(&out)
            .split('\0')
            .filter(|p| !p.is_empty())
            .filter(|p| seen.insert(p.to_string()))
            .map(str::to_string)
            .collect())
    }

    pub fn lfs_installed(&self) -> bool {
        self.git.succeeds(&["lfs", "version"])
    }

    /// Which of `paths` have `filter=lfs` according to .gitattributes.
    pub fn lfs_tracked(&self, paths: &[String]) -> Result<HashSet<String>> {
        if paths.is_empty() {
            return Ok(HashSet::new());
        }
        let mut input = String::new();
        for p in paths {
            input.push_str(p);
            input.push('\0');
        }
        let out = self.git.run_input(&["check-attr", "-z", "--stdin", "filter"], Some(input.as_bytes()))?;
        let text = String::from_utf8_lossy(&out);
        let f: Vec<&str> = text.split('\0').collect();
        let mut set = HashSet::new();
        for rec in f.chunks(3) {
            if rec.len() == 3 && rec[2] == "lfs" {
                set.insert(rec[0].to_string());
            }
        }
        Ok(set)
    }

    fn is_ignored(&self, path: &str) -> bool {
        self.git.succeeds(&["check-ignore", "-q", "--no-index", "--", path])
    }

    /// Everything except the (slow) history scan.
    pub fn insight(&self) -> Result<Insight> {
        let tracked = self.ls_files(&["--cached"])?;
        let untracked = self.ls_files(&["--others", "--exclude-standard"])?;
        let mut files: Vec<(String, u64)> = Vec::with_capacity(tracked.len() + untracked.len());
        for p in tracked.iter().chain(untracked.iter()) {
            if let Ok(m) = fs::symlink_metadata(self.root().join(p)) {
                if m.is_file() {
                    files.push((p.clone(), m.len()));
                }
            }
        }
        let projects = project::detect(self.root(), &files);
        let languages = project::languages(&files);
        let types = file_types(&files);
        let tree = build_tree(&files);

        let mut size = RepoSize { worktree: tree.size, worktree_files: tree.files, ..Default::default() };
        size.git_dir = dir_size(self.git_dir());
        size.lfs_store = dir_size(&self.git_dir().join("lfs"));
        if let Ok(out) = self.git.run_text(&["count-objects", "-v"]) {
            for line in out.lines() {
                let (k, v) = line.split_once(": ").unwrap_or((line, "0"));
                let kib: u64 = v.trim().parse().unwrap_or(0);
                match k {
                    "size-pack" => size.packed = kib * 1024,
                    "size" => size.loose = kib * 1024,
                    _ => {}
                }
            }
        }

        let lfs_installed = self.lfs_installed();
        let tracked_set: HashSet<&str> = tracked.iter().map(String::as_str).collect();
        let mut health = Vec::new();
        self.check_lfs(&files, &tracked_set, &projects, lfs_installed, &mut health)?;
        self.check_ignores(&tracked, &projects, &mut health);
        self.check_unity(&projects, &mut health);
        health.sort_by_key(|h| h.severity);

        let (hot, owners) = self.activity(&tracked_set).unwrap_or_default();
        Ok(Insight { projects, languages, types, size, tree, health, hot, owners, lfs_installed })
    }

    fn check_lfs(
        &self,
        files: &[(String, u64)],
        tracked: &HashSet<&str>,
        projects: &[Project],
        lfs_installed: bool,
        out: &mut Vec<Health>,
    ) -> Result<()> {
        let engine_patterns: HashSet<&str> = projects.iter().flat_map(|p| p.kind.lfs_patterns().iter().copied()).collect();
        let candidates: Vec<(String, u64)> = files
            .iter()
            .filter(|(p, s)| {
                tracked.contains(p.as_str())
                    && (*s >= LFS_THRESHOLD || (*s >= 1024 * 1024 && ext_pattern(p).is_some_and(|e| engine_patterns.contains(e.as_str()))))
            })
            .cloned()
            .collect();
        let paths: Vec<String> = candidates.iter().map(|(p, _)| p.clone()).collect();
        let in_lfs = self.lfs_tracked(&paths)?;
        let mut missing: Vec<(String, u64)> = candidates.into_iter().filter(|(p, _)| !in_lfs.contains(p)).collect();
        if missing.is_empty() {
            if !in_lfs.is_empty() {
                out.push(Health {
                    severity: Severity::Ok,
                    title: format!("{} large files are stored in LFS", in_lfs.len()),
                    detail: String::new(),
                    fix: None,
                });
            }
            return Ok(());
        }
        missing.sort_by_key(|m| Reverse(m.1));
        let total: u64 = missing.iter().map(|(_, s)| s).sum();
        let mut patterns: Vec<String> = missing.iter().filter_map(|(p, _)| ext_pattern(p)).collect();
        patterns.sort();
        patterns.dedup();
        let list: Vec<String> = missing.iter().take(5).map(|(p, s)| format!("{}  {p}", human_size(*s))).collect();
        let more = if missing.len() > 5 { format!("\n…and {} more", missing.len() - 5) } else { String::new() };
        if !lfs_installed {
            out.push(Health {
                severity: Severity::Warn,
                title: format!("{} large files, but Git LFS is not installed", missing.len()),
                detail: format!(
                    "{} in files that every clone downloads in full.\nInstall it from https://git-lfs.com, then run `git lfs install`.\n\n{}{more}",
                    human_size(total),
                    list.join("\n")
                ),
                fix: None,
            });
            return Ok(());
        }
        out.push(Health {
            severity: Severity::Warn,
            title: format!("{} large files are not in LFS ({})", missing.len(), human_size(total)),
            detail: format!(
                "Track {} with LFS and convert the current versions.\nOlder versions stay in history until you migrate it.\n\n{}{more}",
                patterns.join(" "),
                list.join("\n")
            ),
            fix: Some(Fix::LfsTrack { patterns, convert: missing.into_iter().map(|(p, _)| p).collect() }),
        });
        Ok(())
    }

    fn check_ignores(&self, tracked: &[String], projects: &[Project], out: &mut Vec<Health>) {
        let mut rules: Vec<(String, &'static str)> = Vec::new();
        for p in projects {
            for r in p.kind.ignore_rules() {
                rules.push((format!("{}{r}", p.dir), p.kind.name()));
            }
        }
        let mut ok = Vec::new();
        for (rule, kind) in rules {
            let is_dir = rule.ends_with('/');
            let committed: Vec<String> = tracked
                .iter()
                .filter(|t| if is_dir { t.starts_with(&rule) } else { **t == rule })
                .cloned()
                .collect();
            let anchored = format!("/{rule}");
            if !committed.is_empty() {
                out.push(Health {
                    severity: Severity::Warn,
                    title: format!("{rule} is committed ({} file{})", committed.len(), if committed.len() == 1 { "" } else { "s" }),
                    detail: format!(
                        "{kind} regenerates this; it only adds noise and conflicts.\nThe fix adds `{anchored}` to .gitignore and stops tracking it (files stay on disk)."
                    ),
                    fix: Some(Fix::Ignore { pattern: anchored, untrack: vec![rule.clone()] }),
                });
                continue;
            }
            let exists = self.root().join(rule.trim_end_matches('/')).exists();
            if exists && !self.is_ignored(rule.trim_end_matches('/')) {
                out.push(Health {
                    severity: Severity::Warn,
                    title: format!("{rule} is not ignored"),
                    detail: format!("{kind} creates this folder; add `{anchored}` to .gitignore so it is never committed."),
                    fix: Some(Fix::Ignore { pattern: anchored, untrack: Vec::new() }),
                });
            } else if exists {
                ok.push(rule);
            }
        }
        for junk in [".DS_Store", "Thumbs.db", "desktop.ini"] {
            let hits: Vec<String> =
                tracked.iter().filter(|t| t.rsplit('/').next() == Some(junk)).cloned().collect();
            if !hits.is_empty() {
                out.push(Health {
                    severity: Severity::Warn,
                    title: format!("{junk} is committed ({}×)", hits.len()),
                    detail: "Operating system clutter. The fix ignores it everywhere and untracks the copies.".into(),
                    fix: Some(Fix::Ignore { pattern: junk.to_string(), untrack: hits }),
                });
            }
        }
        if !ok.is_empty() {
            out.push(Health { severity: Severity::Ok, title: format!("{} ignored correctly", ok.join(", ")), detail: String::new(), fix: None });
        }
    }

    fn check_unity(&self, projects: &[Project], out: &mut Vec<Health>) {
        for p in projects.iter().filter(|p| p.kind == ProjectKind::Unity) {
            let editor = fs::read_to_string(self.root().join(format!("{}ProjectSettings/EditorSettings.asset", p.dir))).unwrap_or_default();
            if editor.is_empty() {
                continue;
            }
            if let Some(mode) = editor.lines().find_map(|l| l.trim().strip_prefix("m_SerializationMode:")) {
                if mode.trim() != "2" {
                    out.push(Health {
                        severity: Severity::Warn,
                        title: "Unity assets are saved as binary".into(),
                        detail: "Set Edit › Project Settings › Editor › Asset Serialization to Force Text so scenes and prefabs can be diffed and merged.".into(),
                        fix: None,
                    });
                }
            }
            if let Some(mode) = editor.lines().find_map(|l| l.trim().strip_prefix("m_ExternalVersionControlSupport:")) {
                if !mode.contains("Visible Meta Files") {
                    out.push(Health {
                        severity: Severity::Warn,
                        title: "Unity .meta files are hidden".into(),
                        detail: "Set Version Control mode to Visible Meta Files, otherwise references break for teammates.".into(),
                        fix: None,
                    });
                }
            }
        }
    }

    /// Hot files (last 90 days) and the most active author per folder (last year).
    fn activity(&self, tracked: &HashSet<&str>) -> Result<(Vec<HotFile>, Vec<Owner>)> {
        let out = self.git.run_text(&["log", "--since=1.year.ago", "--no-merges", "--format=%x1e%at%x1f%an", "--name-only", "HEAD", "--"]);
        let out = match out {
            Ok(o) => o,
            Err(Error::Command { .. }) => return Ok(Default::default()),
            Err(e) => return Err(e),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let recent = now - 90 * 86_400;
        let mut hot: HashMap<String, (u32, String)> = HashMap::new();
        let mut dirs: HashMap<String, HashMap<String, u32>> = HashMap::new();
        for rec in out.split('\x1e').filter(|r| !r.trim().is_empty()) {
            let mut lines = rec.lines();
            let header = lines.next().unwrap_or_default();
            let (ts, author) = header.split_once('\x1f').unwrap_or((header, ""));
            let ts: i64 = ts.parse().unwrap_or(0);
            let mut seen_dirs = HashSet::new();
            for f in lines.filter(|l| !l.is_empty() && tracked.contains(*l)) {
                if ts >= recent {
                    let e = hot.entry(f.to_string()).or_insert((0, author.to_string()));
                    e.0 += 1;
                }
                let parts: Vec<&str> = f.split('/').collect();
                let key = match parts.len() {
                    0 | 1 => continue,
                    2 => format!("{}/", parts[0]),
                    _ => format!("{}/{}/", parts[0], parts[1]),
                };
                if seen_dirs.insert(key.clone()) {
                    *dirs.entry(key).or_default().entry(author.to_string()).or_default() += 1;
                }
            }
        }
        let mut hot: Vec<HotFile> =
            hot.into_iter().map(|(path, (commits, last_author))| HotFile { path, commits, last_author }).collect();
        hot.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.path.cmp(&b.path)));
        hot.truncate(30);
        let mut owners: Vec<Owner> = dirs
            .into_iter()
            .map(|(dir, authors)| {
                let total: u32 = authors.values().sum();
                let (author, n) = authors.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).unwrap_or_default();
                Owner { dir, author, share: n * 100 / total.max(1), commits: total }
            })
            .collect();
        owners.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.dir.cmp(&b.dir)));
        owners.truncate(12);
        Ok((hot, owners))
    }

    /// The largest file versions anywhere in history, grouped by path.
    /// Cached in `.git/gitty/cache/blobs.json` until refs change.
    pub fn largest_blobs(&self, limit: usize, use_cache: bool) -> Result<Vec<Blob>> {
        let fingerprint = self.refs_fingerprint()?;
        let cache_path = self.git_dir().join("gitty").join("cache").join("blobs.json");
        if use_cache {
            if let Ok(text) = fs::read_to_string(&cache_path) {
                if let Ok(c) = serde_json::from_str::<BlobCache>(&text) {
                    if c.fingerprint == fingerprint {
                        return Ok(c.blobs);
                    }
                }
            }
        }
        if fingerprint.trim().is_empty() {
            return Ok(Vec::new());
        }

        // rev-list --objects --all | cat-file --batch-check
        let mut rev = self.git.command(["rev-list", "--objects", "--all"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let rev_out = rev.stdout.take().expect("piped");
        let cat = self
            .git
            .command(["cat-file", "--batch-check=%(objecttype) %(objectname) %(objectsize) %(rest)"])
            .stdin(Stdio::from(rev_out))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let cat_out = cat.wait_with_output()?;
        let _ = rev.wait();
        let mut heap: BinaryHeap<Reverse<(u64, String, String)>> = BinaryHeap::new();
        let mut versions: HashMap<String, usize> = HashMap::new();
        for line in String::from_utf8_lossy(&cat_out.stdout).lines() {
            let mut f = line.splitn(4, ' ');
            let (Some("blob"), Some(oid), Some(size), Some(path)) = (f.next(), f.next(), f.next(), f.next()) else { continue };
            let size: u64 = size.parse().unwrap_or(0);
            *versions.entry(path.to_string()).or_default() += 1;
            heap.push(Reverse((size, oid.to_string(), path.to_string())));
            if heap.len() > limit * 8 {
                heap.pop();
            }
        }
        let mut top: Vec<(u64, String, String)> = heap.into_iter().map(|Reverse(x)| x).collect();
        top.sort_by_key(|t| Reverse(t.0));
        let mut by_path: Vec<Blob> = Vec::new();
        for (size, oid, path) in top {
            if by_path.iter().any(|b| b.path == path) {
                continue;
            }
            by_path.push(Blob { oid, versions: versions.get(&path).copied().unwrap_or(1), path, size, lfs: false, commit: None });
            if by_path.len() == limit {
                break;
            }
        }
        let paths: Vec<String> = by_path.iter().map(|b| b.path.clone()).collect();
        let lfs = self.lfs_tracked(&paths).unwrap_or_default();
        let commits = self.introducing_commits(&by_path.iter().map(|b| b.oid.clone()).collect::<Vec<_>>())?;
        for b in &mut by_path {
            b.lfs = lfs.contains(&b.path);
            b.commit = commits.get(&b.oid).cloned();
        }
        let _ = fs::create_dir_all(cache_path.parent().expect("cache dir"));
        let _ = fs::write(&cache_path, serde_json::to_string(&BlobCache { fingerprint, blobs: by_path.clone() })?);
        Ok(by_path)
    }

    /// One pass over history to find the oldest commit that added each blob.
    fn introducing_commits(&self, oids: &[String]) -> Result<HashMap<String, String>> {
        let wanted: HashSet<&str> = oids.iter().map(String::as_str).collect();
        let mut found = HashMap::new();
        if wanted.is_empty() {
            return Ok(found);
        }
        let mut child = self
            .git
            .command(["log", "--all", "--no-renames", "--format=@%H", "--raw", "--no-abbrev", "--diff-filter=AM", "--root"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let reader = BufReader::new(child.stdout.take().expect("piped"));
        let mut current = String::new();
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Some(h) = line.strip_prefix('@') {
                current = h.to_string();
            } else if line.starts_with(':') {
                // :100644 100644 <old> <new> M\tpath
                if let Some(new) = line.split(' ').nth(3) {
                    if wanted.contains(new) {
                        found.insert(new.to_string(), current.clone());
                    }
                }
            }
        }
        let _ = child.wait();
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_sizes_and_order() {
        let t = build_tree(&[("a/x.bin".into(), 10), ("a/b/y.bin".into(), 30), ("z.txt".into(), 5)]);
        assert_eq!(t.size, 45);
        assert_eq!(t.children[0].path, "a/");
        assert_eq!(t.children[0].size, 40);
        assert_eq!(t.children[0].children[0].path, "a/b/");
        assert_eq!(t.find("a/b/").unwrap().files, 1);
        assert!(t.find("a/b/y.bin").is_some());
    }

    #[test]
    fn types() {
        let t = file_types(&[("a.psd".into(), 900), ("b.png".into(), 100), ("c.cs".into(), 50), ("Makefile".into(), 5)]);
        assert_eq!(t[0].kind, Some(FileType::Images));
        assert_eq!(t[0].bytes, 1000);
        assert_eq!(t[0].top[0], ("psd".to_string(), 900));
        assert_eq!(t.last().unwrap().kind, Some(FileType::Other));
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(412 * 1024 * 1024), "412 MB");
        assert_eq!(ext_pattern("Art/Cliff.PSD").as_deref(), Some("*.psd"));
    }
}
