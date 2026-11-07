//! Readable summaries of changes to Unity YAML assets (scenes, prefabs…).
//! Purely structural: it maps changed lines to the object and property they
//! belong to.

use std::collections::HashMap;

use crate::diff::{FileDiff, LineKind};

pub const EXTENSIONS: &[&str] = &[
    "unity", "prefab", "asset", "mat", "anim", "controller", "overridecontroller", "physicmaterial", "physicsmaterial2d",
    "lighting", "playable", "mask", "spriteatlas", "preset", "terrainlayer", "mixer", "guiskin", "fontsettings",
];

pub fn is_unity_yaml(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, e)| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn class_name(id: u32) -> &'static str {
    match id {
        1 => "GameObject",
        4 => "Transform",
        20 => "Camera",
        21 => "Material",
        23 => "MeshRenderer",
        28 => "Texture2D",
        29 => "OcclusionCullingSettings",
        33 => "MeshFilter",
        50 => "Rigidbody2D",
        54 => "Rigidbody",
        58 => "CircleCollider2D",
        61 => "BoxCollider2D",
        64 => "MeshCollider",
        65 => "BoxCollider",
        74 => "AnimationClip",
        81 => "AudioListener",
        82 => "AudioSource",
        91 => "AnimatorController",
        95 => "Animator",
        104 => "RenderSettings",
        108 => "Light",
        111 => "Animation",
        114 => "MonoBehaviour",
        135 => "SphereCollider",
        136 => "CapsuleCollider",
        137 => "SkinnedMeshRenderer",
        157 => "LightmapSettings",
        196 => "NavMeshSettings",
        198 => "ParticleSystem",
        199 => "ParticleSystemRenderer",
        212 => "SpriteRenderer",
        222 => "CanvasRenderer",
        223 => "Canvas",
        224 => "RectTransform",
        225 => "CanvasGroup",
        1001 => "PrefabInstance",
        _ => "Object",
    }
}

#[derive(Clone, Debug)]
struct Obj {
    file_id: String,
    class: u32,
    /// 1-based line of the `--- !u!` header.
    start: u32,
    name: Option<String>,
    game_object: Option<String>,
}

struct Index {
    objects: Vec<Obj>,
    by_id: HashMap<String, usize>,
    lines: Vec<String>,
}

fn index(content: &str) -> Index {
    let lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut objects: Vec<Obj> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(rest) = l.strip_prefix("--- !u!") {
            let mut parts = rest.split_whitespace();
            let class = parts.next().and_then(|c| c.parse().ok()).unwrap_or(0);
            let file_id = parts.next().unwrap_or("").trim_start_matches('&').to_string();
            objects.push(Obj { file_id, class, start: i as u32 + 1, name: None, game_object: None });
        } else if let Some(o) = objects.last_mut() {
            let t = l.trim_start();
            let indent = l.len() - t.len();
            if indent == 2 {
                if let Some(n) = t.strip_prefix("m_Name:") {
                    o.name = Some(n.trim().to_string()).filter(|n| !n.is_empty());
                } else if let Some(g) = t.strip_prefix("m_GameObject:") {
                    o.game_object = g.split("fileID:").nth(1).map(|s| s.trim().trim_end_matches('}').trim().to_string());
                }
            }
        }
    }
    let by_id = objects.iter().enumerate().map(|(i, o)| (o.file_id.clone(), i)).collect();
    Index { objects, by_id, lines }
}

impl Index {
    fn object_at(&self, line: u32) -> Option<&Obj> {
        let pos = self.objects.partition_point(|o| o.start <= line);
        pos.checked_sub(1).map(|i| &self.objects[i])
    }

    fn label(&self, o: &Obj) -> String {
        let class = class_name(o.class);
        let owner = o
            .game_object
            .as_ref()
            .and_then(|g| self.by_id.get(g))
            .and_then(|&i| self.objects[i].name.clone());
        match (o.class, &o.name, owner) {
            (1, Some(n), _) => format!("{n} (GameObject)"),
            (_, _, Some(owner)) => format!("{owner} › {class}"),
            (_, Some(n), None) => format!("{n} ({class})"),
            _ => class.to_string(),
        }
    }

    /// The top-level property (indent 2) that line `n` belongs to.
    fn property_at(&self, n: u32, start: u32) -> Option<String> {
        let mut i = n as usize;
        while i >= start as usize && i >= 1 {
            let l = &self.lines[i - 1];
            let t = l.trim_start();
            if l.len() - t.len() == 2 && !t.starts_with('-') {
                return t.split(':').next().map(str::to_string);
            }
            i -= 1;
        }
        None
    }
}

