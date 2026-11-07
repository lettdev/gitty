//! Detect what kind of project a repository holds, from its marker files.

use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ProjectKind {
    Unity,
    Godot,
    Unreal,
    Node,
    Rust,
    Go,
    Python,
    Flutter,
    Dart,
    Swift,
    DotNet,
    Android,
    Jvm,
    Cpp,
}

impl ProjectKind {
    pub fn name(self) -> &'static str {
        match self {
            ProjectKind::Unity => "Unity",
            ProjectKind::Godot => "Godot",
            ProjectKind::Unreal => "Unreal Engine",
            ProjectKind::Node => "Node.js",
            ProjectKind::Rust => "Rust",
            ProjectKind::Go => "Go",
            ProjectKind::Python => "Python",
            ProjectKind::Flutter => "Flutter",
            ProjectKind::Dart => "Dart",
            ProjectKind::Swift => "Swift / Xcode",
            ProjectKind::DotNet => ".NET",
            ProjectKind::Android => "Android",
            ProjectKind::Jvm => "JVM",
            ProjectKind::Cpp => "C / C++ (CMake)",
        }
    }

    pub fn is_game_engine(self) -> bool {
        matches!(self, ProjectKind::Unity | ProjectKind::Godot | ProjectKind::Unreal)
    }

    /// Paths (relative to the project folder) that should never be committed.
    pub fn ignore_rules(self) -> &'static [&'static str] {
        match self {
            ProjectKind::Unity => &[
                "Library/", "Temp/", "Obj/", "Logs/", "UserSettings/", "MemoryCaptures/", "Build/", "Builds/", ".vs/",
                ".idea/", ".gradle/",
            ],
            ProjectKind::Godot => &[".godot/", ".import/", "export_presets.cfg"],
            ProjectKind::Unreal => &["Binaries/", "Intermediate/", "Saved/", "DerivedDataCache/", ".vs/"],
            ProjectKind::Node => &["node_modules/", ".next/", ".nuxt/", ".turbo/"],
            ProjectKind::Rust => &["target/"],
            ProjectKind::Python => &["__pycache__/", ".venv/", "venv/", ".pytest_cache/", ".mypy_cache/"],
            ProjectKind::Flutter | ProjectKind::Dart => &[".dart_tool/", "build/"],
            ProjectKind::Swift => &["DerivedData/", ".build/", "xcuserdata/"],
            ProjectKind::DotNet => &["bin/", "obj/", ".vs/"],
            ProjectKind::Android | ProjectKind::Jvm => &[".gradle/", "build/", "local.properties"],
            ProjectKind::Cpp => &["build/", "cmake-build-debug/", "cmake-build-release/"],
            ProjectKind::Go => &[],
        }
    }

    /// File patterns that belong in Git LFS for this kind of project.
    pub fn lfs_patterns(self) -> &'static [&'static str] {
        const ART: &[&str] = &[
            "*.psd", "*.psb", "*.png", "*.jpg", "*.jpeg", "*.tga", "*.tif", "*.tiff", "*.exr", "*.hdr", "*.kra",
            "*.wav", "*.mp3", "*.ogg", "*.aif", "*.aiff", "*.flac", "*.mp4", "*.mov", "*.webm", "*.fbx", "*.obj",
            "*.blend", "*.max", "*.ma", "*.mb", "*.glb", "*.gltf", "*.ttf", "*.otf",
        ];
        match self {
            ProjectKind::Unity | ProjectKind::Godot => ART,
            ProjectKind::Unreal => &["*.uasset", "*.umap", "*.psd", "*.png", "*.tga", "*.wav", "*.fbx", "*.exr"],
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Project {
    pub kind: ProjectKind,
    /// Folder relative to the repo root, with a trailing slash ("" = root).
    pub dir: String,
    pub version: Option<String>,
    pub details: Vec<String>,
    /// The file the detection was based on.
    pub marker: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LanguageStat {
    pub name: &'static str,
    pub files: usize,
    pub bytes: u64,
}

fn language(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "cs" => "C#",
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "gd" => "GDScript",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "h" => "C++",
        "c" => "C",
        "swift" => "Swift",
        "kt" | "kts" => "Kotlin",
        "java" => "Java",
        "dart" => "Dart",
        "shader" | "hlsl" | "cginc" | "compute" | "glsl" | "gdshader" | "usf" | "ush" => "Shaders",
        "lua" => "Lua",
        "rb" => "Ruby",
        "php" => "PHP",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "sh" | "bash" | "zsh" | "ps1" => "Shell",
        _ => return None,
    })
}

/// Language breakdown by bytes, largest first.
pub fn languages(files: &[(String, u64)]) -> Vec<LanguageStat> {
    let mut map: HashMap<&'static str, (usize, u64)> = HashMap::new();
    for (path, size) in files {
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if let Some(lang) = language(&ext) {
            let e = map.entry(lang).or_default();
            e.0 += 1;
            e.1 += size;
        }
    }
    let mut out: Vec<LanguageStat> = map.into_iter().map(|(name, (files, bytes))| LanguageStat { name, files, bytes }).collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(b.name)));
    out
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

