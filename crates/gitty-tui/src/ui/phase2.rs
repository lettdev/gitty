//! Drawing for conflicts, Insight, Repositories, rebase and stashes.

use gitty_core::insight::{Severity, human_size};
use gitty_core::rebase::{self, RebaseAction};
use gitty_core::RepoState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::{centered, footer, pane, popup_block, row_rect, scroll_to};
use crate::app::{App, Focus, Hit, RebaseForm};
use crate::theme::{SymbolSet, Symbols, Theme};
use crate::util::{clean, relative_time};

/// Shorten to `w` characters with an ellipsis.
fn fit(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_string();
    }
    let mut out: String = s.chars().take(w.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn bar(s: &Symbols, frac: f64, width: usize) -> String {
    let frac = frac.clamp(0.0, 1.0);
    if s.set == SymbolSet::Ascii {
        let n = (frac * width as f64).round() as usize;
        return format!("{}{}", "#".repeat(n), " ".repeat(width - n));
    }
    const PARTS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let total = frac * width as f64;
    let full = total.floor() as usize;
    let part = ((total - full as f64) * 8.0).floor() as usize;
    let mut out = "█".repeat(full);
    if full < width {
        out.push_str(PARTS[part]);
        out.push_str(&" ".repeat(width - full - 1));
    }
    out
}

// ------------------------------------------------------------------ unity summary

/// Draw a Unity change summary; returns the area left for the diff.
pub(super) fn split_summary(f: &mut Frame, t: &Theme, s: &Symbols, summary: &[String], area: Rect) -> Rect {
    if summary.is_empty() || area.height < 10 {
        return area;
    }
    let h = (summary.len() as u16 + 2).min(area.height * 2 / 5).max(3);
    let [top, rest] = Layout::vertical([Constraint::Length(h), Constraint::Min(0)]).areas(area);
    let block = pane(t, s, format!("Unity changes ({})", summary.len()), false);
    let inner = block.inner(top);
    f.render_widget(block, top);
    let lines: Vec<Line> = summary
        .iter()
        .take(inner.height as usize)
        .map(|l| {
            let color = match l.chars().next() {
                Some('+') => t.add,
                Some('-') => t.del,
                _ => t.modified,
            };
            Line::styled(format!(" {}", clean(l)), Style::new().fg(color))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
    rest
}

// ------------------------------------------------------------------ conflicts

pub(super) fn draw_conflict(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Conflict;
    let t = &app.theme;
    let cv = &app.conflict;
    let n = cv.remaining();
    let title = if n > 0 {
        format!("Conflict {}/{} · {}", cv.region + 1, n, clean(&cv.path))
    } else {
        format!("Conflict · {}", clean(&cv.path))
    };
    let block = pane(t, &app.sym, title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let pad = inner.inner(Margin { horizontal: 1, vertical: 0 });
    if let Some(note) = &cv.note {
        f.render_widget(Paragraph::new(clean(note)).wrap(Wrap { trim: false }), pad);
        return;
    }
    let Some(cf) = &cv.file else { return };
    let Some(r) = cf.regions.get(cv.region) else { return };

    let ctx_n = 3.min(r.start);
    let [ctx_area, cols_area, after_area] =
        Layout::vertical([Constraint::Length(ctx_n as u16), Constraint::Min(3), Constraint::Length(3)]).areas(pad);
    let ctx: Vec<Line> = cf.lines[r.start - ctx_n..r.start].iter().map(|l| Line::styled(clean(l), t.dim_style())).collect();
    f.render_widget(Paragraph::new(ctx), ctx_area);
    let after: Vec<Line> =
        cf.lines.iter().skip(r.end + 1).take(3).map(|l| Line::styled(clean(l), t.dim_style())).collect();
    f.render_widget(Paragraph::new(after), after_area);

    let mut columns: Vec<(String, &Vec<String>, ratatui::style::Color)> =
        vec![(format!("ours · {}", clean(&r.ours_label)), &r.ours, t.info)];
    if let Some(b) = &r.base {
        columns.push(("original".into(), b, t.dim));
    }
    columns.push((format!("theirs · {}", clean(&r.theirs_label)), &r.theirs, t.header));
    let constraints: Vec<Constraint> = columns.iter().map(|_| Constraint::Ratio(1, columns.len() as u32)).collect();
    let rects = Layout::horizontal(constraints).spacing(1).split(cols_area);
    for ((label, lines, color), rect) in columns.iter().zip(rects.iter()) {
        let mut out = vec![Line::styled(label.clone(), Style::new().fg(*color).add_modifier(Modifier::BOLD | Modifier::UNDERLINED))];
        if lines.is_empty() {
            out.push(Line::styled("(nothing)", t.dim_style()));
        }
        out.extend(lines.iter().map(|l| Line::styled(clean(l), Style::new().fg(*color))));
        f.render_widget(Paragraph::new(out), *rect);
    }
}

// ------------------------------------------------------------------ insight

pub(super) fn draw_insight(f: &mut Frame, app: &mut App, area: Rect) {
    if app.insight.is_none() {
        let msg = match &app.insight_error {
            Some(e) => format!("Insight failed: {}", clean(e)),
            None => format!("{} Measuring {}…", app.spinner(), clean(app.repo.name())),
        };
        let block = pane(&app.theme, &app.sym, "Insight".into(), true);
        let inner = block.inner(area);
        f.render_widget(block, area);
        f.render_widget(Paragraph::new(msg).style(app.theme.dim_style()).wrap(Wrap { trim: true }), inner.inner(Margin { horizontal: 1, vertical: 0 }));
        return;
    }
    let [left, right] = Layout::horizontal([Constraint::Percentage(42), Constraint::Min(30)]).areas(area);
    let proj_lines = project_lines(app);
    let ph = (proj_lines.len() as u16 + 2).min(left.height * 45 / 100).max(4);
    let [proj, health] = Layout::vertical([Constraint::Length(ph), Constraint::Min(4)]).areas(left);
    let block = pane(&app.theme, &app.sym, "Project".into(), false);
    let inner = block.inner(proj);
    f.render_widget(block, proj);
    f.render_widget(Paragraph::new(proj_lines).wrap(Wrap { trim: false }), inner.inner(Margin { horizontal: 1, vertical: 0 }));
    draw_health(f, app, health);

    let [sizes, blobs, hot] =
        Layout::vertical([Constraint::Percentage(42), Constraint::Percentage(30), Constraint::Min(4)]).areas(right);
    draw_sizes(f, app, sizes);
    draw_blobs(f, app, blobs);
    draw_hot(f, app, hot);
}

fn project_lines(app: &App) -> Vec<Line<'static>> {
    let t = &app.theme;
    let Some(ins) = &app.insight else { return Vec::new() };
    let mut lines = Vec::new();
    let mark = if app.sym.set == SymbolSet::Ascii { "*" } else { "◆" };
    if ins.projects.is_empty() {
        lines.push(Line::styled("No known project type detected.", t.dim_style()));
    }
    for p in ins.projects.iter().take(3) {
        let mut head = vec![
            Span::styled(format!("{mark} {}", p.kind.name()), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)),
        ];
        if let Some(v) = &p.version {
            head.push(Span::styled(format!(" {}", clean(v)), Style::new().add_modifier(Modifier::BOLD)));
        }
        if !p.dir.is_empty() {
            head.push(Span::styled(format!("  in {}", clean(&p.dir)), t.dim_style()));
        }
        lines.push(Line::from(head));
        if !p.details.is_empty() {
            lines.push(Line::styled(format!("  {}", p.details.join(" · ")), t.dim_style()));
        }
    }
    let total: u64 = ins.languages.iter().map(|l| l.bytes).sum();
    if total > 0 {
        let langs: Vec<String> =
            ins.languages.iter().take(4).map(|l| format!("{} {}%", l.name, (l.bytes * 100 / total).max(1))).collect();
        lines.push(Line::styled(langs.join(" · "), t.dim_style()));
    }
    lines.push(Line::raw(""));
    let s = &ins.size;
    lines.push(Line::from(vec![
        Span::styled("Working tree ", t.dim_style()),
        Span::styled(human_size(s.worktree), Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!(" · {} files", s.worktree_files), t.dim_style()),
    ]));
    let mut git = vec![
        Span::styled("Repository   ", t.dim_style()),
        Span::styled(human_size(s.git_dir), Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!(" · packed {}", human_size(s.packed)), t.dim_style()),
    ];
    if s.lfs_store > 0 {
        git.push(Span::styled(format!(" · LFS {}", human_size(s.lfs_store)), t.dim_style()));
    }
    lines.push(Line::from(git));
    if !ins.owners.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Most active (last year)", Style::new().fg(t.header)));
        for o in ins.owners.iter().take(3) {
            lines.push(Line::from(vec![
                Span::raw(format!("  {}", clean(&o.dir))),
                Span::styled(format!("  {} {}%", clean(&o.author), o.share), t.dim_style()),
            ]));
        }
    }
    lines
}

fn draw_health(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Health;
    let items = app.health_items();
    let warns = items.iter().filter(|h| h.severity == Severity::Warn).count();
    let title = if warns > 0 { format!("Health · {warns} to fix") } else { "Health".to_string() };
    let block = pane(&app.theme, &app.sym, title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
    let t = &app.theme;
    if items.is_empty() {
        f.render_widget(Paragraph::new(" Nothing to report.").style(t.dim_style()), inner);
        return;
    }
    app.health_sel = app.health_sel.min(items.len() - 1);
    let detail_h = (inner.height / 2).clamp(3, 9);
    let [list, detail] = Layout::vertical([Constraint::Min(1), Constraint::Length(detail_h)]).areas(inner);
    let h = list.height as usize;
    app.health_off = scroll_to(app.health_sel, app.health_off, h, items.len());
    app.hits.push((list, Hit::List(Focus::Health, app.health_off)));
    let ascii = app.sym.set == SymbolSet::Ascii;
    for (i, it) in items.iter().enumerate().skip(app.health_off).take(h) {
        let rect = row_rect(list, i - app.health_off);
        if i == app.health_sel {
            f.buffer_mut().set_style(rect, t.selection(focused));
        }
        let (icon, color) = match it.severity {
            Severity::Warn => ("!", t.warn),
            Severity::Info => ("i", t.info),
            Severity::Ok => (if ascii { "+" } else { "✓" }, t.add),
        };
        f.render_widget(
            Line::from(vec![
                Span::styled(format!(" {icon} "), Style::new().fg(color).add_modifier(Modifier::BOLD)),
                Span::raw(clean(&it.title)),
            ]),
            rect,
        );
    }
    if let Some(it) = items.get(app.health_sel) {
        let mut lines: Vec<Line> = vec![Line::raw("")];
        lines.extend(it.detail.lines().map(|l| Line::styled(clean(l), t.dim_style())));
        if it.fix.is_some() {
            lines.push(Line::styled(format!("enter {} fix this", app.sym.arrow), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), detail.inner(Margin { horizontal: 1, vertical: 0 }));
    }
}

fn draw_sizes(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Sizes;
    let path = if app.size_path.is_empty() { "repo root".to_string() } else { clean(&app.size_path) };
    let block = pane(&app.theme, &app.sym, format!("Size · {path}"), focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
    let Some(node) = app.size_node().cloned() else { return };
    let h = inner.height as usize;
    app.size_sel = app.size_sel.min(node.children.len().saturating_sub(1));
    app.size_off = scroll_to(app.size_sel, app.size_off, h, node.children.len());
    app.hits.push((inner, Hit::List(Focus::Sizes, app.size_off)));
    let t = &app.theme;
    let bar_w = 14usize;
    for (i, c) in node.children.iter().enumerate().skip(app.size_off).take(h) {
        let rect = row_rect(inner, i - app.size_off);
        if i == app.size_sel {
            f.buffer_mut().set_style(rect, t.selection(focused));
        }
        let frac = if node.size == 0 { 0.0 } else { c.size as f64 / node.size as f64 };
        let [main, meta] = Layout::horizontal([Constraint::Min(0), Constraint::Length(bar_w as u16 + 15)]).areas(rect);
        let name_style = if c.is_dir() { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
        let files = if c.is_dir() { format!("  {} file{}", c.files, if c.files == 1 { "" } else { "s" }) } else { String::new() };
        f.render_widget(Line::from(vec![Span::styled(format!(" {}", clean(&c.name)), name_style), Span::raw(if c.is_dir() { "/" } else { "" }), Span::styled(files, t.dim_style())]), main);
        f.render_widget(
            Line::from(vec![
                Span::styled(bar(&app.sym, frac, bar_w), Style::new().fg(if c.is_dir() { t.accent } else { t.info })),
                Span::raw(format!(" {:>8}", human_size(c.size))),
                Span::styled(format!(" {:>3}%", (frac * 100.0).round() as u32), t.dim_style()),
            ]),
            meta,
        );
    }
}

fn draw_blobs(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Blobs;
    let title = if app.blobs_loading { format!("Largest in history · {} scanning…", app.spinner()) } else { "Largest in history".to_string() };
    let block = pane(&app.theme, &app.sym, title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
    let t = &app.theme;
    let Some(blobs) = &app.blobs else {
        let msg = app.blobs_error.as_ref().map(|e| clean(e)).unwrap_or_else(|| " Reading every version of every file…".into());
        f.render_widget(Paragraph::new(msg).style(t.dim_style()), inner);
        return;
    };
    let h = inner.height as usize;
    app.blob_sel = app.blob_sel.min(blobs.len().saturating_sub(1));
    app.blob_off = scroll_to(app.blob_sel, app.blob_off, h, blobs.len());
    app.hits.push((inner, Hit::List(Focus::Blobs, app.blob_off)));
    for (i, b) in blobs.iter().enumerate().skip(app.blob_off).take(h) {
        let rect = row_rect(inner, i - app.blob_off);
        if i == app.blob_sel {
            f.buffer_mut().set_style(rect, t.selection(focused));
        }
        let exists = app.repo.root().join(&b.path).exists();
        let lfs = if b.lfs { Span::styled(" LFS ", Style::new().fg(t.add)) } else { Span::styled("     ", Style::new()) };
        let mut spans = vec![
            Span::raw(format!(" {:>8}", human_size(b.size))),
            lfs,
            Span::styled(clean(&b.path), if exists { Style::new() } else { t.dim_style().add_modifier(Modifier::CROSSED_OUT) }),
        ];
        if b.versions > 1 {
            spans.push(Span::styled(format!("  ×{}", b.versions), t.dim_style()));
        }
        if let Some(c) = &b.commit {
            spans.push(Span::styled(format!("  {}", &c[..c.len().min(7)]), Style::new().fg(t.hash)));
        }
        f.render_widget(Line::from(spans), rect);
    }
}

fn draw_hot(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Hot;
    let block = pane(&app.theme, &app.sym, "Hot files · 90 days".into(), focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
    let t = &app.theme;
    let Some(ins) = &app.insight else { return };
    if ins.hot.is_empty() {
        f.render_widget(Paragraph::new(" No commits in the last 90 days.").style(t.dim_style()), inner);
        return;
    }
    let h = inner.height as usize;
    app.hot_off = scroll_to(app.hot_sel, app.hot_off, h, ins.hot.len());
    app.hits.push((inner, Hit::List(Focus::Hot, app.hot_off)));
    let max = ins.hot.first().map(|x| x.commits).unwrap_or(1).max(1);
    for (i, hf) in ins.hot.iter().enumerate().skip(app.hot_off).take(h) {
        let rect = row_rect(inner, i - app.hot_off);
        if i == app.hot_sel {
            f.buffer_mut().set_style(rect, t.selection(focused));
        }
        f.render_widget(
            Line::from(vec![
                Span::styled(format!(" {}", bar(&app.sym, f64::from(hf.commits) / f64::from(max), 8)), Style::new().fg(t.warn)),
                Span::raw(format!(" {:>3} ", hf.commits)),
                Span::raw(clean(&hf.path)),
                Span::styled(format!("  {}", clean(&hf.last_author)), t.dim_style()),
            ]),
            rect,
        );
    }
}

// ------------------------------------------------------------------ repositories

pub(super) fn draw_repos(f: &mut Frame, app: &mut App, area: Rect) {
    let roots: Vec<String> = app.workspaces.iter().map(|p| clean(&p.display().to_string())).collect();
    let title = format!("Repositories · {}", roots.join(", "));
    let block = pane(&app.theme, &app.sym, title, true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.page = inner.height.saturating_sub(1) as usize;
    let t = &app.theme;
    if app.repos.is_empty() {
        let msg = if app.repos_loading {
            format!(" {} Looking for repositories…", app.spinner())
        } else {
            " No repositories found. Add folders with `workspaces = [\"~/code\"]` in the config.".to_string()
        };
        f.render_widget(Paragraph::new(msg).style(t.dim_style()).wrap(Wrap { trim: false }), inner);
        return;
    }
    let h = inner.height as usize;
    app.repo_off = scroll_to(app.repo_sel, app.repo_off, h, app.repos.len());
    app.hits.push((inner, Hit::List(Focus::Repos, app.repo_off)));
    let me = app.repo.root().canonicalize().ok();
    let name_w = app.repos.iter().map(|r| r.name.chars().count()).max().unwrap_or(10).min(28);
    for (i, r) in app.repos.iter().enumerate().skip(app.repo_off).take(h) {
        let rect = row_rect(inner, i - app.repo_off);
        if i == app.repo_sel {
            f.buffer_mut().set_style(rect, t.selection(true));
        }
        let current = r.path.canonicalize().ok() == me;
        let mut spans = vec![
            Span::styled(if current { " * " } else { "   " }, Style::new().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<name_w$}", clean(&r.name)), Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {} {:<18} ", app.sym.branch, fit(&clean(r.branch.as_deref().unwrap_or("-")), 18)), Style::new().fg(t.header)),
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
                sync.push_str(&format!("{}{} ", app.sym.ahead, r.ahead));
            }
            if r.behind > 0 {
                sync.push_str(&format!("{}{} ", app.sym.behind, r.behind));
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
}

// ------------------------------------------------------------------ stash pane

pub(super) fn draw_stash_detail(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let Some(st) = app.selected_stash() else { return };
    let block = pane(t, &app.sym, format!("Stash · {}", st.name), false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines = vec![
        Line::styled(clean(&st.message), Style::new().add_modifier(Modifier::BOLD)),
        Line::styled(format!("{}  {}", &st.oid[..st.oid.len().min(12)], relative_time(st.time)), t.dim_style()),
        Line::raw(""),
    ];
    if app.stash_files_for.as_deref() == Some(st.name.as_str()) {
        for file in &app.stash_files {
            let (label, color) = match file.status {
                '?' => ("new", t.add),
                'A' => ("A", t.add),
                'D' => ("D", t.del),
                _ => ("M", t.modified),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{label:>3} "), Style::new().fg(color).add_modifier(Modifier::BOLD)),
                Span::raw(clean(&file.path)),
            ]));
        }
    } else {
        lines.push(Line::styled("Loading…", t.dim_style()));
    }
    lines.push(Line::raw(""));
    lines.push(footer(t, &[("space", "apply"), ("o", "pop"), ("D", "drop")]));
    f.render_widget(Paragraph::new(lines), inner.inner(Margin { horizontal: 1, vertical: 0 }));
}

// ------------------------------------------------------------------ popups

pub(super) fn draw_opmenu(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let label = app.state.label().unwrap_or("Operation");
    let conflicts = app.status.entries.iter().filter(|e| e.is_conflict()).count();
    let mut lines = vec![];
    if conflicts > 0 {
        lines.push(Line::styled(format!("{conflicts} file(s) still have conflicts. Resolve and stage them first."), Style::new().fg(t.warn)));
    } else {
        lines.push(Line::styled("No conflicts left.", Style::new().fg(t.add)));
    }
    lines.push(Line::raw(""));
    lines.push(footer(t, &[("c", "continue")]));
    lines.push(footer(t, &[("a", "abort and go back")]));
    if app.state == RepoState::Rebasing {
        lines.push(footer(t, &[("s", "skip this commit")]));
    }
    lines.push(footer(t, &[("esc", "close")]));
    let rect = centered(area, 60, lines.len() as u16 + 4);
    f.render_widget(Clear, rect);
    let block = popup_block(t, &app.sym, format!("{label} in progress"), false);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(Paragraph::new(lines), inner.inner(Margin { horizontal: 1, vertical: 1 }));
}

pub(super) fn draw_rebase(f: &mut Frame, app: &App, form: &RebaseForm, area: Rect) {
    let t = &app.theme;
    let rect = centered(area, 100, area.height.saturating_sub(4));
    f.render_widget(Clear, rect);
    let block = popup_block(t, &app.sym, format!("Interactive rebase · {} commits", form.steps.len()), false);
    let inner = block.inner(rect).inner(Margin { horizontal: 1, vertical: 0 });
    f.render_widget(block, rect);
    let [list, info, foot] = Layout::vertical([Constraint::Min(3), Constraint::Length(3), Constraint::Length(2)]).areas(inner);
    let h = list.height as usize;
    let off = scroll_to(form.sel, 0, h, form.steps.len());
    for d in off..(off + h).min(form.steps.len()) {
        let s = &form.steps[form.step_index(d)];
        let rect = row_rect(list, d - off);
        if d == form.sel {
            f.buffer_mut().set_style(rect, t.selection(true));
        }
        let (color, extra) = match s.action {
            RebaseAction::Pick => (t.accent, Modifier::empty()),
            RebaseAction::Reword => (t.info, Modifier::empty()),
            RebaseAction::Squash | RebaseAction::Fixup => (t.header, Modifier::empty()),
            RebaseAction::Drop => (t.del, Modifier::CROSSED_OUT),
        };
        let subject = match (&s.action, &s.message) {
            (RebaseAction::Reword, Some(m)) => format!("{}  (was: {})", m.lines().next().unwrap_or(""), s.subject),
            _ => s.subject.clone(),
        };
        let mut spans = vec![
            Span::styled(format!(" {:<7}", s.action.label()), Style::new().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", &s.hash[..7]), Style::new().fg(t.hash)),
            Span::styled(clean(&subject), Style::new().add_modifier(extra)),
        ];
        if s.pushed {
            spans.push(Span::styled("  pushed", Style::new().fg(t.warn)));
        }
        f.render_widget(Line::from(spans), rect);
    }
    let result = rebase::preview(&form.steps);
    let pushed = form.steps.iter().filter(|s| s.pushed).count();
    let mut info_lines = vec![Line::styled(
        format!("Newest at the top. After this: {} commit(s) here.", result.len()),
        t.dim_style(),
    )];
    if pushed > 0 {
        info_lines.push(Line::styled(format!("{pushed} commit(s) are already pushed — rewriting them needs a force-push."), Style::new().fg(t.warn)));
    }
    if let Some(e) = &form.error {
        info_lines.push(Line::styled(clean(e), Style::new().fg(t.del)));
    }
    f.render_widget(Paragraph::new(info_lines), info);
    f.render_widget(
        Paragraph::new(vec![
            footer(t, &[("p", "pick"), ("r", "reword"), ("s", "squash"), ("f", "fixup"), ("d", "drop")]),
            footer(t, &[("J/K", "move down/up"), ("enter", "review"), ("esc", "cancel")]),
        ]),
        foot,
    );
}