fn value_of(line: &str) -> Option<String> {
    let t = line.trim();
    let (_, v) = t.split_once(':')?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    let mut short: String = v.chars().take(48).collect();
    if v.chars().count() > 48 {
        short.push('…');
    }
    Some(short)
}

/// Summarize a Unity YAML diff. `old`/`new` are the full file contents.
pub fn summarize(old: &str, new: &str, diff: &FileDiff) -> Vec<String> {
    let old_ix = index(old);
    let new_ix = index(new);
    // (object label, property) → (old value, new value, added/removed object)
    let mut order: Vec<String> = Vec::new();
    let mut changes: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();

    for h in &diff.hunks {
        for l in &h.lines {
            let (ix, n, is_new) = match (l.kind, l.old_no, l.new_no) {
                (LineKind::Add, _, Some(n)) => (&new_ix, n, true),
                (LineKind::Del, Some(n), _) => (&old_ix, n, false),
                _ => continue,
            };
            if l.text.starts_with("--- !u!") {
                if let Some(o) = ix.object_at(n) {
                    let label = ix.label(o);
                    if is_new { added.push(label) } else { removed.push(label) }
                }
                continue;
            }
            let Some(o) = ix.object_at(n) else { continue };
            let label = ix.label(o);
            if (is_new && added.contains(&label)) || (!is_new && removed.contains(&label)) {
                continue;
            }
            let prop = ix.property_at(n, o.start).unwrap_or_else(|| "?".into());
            let key = format!("{label}.{prop}");
            if !changes.contains_key(&key) {
                order.push(key.clone());
            }
            let entry = changes.entry(key).or_default();
            let indent = l.text.len() - l.text.trim_start().len();
            if indent == 2 {
                let v = value_of(&l.text);
                if is_new { entry.1 = v } else { entry.0 = v }
            }
        }
    }

    let mut out = Vec::new();
    for a in &added {
        out.push(format!("+ {a} added"));
    }
    for r in &removed {
        out.push(format!("- {r} removed"));
    }
    for key in order {
        let (o, n) = &changes[&key];
        let line = match (o, n) {
            (Some(o), Some(n)) => format!("~ {key}: {o} → {n}"),
            _ => format!("~ {key} changed"),
        };
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parse;

    const OLD: &str = "%YAML 1.1
--- !u!1 &100
GameObject:
  m_Name: InventoryChest
--- !u!4 &101
Transform:
  m_GameObject: {fileID: 100}
  m_LocalPosition: {x: 12.5, y: 0, z: -3.25}
  m_LocalScale: {x: 1, y: 1, z: 1}
--- !u!104 &2
RenderSettings:
  m_AmbientIntensity: 1
";

    #[test]
    fn transform_change_is_named_after_its_game_object() {
        let new = OLD.replace("x: 12.5", "x: 14").replace("m_AmbientIntensity: 1", "m_AmbientIntensity: 0.85")
            + "--- !u!1 &200\nGameObject:\n  m_Name: Lamp\n";
        let diff_text = "diff --git a/s.unity b/s.unity
--- a/s.unity
+++ b/s.unity
@@ -7,3 +7,3 @@ Transform:
   m_GameObject: {fileID: 100}
-  m_LocalPosition: {x: 12.5, y: 0, z: -3.25}
+  m_LocalPosition: {x: 14, y: 0, z: -3.25}
   m_LocalScale: {x: 1, y: 1, z: 1}
@@ -12,1 +12,4 @@ RenderSettings:
-  m_AmbientIntensity: 1
+  m_AmbientIntensity: 0.85
+--- !u!1 &200
+GameObject:
+  m_Name: Lamp
";
        let d = &parse(diff_text)[0];
        let s = summarize(OLD, &new, d);
        assert!(s.contains(&"+ Lamp (GameObject) added".to_string()), "{s:?}");
        assert!(s.iter().any(|l| l == "~ InventoryChest › Transform.m_LocalPosition: {x: 12.5, y: 0, z: -3.25} → {x: 14, y: 0, z: -3.25}"), "{s:?}");
        assert!(s.iter().any(|l| l == "~ RenderSettings.m_AmbientIntensity: 1 → 0.85"), "{s:?}");
        assert_eq!(s.len(), 3, "{s:?}");
    }

    #[test]
    fn extensions() {
        assert!(is_unity_yaml("Assets/Scenes/Keep.unity"));
        assert!(is_unity_yaml("Assets/UI/Slot.prefab"));
        assert!(!is_unity_yaml("Assets/Scripts/Player.cs"));
    }
}
