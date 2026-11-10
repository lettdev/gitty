//! Drawing the repository picker (start screen and Repositories view).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::{row_rect, scroll_to};
use crate::keys::{Action, Keymap};
use gitty_core::insight::human_size;
use gitty_core::preview::FolderPreview;

use crate::picker::{Picker, Prompt, Target};
use crate::theme::{Symbols, Theme};
use crate::util::{clean, relative_time};

fn fit(s: &str, w: usize) -> String {
    if s.chars().count() <= w { s.to_string() } else { s.chars().take(w.saturating_sub(1)).collect::<String>() + "…" }
}

/// 3412 → "3,412".
fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: u64, word: &str) -> String {
    format!("{} {word}{}", count(n), if n == 1 { "" } else { "s" })
}

/// What's in a folder that isn't a repository yet.
fn preview_lines(pr: &FolderPreview, t: &Theme, lines: &mut Vec<Line<'static>>) {
    let at_least = if pr.truncated { "at least " } else { "" };
    if pr.is_empty() {
        lines.push(Line::styled("Empty folder.", t.dim_style()));
        return;
    }
    let kind = pr.kind_label().map(|k| format!("{k} project")).unwrap_or_else(|| "No project type recognised".into());
    lines.push(Line::from(vec![
        Span::styled(kind, Style::new().fg(t.header).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" · {at_least}{} · {}", plural(pr.files, "file"), human_size(pr.bytes)), t.dim_style()),
    ]));
    const SHOWN: usize = 6;
    let name_w = pr.top.iter().take(SHOWN).map(|e| e.name.chars().count() + usize::from(e.is_dir)).max().unwrap_or(8).min(32);
    for e in pr.top.iter().take(SHOWN) {
        let name = format!("{}{}", clean(&e.name), if e.is_dir { "/" } else { "" });
        let mut spans = vec![
            Span::raw(format!("  {:<name_w$}  ", fit(&name, name_w))),
            Span::styled(format!("{:>9}", human_size(e.bytes)), t.dim_style()),
        ];
        if let Some(by) = &e.ignored_by {
            spans.push(Span::styled(format!("   left out ({by})"), t.dim_style()));
        }
        lines.push(Line::from(spans));
    }
    if pr.top.len() > SHOWN {
        let rest = &pr.top[SHOWN..];
        let bytes: u64 = rest.iter().map(|e| e.bytes).sum();
        lines.push(Line::styled(format!("  … {} more, {}", count(rest.len() as u64), human_size(bytes)), t.dim_style()));
    }
    if pr.kept_bytes != pr.bytes {
        lines.push(Line::raw(format!(
            "With the recommended rules, git would track {at_least}{} in {}.",
            human_size(pr.kept_bytes),
            plural(pr.kept_files, "file")
        )));
    }
    if !pr.big.is_empty() {
        let kinds = pr.lfs_patterns.iter().map(|p| p.trim_start_matches("*.").to_string()).collect::<Vec<_>>().join(", ");
        lines.push(Line::styled(
            format!("! {} ({}) — better kept in Git LFS: {kinds}", plural(pr.big.len() as u64, "large file"), human_size(pr.big.iter().map(|b| b.1).sum())),
            Style::new().fg(t.warn),
        ));
        if !pr.lfs_available {
            lines.push(Line::styled("  Git LFS isn't installed — get it from https://git-lfs.com", t.dim_style()));
        }
    }
}

fn status_lines(p: &Picker, t: &Theme, keys: &Keymap, spinner: &str) -> Vec<Line<'static>> {
    let k = |a: Action| keys.key(a);
    let mut lines = Vec::new();
    match &p.target {
        Some(Target::NotARepo { path, warning, preview }) => {
            lines.push(Line::raw(format!("{} isn't a git repository yet.", clean(&path.display().to_string()))));
            if let Some(w) = warning {
                lines.push(Line::styled(clean(w), Style::new().fg(t.warn)));
            }
            lines.push(Line::raw(""));
            match preview {
                Some(pr) => preview_lines(pr, t, &mut lines),
                None => lines.push(Line::styled(format!("{spinner} Looking through the folder…"), t.dim_style())),
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(format!("{} set up git here · or open a repository below", k(Action::InitHere)), t.dim_style()));
        }
        Some(Target::Untrusted { path, .. }) => {
            lines.push(Line::raw(format!(
                "{} is a git repository, but the folder belongs to another user account, so git won't open it.",
                clean(&path.display().to_string())
            )));
            lines.push(Line::styled(
                "Common for external drives, folders made as Administrator, or repositories copied from another computer.",
                t.dim_style(),
            ));
            lines.push(Line::styled(format!("{} trust this folder (only if you know where it came from)", k(Action::TrustFolder)), t.dim_style()));
        }
        Some(Target::Failed { path, error }) => {
            lines.push(Line::raw(format!("Couldn't open {}:", clean(&path.display().to_string()))));
            for l in error.lines().take(3) {
                lines.push(Line::styled(clean(l), Style::new().fg(t.del)));
            }
        }
        None => {}
    }
    if let Some((m, err)) = &p.message {
        lines.push(Line::styled(clean(m), Style::new().fg(if *err { t.del } else { t.accent })));
    }
    if !lines.is_empty() {
        lines.push(Line::raw(""));
    }
    lines
}