fn depth(path: &str) -> usize {
    path.matches('/').count()
}

fn dir_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((d, _)) => format!("{d}/"),
        None => String::new(),
    }
}

fn count_ext(files: &[(String, u64)], dir: &str, exts: &[&str]) -> usize {
    files
        .iter()
        .filter(|(p, _)| p.starts_with(dir))
        .filter(|(p, _)| p.rsplit_once('.').is_some_and(|(_, e)| exts.contains(&e.to_ascii_lowercase().as_str())))
        .count()
}

fn toml_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = section.is_empty();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_section = l == format!("[{section}]");
            continue;
        }
        if in_section {
            if let Some(rest) = l.strip_prefix(key) {
                if let Some(v) = rest.trim_start().strip_prefix('=') {
                    return Some(v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Detect projects from the repository's file list (tracked plus untracked,
/// not ignored). Only looks a few folders deep so huge trees stay fast.
pub fn detect(root: &Path, files: &[(String, u64)]) -> Vec<Project> {
    let mut found: Vec<Project> = Vec::new();
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).filter(|p| depth(p) <= 3).collect();
    let has = |rel: &str| files.iter().any(|(p, _)| p == rel);

    for &p in &paths {
        let name = p.rsplit('/').next().unwrap_or(p);
        let vendored = p.contains("node_modules/") || p.starts_with("Library/") || p.contains("/Library/");
        if vendored || (p.contains("Packages/") && name == "package.json") {
            continue;
        }
        let dir = dir_of(p);
        let project = if p.ends_with("ProjectSettings/ProjectVersion.txt") {
            let dir = p.trim_end_matches("ProjectSettings/ProjectVersion.txt").to_string();
            let text = read(root, p);
            let version = text.lines().find_map(|l| l.strip_prefix("m_EditorVersion:")).map(|v| v.trim().to_string());
            let manifest = read(root, &format!("{dir}Packages/manifest.json"));
            let pipeline = if manifest.contains("com.unity.render-pipelines.universal") {
                "URP"
            } else if manifest.contains("com.unity.render-pipelines.high-definition") {
                "HDRP"
            } else {
                "Built-in render pipeline"
            };
            let packages = serde_json::from_str::<serde_json::Value>(&manifest)
                .ok()
                .and_then(|v| v.get("dependencies").and_then(|d| d.as_object()).map(|o| o.len()))
                .unwrap_or(0);
            let scripts = count_ext(files, &format!("{dir}Assets/"), &["cs"]);
            let scenes = count_ext(files, &format!("{dir}Assets/"), &["unity"]);
            Some(Project {
                kind: ProjectKind::Unity,
                dir,
                version,
                details: vec![pipeline.into(), plural(scripts, "C# script"), plural(scenes, "scene"), plural(packages, "package")],
                marker: p.to_string(),
            })
        } else if name == "project.godot" {
            let text = read(root, p);
            let features = text.lines().find_map(|l| l.strip_prefix("config/features=")).unwrap_or_default();
            let parts: Vec<String> =
                features.split('"').skip(1).step_by(2).map(str::to_string).collect();
            let version = parts.first().cloned();
            let mut details: Vec<String> = parts.into_iter().skip(1).collect();
            details.push(plural(count_ext(files, &dir, &["gd"]), "GDScript file"));
            let cs = count_ext(files, &dir, &["cs"]);
            if cs > 0 {
                details.push(plural(cs, "C# file"));
            }
            details.push(plural(count_ext(files, &dir, &["tscn"]), "scene"));
            Some(Project { kind: ProjectKind::Godot, dir, version, details, marker: p.to_string() })
        } else if name.ends_with(".uproject") {
            let v: serde_json::Value = serde_json::from_str(&read(root, p)).unwrap_or_default();
            let version = v.get("EngineAssociation").and_then(|e| e.as_str()).map(str::to_string);
            let modules = v.get("Modules").and_then(|m| m.as_array()).map(Vec::len).unwrap_or(0);
            let cpp = count_ext(files, &dir, &["cpp", "h"]);
            let maps = count_ext(files, &dir, &["umap"]);
            Some(Project {
                kind: ProjectKind::Unreal,
                dir,
                version,
                details: vec![plural(modules, "module"), plural(cpp, "C++ file"), plural(maps, "map")],
                marker: p.to_string(),
            })
        } else if name == "package.json" {
            let v: serde_json::Value = serde_json::from_str(&read(root, p)).unwrap_or_default();
            let deps: Vec<String> = ["dependencies", "devDependencies"]
                .iter()
                .filter_map(|k| v.get(k).and_then(|d| d.as_object()))
                .flat_map(|o| o.keys().cloned())
                .collect();
            let has_dep = |d: &str| deps.iter().any(|x| x == d);
            let manager = if has(&format!("{dir}bun.lock")) || has(&format!("{dir}bun.lockb")) {
                "bun"
            } else if has(&format!("{dir}pnpm-lock.yaml")) {
                "pnpm"
            } else if has(&format!("{dir}yarn.lock")) {
                "yarn"
            } else {
                "npm"
            };
            let mut details = Vec::new();
            for (dep, label) in [
                ("next", "Next.js"),
                ("nuxt", "Nuxt"),
                ("@angular/core", "Angular"),
                ("@sveltejs/kit", "SvelteKit"),
                ("svelte", "Svelte"),
                ("vue", "Vue"),
                ("react", "React"),
                ("electron", "Electron"),
                ("@tauri-apps/api", "Tauri"),
                ("vite", "Vite"),
                ("express", "Express"),
                ("@nestjs/core", "NestJS"),
            ] {
                if has_dep(dep) && !(label == "Svelte" && has_dep("@sveltejs/kit")) && !(label == "React" && has_dep("next")) {
                    details.push(label.to_string());
                }
            }
            if has_dep("typescript") || has(&format!("{dir}tsconfig.json")) {
                details.push("TypeScript".into());
            }
            let n = deps.len();
            details.push(format!("{manager} · {n} {}", if n == 1 { "dependency" } else { "dependencies" }));
            let version = v.get("name").and_then(|n| n.as_str()).map(str::to_string);
            Some(Project { kind: ProjectKind::Node, dir, version, details, marker: p.to_string() })
        } else if name == "Cargo.toml" {
            let text = read(root, p);
            let mut details = Vec::new();
            if text.contains("[workspace]") {
                let members = text.split("members").nth(1).map(|s| s.split(']').next().unwrap_or("").matches('"').count() / 2).unwrap_or(0);
                details.push(format!("workspace · {}", plural(members, "crate")));
            }
            if let Some(e) = toml_value(&text, "package", "edition").or_else(|| toml_value(&text, "workspace.package", "edition")) {
                details.push(format!("edition {e}"));
            }
            let version = toml_value(&text, "package", "name");
            Some(Project { kind: ProjectKind::Rust, dir, version, details, marker: p.to_string() })
        } else if name == "go.mod" {
            let text = read(root, p);
            let module = text.lines().find_map(|l| l.strip_prefix("module ")).map(|m| m.trim().to_string());
            let go = text.lines().find_map(|l| l.strip_prefix("go ")).map(|v| format!("go {}", v.trim()));
            Some(Project { kind: ProjectKind::Go, dir, version: module, details: go.into_iter().collect(), marker: p.to_string() })
        } else if name == "pyproject.toml" || (name == "requirements.txt" && !has(&format!("{dir}pyproject.toml"))) {
            let text = read(root, p).to_lowercase();
            let manager = if has(&format!("{dir}uv.lock")) {
                "uv"
            } else if has(&format!("{dir}poetry.lock")) {
                "poetry"
            } else if has(&format!("{dir}Pipfile.lock")) {
                "pipenv"
            } else {
                "pip"
            };
            let mut details = vec![manager.to_string()];
            for (k, label) in [("django", "Django"), ("fastapi", "FastAPI"), ("flask", "Flask"), ("torch", "PyTorch")] {
                if text.contains(k) {
                    details.push(label.into());
                }
            }
            let version = toml_value(&read(root, p), "project", "name");
            Some(Project { kind: ProjectKind::Python, dir, version, details, marker: p.to_string() })
        } else if name == "pubspec.yaml" {
            let text = read(root, p);
            let flutter = text.contains("flutter:");
            let version = text.lines().find_map(|l| l.strip_prefix("name:")).map(|n| n.trim().to_string());
            let kind = if flutter { ProjectKind::Flutter } else { ProjectKind::Dart };
            let dart = plural(count_ext(files, &dir, &["dart"]), "Dart file");
            Some(Project { kind, dir, version, details: vec![dart], marker: p.to_string() })
        } else if name == "Package.swift" || p.ends_with(".xcodeproj/project.pbxproj") {
            let dir = if name == "Package.swift" { dir } else { dir_of(dir.trim_end_matches('/')) };
            let what = if name == "Package.swift" { "Swift package" } else { "Xcode project" };
            Some(Project {
                kind: ProjectKind::Swift,
                dir: dir.clone(),
                version: None,
                details: vec![what.into(), plural(count_ext(files, &dir, &["swift"]), "Swift file")],
                marker: p.to_string(),
            })
        } else if name.ends_with(".sln") || name.ends_with(".csproj") {
            Some(Project { kind: ProjectKind::DotNet, dir, version: None, details: vec![name.to_string()], marker: p.to_string() })
        } else if name == "build.gradle" || name == "build.gradle.kts" || name == "pom.xml" {
            let text = read(root, p);
            let android = text.contains("com.android");
            let kind = if android { ProjectKind::Android } else { ProjectKind::Jvm };
            let tool = if name == "pom.xml" { "Maven" } else { "Gradle" };
            Some(Project { kind, dir, version: None, details: vec![tool.into()], marker: p.to_string() })
        } else if p == "CMakeLists.txt" {
            Some(Project { kind: ProjectKind::Cpp, dir, version: None, details: vec![], marker: p.to_string() })
        } else {
            None
        };
        if let Some(pr) = project {
            found.push(pr);
        }
    }

    // Unity and Godot generate .csproj/.sln files; those are not separate .NET projects.
    let engine_dirs: Vec<String> = found.iter().filter(|p| p.kind.is_game_engine()).map(|p| p.dir.clone()).collect();
    found.retain(|p| !(p.kind == ProjectKind::DotNet && engine_dirs.iter().any(|d| p.dir.starts_with(d.as_str()))));

    // Keep the shallowest project of each kind; mention the rest.
    found.sort_by_key(|p| (depth(&p.dir), p.dir.clone()));
    let mut out: Vec<Project> = Vec::new();
    let mut extra: HashMap<ProjectKind, usize> = HashMap::new();
    for p in found {
        if out.iter().any(|o| o.kind == p.kind) {
            *extra.entry(p.kind).or_default() += 1;
        } else {
            out.push(p);
        }
    }
    for p in &mut out {
        if let Some(n) = extra.get(&p.kind) {
            p.details.push(format!("+{n} more in subfolders"));
        }
    }
    // Engines first: they shape everything else (ignore rules, LFS).
    out.sort_by_key(|p| (!p.kind.is_game_engine(), depth(&p.dir)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) -> (String, u64) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        (rel.to_string(), text.len() as u64)
    }

    #[test]
    fn unity_project_in_subfolder() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let files = vec![
            write(r, "game/ProjectSettings/ProjectVersion.txt", "m_EditorVersion: 6000.0.23f1\n"),
            write(r, "game/Packages/manifest.json", r#"{"dependencies":{"com.unity.render-pipelines.universal":"17.0.3","com.unity.ugui":"2.0.0"}}"#),
            write(r, "game/Assets/Scripts/Player.cs", "class P {}"),
            write(r, "game/Assets/Scenes/Main.unity", "%YAML"),
            write(r, "game/game.sln", ""),
            write(r, "tools/package.json", r#"{"name":"tools","devDependencies":{"typescript":"5"}}"#),
        ];
        let p = detect(r, &files);
        assert_eq!(p[0].kind, ProjectKind::Unity);
        assert_eq!(p[0].dir, "game/");
        assert_eq!(p[0].version.as_deref(), Some("6000.0.23f1"));
        assert!(p[0].details.contains(&"URP".to_string()));
        assert!(p[0].details.contains(&"1 C# script".to_string()));
        assert!(!p.iter().any(|x| x.kind == ProjectKind::DotNet), "Unity .sln is not a .NET project");
        assert!(p.iter().any(|x| x.kind == ProjectKind::Node && x.details.contains(&"TypeScript".to_string())));
    }

    #[test]
    fn rust_workspace_and_languages() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let files = vec![
            write(r, "Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n[workspace.package]\nedition = \"2024\"\n"),
            write(r, "a/Cargo.toml", "[package]\nname = \"a\"\n"),
            write(r, "a/src/lib.rs", "fn main() {}"),
            write(r, "web/app.ts", "let x = 1;"),
        ];
        let p = detect(r, &files);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].dir, "");
        assert!(p[0].details.iter().any(|d| d.contains("2 crates")));
        assert!(p[0].details.iter().any(|d| d.contains("+1 more")));
        let langs = languages(&files);
        assert_eq!(langs[0].name, "Rust");
    }

    #[test]
    fn godot_version() {
        let d = tempfile::tempdir().unwrap();
        let files = vec![write(d.path(), "project.godot", "config/features=PackedStringArray(\"4.3\", \"Forward Plus\")\n")];
        let p = detect(d.path(), &files);
        assert_eq!(p[0].version.as_deref(), Some("4.3"));
        assert!(p[0].details.contains(&"Forward Plus".to_string()));
    }
}