/// The prompt at the bottom: its lines, and the cursor column on the last one.
fn prompt_lines(p: &Picker, t: &Theme) -> Option<(Vec<Line<'static>>, Option<u16>)> {
    let label = |s: &str| Span::styled(s.to_string(), Style::new().fg(t.accent).add_modifier(Modifier::BOLD));
    Some(match p.prompt.as_ref()? {
        Prompt::OpenPath(input) => {
            let head = "Open folder: ";
            (vec![Line::from(vec![label(head), Span::raw(input.text.clone())])], Some(head.len() as u16 + input.cursor_x()))
        }
        Prompt::CloneUrl(input) => {
            let head = "Clone URL: ";
            let mut spans = vec![label(head), Span::raw(input.text.clone())];
            if !input.text.trim().is_empty() {
                spans.push(Span::styled(format!("  → {}", clean(&p.clone_target(input.text.trim()).display().to_string())), t.dim_style()));
            }
            (vec![Line::from(spans)], Some(head.len() as u16 + input.cursor_x()))
        }
        Prompt::ConfirmInit(c) => {
            let (path, warned) = match &p.target {
                Some(Target::NotARepo { path, warning, .. }) => (path.display().to_string(), warning.is_some()),
                _ => (String::new(), false),
            };
            let pr = p.target_preview();
            let mut lines = vec![Line::from(vec![label("Set up git in "), Span::raw(clean(&path)), label("?")])];
            let check = |on: bool| if on { "[x]" } else { "[ ]" };
            let cursor = |row: usize| if c.row == row { "›" } else { " " };
            match pr {
                None => lines.push(Line::styled("  (still looking through the folder…)", t.dim_style())),
                Some(pr) => {
                    if pr.ignore_rules.is_empty() {
                        lines.push(Line::styled(format!("{} [-] No ignore rules to add", cursor(0)), t.dim_style()));
                    } else {
                        let kind = pr.kind_label().unwrap_or_default();
                        let rules: Vec<&str> = pr.ignore_rules.iter().take(4).map(|r| r.trim_start_matches('/')).collect();
                        let more = if pr.ignore_rules.len() > 4 { ", …" } else { "" };
                        lines.push(Line::raw(format!("{} {} Add {kind} .gitignore rules ({}{more})", cursor(0), check(c.ignore), rules.join(", "))));
                    }
                    if pr.lfs_patterns.is_empty() {
                        lines.push(Line::styled(format!("{} [-] No large files for Git LFS", cursor(1)), t.dim_style()));
                    } else if !pr.lfs_available {
                        lines.push(Line::styled(format!("{} [-] Git LFS isn't installed (https://git-lfs.com)", cursor(1)), t.dim_style()));
                    } else {
                        let pats = pr.lfs_patterns.iter().take(5).cloned().collect::<Vec<_>>().join(" ");
                        lines.push(Line::raw(format!("{} {} Track {pats} with Git LFS", cursor(1), check(c.lfs))));
                    }
                }
            }
            let go = if warned { "y set up anyway" } else { "enter set up" };
            lines.push(Line::styled(format!("{go} · space toggle · ↑↓ move · esc cancel  (nothing is staged or committed)"), t.dim_style()));
            (lines, None)
        }
        Prompt::ConfirmTrust => {
            let trust = match &p.target {
                Some(Target::Untrusted { trust, .. }) => trust.clone(),
                _ => String::new(),
            };
            (
                vec![Line::from(vec![
                    label("Trust "),
                    Span::raw(clean(&trust)),
                    label("? "),
                    Span::styled("Its settings and hooks will run as you. y trust · n no".to_string(), t.dim_style()),
                ])],
                None,
            )
        }
    })
}

/// Draw the picker into `inner` (inside its frame). Returns the list area
/// and its first line, for mouse clicks.
pub fn draw_body(f: &mut Frame, p: &mut Picker, t: &Theme, sym: &Symbols, keys: &Keymap, spinner: &str, inner: Rect) -> (Rect, usize) {
    let mut status = status_lines(p, t, keys, spinner);
    if let Some(b) = &p.busy {
        status.insert(0, Line::from(vec![Span::styled(format!("{spinner} "), Style::new().fg(t.accent)), Span::raw(clean(b))]));
        status.push(Line::raw(""));
    }
    let prompt = prompt_lines(p, t);
    let prompt_h = prompt.as_ref().map(|(l, _)| l.len() as u16 + 1).unwrap_or(0);
    let status_h = (status.len() as u16).min(inner.height.saturating_sub(2 + prompt_h));
    let status_rect = Rect { height: status_h, ..inner };
    f.render_widget(Paragraph::new(status).wrap(Wrap { trim: false }), status_rect);
    let list = Rect { y: inner.y + status_h, height: inner.height.saturating_sub(status_h + prompt_h), ..inner };
    if let Some((lines, cursor)) = prompt {
        let n = lines.len() as u16;
        let r = Rect { y: inner.bottom().saturating_sub(n), height: n, ..inner };
        f.render_widget(Paragraph::new(lines), r);
        if let Some(cx) = cursor {
            f.set_cursor_position((r.x + cx.min(r.width.saturating_sub(1)), r.bottom().saturating_sub(1)));
        }
    }

    // the list: recent repositories, then nearby ones, each under a header
    let mut lines: Vec<Option<usize>> = Vec::new();
    let mut headers: Vec<(usize, String)> = Vec::new();
    let recent: Vec<usize> = (0..p.rows.len()).filter(|i| p.rows[*i].recent).collect();
    let nearby: Vec<usize> = (0..p.rows.len()).filter(|i| !p.rows[*i].recent).collect();
    if !recent.is_empty() {
        headers.push((lines.len(), "Recent".into()));
        lines.push(None);
        lines.extend(recent.iter().map(|i| Some(*i)));
    }
    if !nearby.is_empty() {
        if !lines.is_empty() {
            lines.push(None);
        }
        let roots: Vec<String> = p.roots.iter().map(|r| clean(&r.display().to_string())).collect();
        headers.push((lines.len(), format!("In {}", roots.join(", "))));
        lines.push(None);
        lines.extend(nearby.iter().map(|i| Some(*i)));
    }
    p.line_rows = lines.clone();
    if p.rows.is_empty() {
        let msg = if p.loading {
            format!("{spinner} Looking for repositories…")
        } else {
            "No repositories here yet. Open a folder (o), clone one (c), or add folders with `workspaces = [\"~/code\"]` in the config.".to_string()
        };
        f.render_widget(Paragraph::new(msg).style(t.dim_style()).wrap(Wrap { trim: false }), list);
        return (list, 0);
    }
    let h = list.height as usize;
    let sel_line = lines.iter().position(|l| *l == Some(p.sel)).unwrap_or(0);
    // keep a section header visible above the first row
    p.off = scroll_to(sel_line, p.off, h, lines.len());
    if p.off == 1 && lines.first() == Some(&None) {
        p.off = 0;
    }
    let name_w = p.rows.iter().map(|r| r.summary.name.chars().count()).max().unwrap_or(10).min(28);
    for (li, entry) in lines.iter().enumerate().skip(p.off).take(h) {
        let rect = row_rect(list, li - p.off);
        let Some(i) = entry else {
            if let Some((_, title)) = headers.iter().find(|(at, _)| *at == li) {
                f.render_widget(Line::styled(format!(" {title}"), Style::new().fg(t.header).add_modifier(Modifier::BOLD)), rect);
            }
            continue;
        };
        let r = &p.rows[*i].summary;
        if *i == p.sel {
            f.buffer_mut().set_style(rect, t.selection(true));
        }
        let current = p.is_current(&r.path);
        let mut spans = vec![
            Span::styled(if current { " * " } else { "   " }, Style::new().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<name_w$}", fit(&clean(&r.name), name_w)), Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {} {:<18} ", sym.branch, fit(&clean(r.branch.as_deref().unwrap_or("-")), 18)), Style::new().fg(t.header)),
        ];
        if let Some(e) = &r.error {
            spans.push(Span::styled(clean(e.lines().next().unwrap_or("")), Style::new().fg(t.del)));
        } else {
            let state = if r.conflicts > 0 {
                Span::styled(format!("{:<12}", format!("{} conflicts", r.conflicts)), Style::new().fg(t.warn).add_modifier(Modifier::BOLD))
            } else if r.changes > 0 {
                Span::styled(format!("{:<12}", format!("{} changed", r.changes)), Style::new().fg(t.modified))
            } else {
                Span::styled(format!("{:<12}", "clean"), Style::new().fg(t.add))
            };
            spans.push(state);
            let mut sync = String::new();
            if r.ahead > 0 {
                sync.push_str(&format!("{}{} ", sym.ahead, r.ahead));
            }
            if r.behind > 0 {
                sync.push_str(&format!("{}{} ", sym.behind, r.behind));
            }
            if !r.upstream {
                sync.push_str("local only");
            }
            spans.push(Span::styled(format!("{sync:<12}"), t.dim_style()));
            spans.push(Span::styled(format!("{:>5}  ", r.last_commit.map(relative_time).unwrap_or_default()), t.dim_style()));
        }
        spans.push(Span::styled(clean(&r.path.display().to_string()), t.dim_style()));
        f.render_widget(Line::from(spans), rect);
    }
    (list, p.off)
}

/// Bottom-bar hints for the picker.
pub fn hints(p: &Picker, keys: &Keymap) -> Vec<(String, String)> {
    let k = |a: Action| keys.key(a);
    if p.prompt.is_some() {
        return vec![("enter".into(), "ok".into()), ("esc".into(), "cancel".into())];
    }
    let mut h = Vec::new();
    match p.target {
        Some(Target::NotARepo { .. }) => h.push((k(Action::InitHere), "set up git here".into())),
        Some(Target::Untrusted { .. }) => h.push((k(Action::TrustFolder), "trust folder".into())),
        _ => {}
    }
    if !p.rows.is_empty() {
        h.push((k(Action::Open), "open".into()));
    }
    h.push((k(Action::OpenFolder), "open folder…".into()));
    h.push((k(Action::CloneRepo), "clone…".into()));
    h.push((k(Action::Refresh), "rescan".into()));
    h
}

/// The whole start screen: the cat and title, the picker, and key hints.
pub fn draw_start(f: &mut Frame, l: &mut crate::launcher::Launcher) {
    use crate::theme::SymbolSet;
    let area = f.area();
    let t = &l.theme;
    // braille needs a font that has it; the old Windows console (ascii mode) doesn't
    let show_cat = l.sym.set != SymbolSet::Ascii && area.height >= 16;
    let head_h: u16 = if show_cat { crate::cat::HEIGHT as u16 + 1 } else { 3 };
    let head = Rect { x: area.x + 1, y: area.y, width: area.width.saturating_sub(2), height: head_h.min(area.height) };
    let mut text_x = head.x + 1;
    if show_cat {
        let rows: Vec<Line> = crate::cat::frame(l.elapsed_ms()).into_iter().map(|r| Line::styled(r, Style::new().fg(t.accent))).collect();
        f.render_widget(Paragraph::new(rows), Rect { x: head.x + 1, y: head.y + 1, width: crate::cat::WIDTH as u16, height: crate::cat::HEIGHT as u16 });
        text_x = head.x + crate::cat::WIDTH as u16 + 4;
    }
    let text = vec![
        Line::from(vec![
            Span::styled("gitty ", Style::new().fg(t.header).add_modifier(Modifier::BOLD)),
            Span::styled(concat!("v", env!("CARGO_PKG_VERSION")), t.dim_style()),
        ]),
        Line::styled("Open a repository to start.", t.dim_style()),
    ];
    let text_y = head.y + if show_cat { 3 } else { 1 };
    f.render_widget(Paragraph::new(text), Rect { x: text_x, y: text_y, width: area.right().saturating_sub(text_x), height: 2 });

    let body = Rect { x: area.x, y: area.y + head.height, width: area.width, height: area.height.saturating_sub(head.height + 1) };
    let block = super::pane(t, &l.sym, "Repositories".into(), true);
    let inner = block.inner(body);
    f.render_widget(block, body);
    let spinner = l.spinner();
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    draw_body(f, &mut l.picker, t, &l.sym, &l.keys, spinner, inner);

    let mut items = hints(&l.picker, &l.keys);
    if l.picker.prompt.is_none() {
        items.push((l.keys.key(Action::Quit), "quit".into()));
    }
    let refs: Vec<(&str, &str)> = items.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let foot = Rect { x: area.x + 1, y: area.bottom().saturating_sub(1), width: area.width.saturating_sub(2), height: 1 };
    f.render_widget(super::footer(t, &refs), foot);
}
