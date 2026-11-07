//! Rendering. Drawing never talks to git; it only reads `App`.

mod phase2;

use gitty_core::diff::LineKind;
use gitty_core::log::{RefKind, RefLabel};
use gitty_core::status::{EntryKind, StatusEntry};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, BranchRow, DiffView, FileRow, Focus, Hit, InputPurpose, Popup, Section, View};
use crate::keys::{self, Action};
use crate::theme::{Symbols, Theme};
use crate::util::{clean, relative_time};

const MIN_W: u16 = 60;
const MIN_H: u16 = 12;

/// Keep `sel` visible in a window of `h` rows; returns the new offset.
fn scroll_to(sel: usize, off: usize, h: usize, len: usize) -> usize {
    if h == 0 || len == 0 {
        return 0;
    }
    let mut off = off.min(len.saturating_sub(1));
    if sel < off {
        off = sel;
    } else if sel >= off + h {
        off = sel + 1 - h;
    }
    off.min(len.saturating_sub(h))
}

fn row_rect(inner: Rect, i: usize) -> Rect {
    Rect { x: inner.x, y: inner.y + i as u16, width: inner.width, height: 1 }
}

fn pane<'a>(t: &Theme, s: &Symbols, title: String, focused: bool) -> Block<'a> {
    let title_style = if focused {
        Style::new().fg(t.accent).add_modifier(Modifier::BOLD)
    } else {
        t.dim_style()
    };
    Block::bordered()
        .border_set(s.border)
        .border_style(Style::new().fg(if focused { t.accent } else { t.border }))
        .title(Span::styled(format!(" {title} "), title_style))
}

fn split_path(path: &str) -> (String, String) {
    let trimmed = path.trim_end_matches('/');
    let dir_marker = if path.ends_with('/') { "/" } else { "" };
    match trimmed.rsplit_once('/') {
        Some((d, n)) => (format!("{}{dir_marker}", clean(n)), clean(d)),
        None => (format!("{}{dir_marker}", clean(trimmed)), String::new()),
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.hits.clear();
    if area.width < MIN_W || area.height < MIN_H {
        let msg = format!("gitty needs at least {MIN_W}×{MIN_H} (now {}×{})", area.width, area.height);
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), area);
        return;
    }
    let [top, body, bottom] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(area);
    draw_top(f, app, top);
    if !app.loaded_once {
        let p = Paragraph::new(format!("{} Loading {}…", app.spinner(), app.repo.name())).style(app.theme.dim_style());
        f.render_widget(p, body.inner(ratatui::layout::Margin { horizontal: 2, vertical: 1 }));
    } else {
        match app.view {
            View::Changes => draw_changes(f, app, body),
            View::History => draw_history(f, app, body),
            View::Branches => draw_branches(f, app, body),
            View::Insight => phase2::draw_insight(f, app, body),
            View::Repos => phase2::draw_repos(f, app, body),
        }
    }
    draw_bottom(f, app, bottom);
    if app.popup.is_some() {
        draw_popup(f, app, area);
    }
}

// ------------------------------------------------------------------- top bar

fn draw_top(f: &mut Frame, app: &mut App, area: Rect) {
    let t = &app.theme;
    let s = &app.sym;
    let b = &app.status.branch;
    let mut spans = vec![
        Span::styled(" gitty ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(clean(app.repo.name()), Style::new().add_modifier(Modifier::BOLD)),
        Span::raw("  "),
    ];
    let head = match (&b.head, &b.oid) {
        (Some(h), _) => format!("{} {}", s.branch, clean(h)),
        (None, Some(o)) => format!("{} detached @ {}", s.branch, &o[..o.len().min(7)]),
        (None, None) => format!("{} (no branch)", s.branch),
    };
    spans.push(Span::styled(head, Style::new().fg(t.header)));
    if b.upstream.is_some() {
        if b.ahead == 0 && b.behind == 0 {
            spans.push(Span::styled(format!(" {}", s.synced), t.dim_style()));
        }
        if b.ahead > 0 {
            spans.push(Span::styled(format!(" {}{}", s.ahead, b.ahead), Style::new().fg(t.add)));
        }
        if b.behind > 0 {
            spans.push(Span::styled(format!(" {}{}", s.behind, b.behind), Style::new().fg(t.del)));
        }
    } else if b.head.is_some() && b.oid.is_some() {
        spans.push(Span::styled("  not pushed yet", t.dim_style()));
    }
    if let Some(label) = app.state.label() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!(" {label} "), Style::new().fg(t.warn).add_modifier(Modifier::BOLD | Modifier::REVERSED)));
    }
    let spans_copy = spans.clone();
    f.render_widget(Line::from(spans_copy), area);

    let wide = [(View::Changes, "1 Changes"), (View::History, "2 History"), (View::Branches, "3 Branches"), (View::Insight, "4 Insight"), (View::Repos, "5 Repos")];
    let narrow = [(View::Changes, "1 Chg"), (View::History, "2 Hist"), (View::Branches, "3 Br"), (View::Insight, "4 Ins"), (View::Repos, "5 Rep")];
    let left_w: u16 = spans.iter().map(|s| s.content.width() as u16).sum();
    let wide_w: u16 = wide.iter().map(|(_, l)| l.len() as u16 + 2).sum();
    let tabs = if left_w + wide_w + 2 <= area.width { wide } else { narrow };
    let total: u16 = tabs.iter().map(|(_, l)| l.len() as u16 + 2).sum();
    let mut x = area.right().saturating_sub(total + 1);
    for (v, label) in tabs {
        let w = label.len() as u16 + 2;
        let rect = Rect { x, y: area.y, width: w, height: 1 };
        let style = if app.view == v {
            Style::new().fg(t.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            t.dim_style()
        };
        f.render_widget(Span::styled(format!(" {label} "), style), rect);
        app.hits.push((rect, Hit::Tab(v)));
        x += w;
    }
}

// ------------------------------------------------------------------- bottom bar

fn hints(app: &App) -> Vec<(String, String)> {
    let k = |a: Action| app.keys.key(a);
    let staged_diff = app.diff.section == Some(Section::Staged);
    let mut h: Vec<(String, String)> = match app.focus {
        Focus::Files => {
            let stage = match app.selected_entry().map(|e| e.0) {
                Some(Section::Staged) => "unstage",
                _ => "stage",
            };
            vec![
                (k(Action::Stage), stage.into()),
                (k(Action::StageAll), "all".into()),
                (k(Action::Commit), "commit".into()),
                (k(Action::Discard), "discard".into()),
                (k(Action::Open), if app.conflict_focus_target() == Focus::Conflict { "resolve".into() } else { "diff".into() }),
            ]
        }
        Focus::Diff => {
            let target = if app.diff.anchor.is_some() { "lines" } else { "hunk" };
            let verb = if staged_diff { "unstage" } else { "stage" };
            vec![
                (k(Action::Stage), format!("{verb} {target}")),
                (k(Action::SelectLines), if app.diff.anchor.is_some() { "end select".into() } else { "select lines".into() }),
                (k(Action::NextHunk), "next hunk".into()),
                (k(Action::Discard), "discard".into()),
                (k(Action::Back), "back".into()),
            ]
        }
        Focus::Commits => vec![
            (k(Action::Open), "files".into()),
            (k(Action::Rebase), "rebase from here".into()),
            (k(Action::ToggleAllBranches), if app.log_all { "current only".into() } else { "all branches".into() }),
            (k(Action::CopyHash), "copy hash".into()),
            (k(Action::NewBranch), "branch here".into()),
        ],
        Focus::CommitFiles => vec![(k(Action::Open), "diff".into()), (k(Action::Back), "back".into())],
        Focus::CommitDiff => vec![(k(Action::NextHunk), "next hunk".into()), (k(Action::Back), "back".into())],
        Focus::Branches if app.selected_stash().is_some() => vec![
            (k(Action::Checkout), "apply".into()),
            (k(Action::StashPop), "pop".into()),
            (k(Action::DeleteBranch), "drop".into()),
        ],
        Focus::Branches => vec![
            (k(Action::Checkout), "switch".into()),
            (k(Action::NewBranch), "new".into()),
            (k(Action::RenameBranch), "rename".into()),
            (k(Action::DeleteBranch), "delete".into()),
            (k(Action::StashPush), "stash".into()),
        ],
        Focus::Conflict => vec![
            (k(Action::TakeOurs), "ours".into()),
            (k(Action::TakeTheirs), "theirs".into()),
            (k(Action::TakeBoth), "both".into()),
            (k(Action::ShowBase), "show original".into()),
            (k(Action::NextHunk), "next".into()),
            (k(Action::Stage), "mark resolved".into()),
            (k(Action::EditFile), "editor".into()),
        ],
        Focus::Health => vec![(k(Action::Open), "fix".into()), (k(Action::NextFocus), "next pane".into())],
        Focus::Sizes => vec![(k(Action::Open), "open folder".into()), (k(Action::Back), "up".into())],
        Focus::Blobs => vec![(k(Action::Open), "go to commit".into()), (k(Action::Refresh), "rescan".into())],
        Focus::Hot => vec![(k(Action::NextFocus), "next pane".into())],
        Focus::Repos => vec![(k(Action::Open), "open".into()), (k(Action::Refresh), "rescan".into())],
    };
    if app.state != gitty_core::RepoState::Clean {
        h.insert(0, (k(Action::OpMenu), format!("{} menu", app.state.label().unwrap_or("").to_lowercase())));
    }
    if app.last_op.is_some() {
        h.push((k(Action::Undo), "undo".into()));
    }
    h.push((k(Action::Help), "help".into()));
    h
}

fn draw_bottom(f: &mut Frame, app: &mut App, area: Rect) {
    let t = &app.theme;
    let right: Vec<Span> = if app.busy > 0 {
        vec![
            Span::styled(app.spinner(), Style::new().fg(t.accent)),
            Span::raw(" "),
            Span::raw(clean(app.progress.as_deref().unwrap_or("Working…"))),
        ]
    } else if let Some(n) = &app.notice {
        vec![Span::styled(clean(&n.text), Style::new().fg(if n.error { t.del } else { t.accent }))]
    } else if !app.last_cmd.is_empty() {
        vec![Span::styled(format!("{} ", app.sym.prompt), t.dim_style()), Span::styled(clean(&app.last_cmd), t.dim_style())]
    } else {
        vec![]
    };
    let right_w = right.iter().map(|s| s.content.width()).sum::<usize>().min(area.width as usize * 3 / 5) as u16;
    let [left_area, right_area] = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w + 1)]).areas(area);
    let mut spans = vec![Span::raw(" ")];
    for (key, label) in hints(app) {
        spans.push(Span::styled(key, Style::new().fg(t.info).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {label}  "), t.dim_style()));
    }
    f.render_widget(Line::from(spans), left_area);
    f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right), right_area);
}

// ------------------------------------------------------------------- changes

fn status_letter(t: &Theme, sec: Section, e: &StatusEntry) -> Span<'static> {
    let (text, color) = match sec {
        Section::Conflicts => (format!("{}{}", e.index, e.worktree), t.warn),
        Section::Unstaged => match e.kind {
            EntryKind::Untracked => ("??".into(), t.add),
            _ => (format!(" {}", e.worktree), if e.worktree == 'D' { t.del } else { t.modified }),
        },
        Section::Staged => {
            let c = match e.index {
                'A' => t.add,
                'D' => t.del,
                'R' | 'C' => t.info,
                _ => t.modified,
            };
            (format!(" {}", e.index), c)
        }
    };
    Span::styled(text, Style::new().fg(color).add_modifier(Modifier::BOLD))
}

fn draw_changes(f: &mut Frame, app: &mut App, area: Rect) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(38), Constraint::Min(20)]).areas(area);
    draw_files(f, app, left);
    let staged = app.diff.section == Some(Section::Staged);
    let title = if app.diff.path.is_empty() {
        "Diff".to_string()
    } else {
        format!("Diff · {}{}", clean(&app.diff.path), if staged { " (staged)" } else { "" })
    };
    if app.selected_entry().is_some_and(|(s, _)| s == Section::Conflicts) {
        phase2::draw_conflict(f, app, right);
        return;
    }
    let focused = app.focus == Focus::Diff;
    let right = phase2::split_summary(f, &app.theme, &app.sym, &app.diff.summary, right);
    let inner = render_diff(f, &app.theme, &app.sym, &mut app.diff, right, focused, title);
    app.hits.push((inner, Hit::List(Focus::Diff, app.diff.scroll)));
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
}

fn draw_files(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Files;
    let n = app.status.entries.len();
    let title = if n == 0 { "Files".to_string() } else { format!("Files ({n})") };
    let block = pane(&app.theme, &app.sym, title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
    if app.file_rows.is_empty() {
        let p = Paragraph::new(vec![
            Line::styled(format!("{} Working tree clean", app.sym.synced), Style::new().fg(app.theme.accent)),
            Line::raw(""),
            Line::styled("Nothing to commit. Edit some files and they show up here.", app.theme.dim_style()),
        ])
        .wrap(Wrap { trim: true });
        f.render_widget(p, inner.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 }));
        return;
    }
    let h = inner.height as usize;
    app.file_off = scroll_to(app.file_sel, app.file_off, h, app.file_rows.len());
    app.hits.push((inner, Hit::List(Focus::Files, app.file_off)));
    let t = &app.theme;
    for (i, row) in app.file_rows.iter().enumerate().skip(app.file_off).take(h) {
        let rect = row_rect(inner, i - app.file_off);
        let line = match *row {
            FileRow::Header(sec, count) => Line::from(vec![
                Span::styled(format!(" {}", sec.title()), Style::new().fg(t.header).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {count}"), t.dim_style()),
            ]),
            FileRow::File(sec, idx) => {
                let e = &app.status.entries[idx];
                let (name, dir) = split_path(&e.path);
                let mut spans = vec![Span::raw(" "), status_letter(t, sec, e), Span::raw(" "), Span::raw(name)];
                if let Some(o) = &e.orig_path {
                    spans.push(Span::styled(format!(" ← {}", clean(o)), t.dim_style()));
                }
                if !dir.is_empty() {
                    spans.push(Span::styled(format!("  {dir}"), t.dim_style()));
                }
                Line::from(spans)
            }
        };
        if i == app.file_sel {
            f.buffer_mut().set_style(rect, t.selection(focused));
        }
        f.render_widget(line, rect);
    }
}

/// Draw a diff pane; returns the inner (rows) area.
fn render_diff(f: &mut Frame, t: &Theme, s: &Symbols, dv: &mut DiffView, area: Rect, focused: bool, title: String) -> Rect {
    let block = pane(t, s, title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let h = inner.height as usize;
    let pad = inner.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });

    if let Some(raw) = &dv.raw {
        dv.scroll = scroll_to(dv.cursor, dv.scroll, h, raw.len());
        for (i, l) in raw.iter().enumerate().skip(dv.scroll).take(h) {
            let rect = row_rect(inner, i - dv.scroll);
            let style = if l.starts_with("++<<<<<<<") || l.starts_with("++=======") || l.starts_with("++>>>>>>>") {
                Style::new().fg(t.warn).add_modifier(Modifier::BOLD)
            } else if l.starts_with("@@@") {
                Style::new().fg(t.info)
            } else if l.starts_with('+') || l.starts_with(" +") {
                Style::new().fg(t.add)
            } else if l.starts_with('-') || l.starts_with(" -") {
                Style::new().fg(t.del)
            } else {
                Style::new()
            };
            if focused && i == dv.cursor {
                f.buffer_mut().set_style(rect, t.selection(true));
            }
            f.render_widget(Line::styled(format!(" {}", clean(l)), style), rect);
        }
        return inner;
    }
    if let Some(note) = &dv.note {
        f.render_widget(Paragraph::new(clean(note)).style(t.dim_style()).wrap(Wrap { trim: true }), pad);
        return inner;
    }
    let Some(d) = &dv.diff else {
        f.render_widget(Paragraph::new("Select a file to see its changes.").style(t.dim_style()), pad);
        return inner;
    };
    let w = d.max_line_no().max(1).to_string().len();
    dv.scroll = scroll_to(dv.cursor, dv.scroll, h, dv.flat.len());
    let range = dv.selection_range();
    for i in 0..h {
        let fi = dv.scroll + i;
        let Some(&(hi, li)) = dv.flat.get(fi) else { break };
        let rect = row_rect(inner, i);
        let hunk = &d.hunks[hi];
        let in_range = range.is_some_and(|(a, b)| fi >= a && fi <= b);
        let line = match li {
            None => {
                if let Some(bg) = t.hunk_bg {
                    f.buffer_mut().set_style(rect, Style::new().bg(bg));
                }
                Line::from(Span::styled(format!(" {}", clean(&hunk.header)), Style::new().fg(t.info)))
            }
            Some(li) => {
                let l = &hunk.lines[li];
                let (sign, fg, bg) = match l.kind {
                    LineKind::Add => ("+", Some(t.add), t.add_bg),
                    LineKind::Del => ("-", Some(t.del), t.del_bg),
                    LineKind::Context => (" ", None, None),
                    LineKind::NoNewline => ("", Some(t.dim), None),
                };
                if let Some(bg) = bg {
                    f.buffer_mut().set_style(rect, Style::new().bg(bg));
                }
                let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                let text_style = fg.map(|c| Style::new().fg(c)).unwrap_or_default();
                Line::from(vec![
                    Span::styled(if in_range { s.bar } else { " " }, Style::new().fg(t.warn)),
                    Span::styled(format!("{:>w$} {:>w$} ", num(l.old_no), num(l.new_no)), t.dim_style()),
                    Span::styled(sign, text_style.add_modifier(Modifier::BOLD)),
                    Span::styled(clean(&l.text), text_style),
                ])
            }
        };
        if in_range {
            f.buffer_mut().set_style(rect, t.range());
        }
        if focused && fi == dv.cursor {
            f.buffer_mut().set_style(rect, t.selection(true));
        }
        f.render_widget(line, rect);
    }
    if d.truncated && dv.scroll + h >= dv.flat.len() && h > 0 {
        let rect = row_rect(inner, h - 1);
        f.render_widget(Line::styled(" … diff truncated (file too large)", t.dim_style()), rect);
    }
    inner
}

// ------------------------------------------------------------------- history

fn ref_spans(t: &Theme, refs: &[RefLabel], arrow: &str) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    for r in refs {
        let (text, style) = match r.kind {
            RefKind::Head if r.name == "HEAD" => ("HEAD".to_string(), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)),
            RefKind::Head => (format!("HEAD {arrow} {}", clean(&r.name)), Style::new().fg(t.accent).add_modifier(Modifier::BOLD)),
            RefKind::Local => (clean(&r.name), Style::new().fg(t.header).add_modifier(Modifier::BOLD)),
            RefKind::Remote => (clean(&r.name), Style::new().fg(t.info)),
            RefKind::Tag => (format!("tag {}", clean(&r.name)), Style::new().fg(t.warn)),
        };
        out.push(Span::styled(format!("({text})"), style));
        out.push(Span::raw(" "));
    }
    out
}

fn draw_history(f: &mut Frame, app: &mut App, area: Rect) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Min(30)]).areas(area);
    draw_commits(f, app, left);

    let info_lines = commit_info_lines(app, right.width.saturating_sub(4) as usize);
    let n_files = app.detail.as_ref().map(|d| d.files.len()).unwrap_or(0);
    let third = right.height / 3;
    let info_h = (info_lines.len() as u16 + 2).clamp(4, third.max(4));
    let files_h = (n_files as u16 + 2).clamp(3, third.max(3));
    let [info, files, diff] =
        Layout::vertical([Constraint::Length(info_h), Constraint::Length(files_h), Constraint::Min(3)]).areas(right);

    let block = pane(&app.theme, &app.sym, "Commit".into(), false);
    let inner = block.inner(info);
    f.render_widget(block, info);
    f.render_widget(Paragraph::new(info_lines), inner.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 }));

    draw_commit_files(f, app, files);
    let title = if app.cdiff.path.is_empty() { "Diff".to_string() } else { format!("Diff · {}", clean(&app.cdiff.path)) };
    let focused = app.focus == Focus::CommitDiff;
    let diff = phase2::split_summary(f, &app.theme, &app.sym, &app.cdiff.summary, diff);
    let inner = render_diff(f, &app.theme, &app.sym, &mut app.cdiff, diff, focused, title);
    app.hits.push((inner, Hit::List(Focus::CommitDiff, app.cdiff.scroll)));
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
}

fn commit_info_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let t = &app.theme;
    let Some(d) = &app.detail else {
        return vec![Line::styled(if app.commits.is_empty() { "No commits yet." } else { "Loading…" }, t.dim_style())];
    };
    let mut lines = Vec::new();
    let mut first = vec![Span::styled(d.hash[..d.hash.len().min(12)].to_string(), Style::new().fg(t.hash)), Span::raw("  ")];
    if let Some(c) = app.commits.iter().find(|c| c.hash == d.hash) {
        first.extend(ref_spans(t, &c.refs, app.sym.arrow));
    }
    lines.push(Line::from(first));
    let mut meta = vec![
        Span::styled(clean(&d.author), Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {}  <{}>", d.date, clean(&d.email)), t.dim_style()),
    ];
    if d.parents.len() > 1 {
        let ps: Vec<&str> = d.parents.iter().map(|p| &p[..p.len().min(7)]).collect();
        meta.push(Span::styled(format!("  merge of {}", ps.join(" + ")), Style::new().fg(t.info)));
    }
    lines.push(Line::from(meta));
    for (i, l) in d.message.lines().enumerate() {
        let mut text = clean(l);
        if text.width() > width && width > 1 {
            text = text.chars().take(width.saturating_sub(1)).collect::<String>() + "…";
        }
        lines.push(if i == 0 { Line::styled(text, Style::new().add_modifier(Modifier::BOLD)) } else { Line::raw(text) });
    }
    lines
}

fn draw_commits(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Commits;
    let more = if app.log_exhausted { "" } else { "+" };
    let scope = if app.log_all { "all branches" } else { "current branch" };
    let block = pane(&app.theme, &app.sym, format!("Commits · {scope} ({}{more})", app.commits.len()), focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
    if app.commits.is_empty() {
        f.render_widget(Paragraph::new(" No commits yet.").style(app.theme.dim_style()), inner);
        return;
    }
    let h = inner.height as usize;
    app.commit_off = scroll_to(app.commit_sel, app.commit_off, h, app.commits.len());
    app.hits.push((inner, Hit::List(Focus::Commits, app.commit_off)));
    let t = &app.theme;
    let s = &app.sym;
    let meta_w: u16 = 18;
    for i in app.commit_off..(app.commit_off + h).min(app.commits.len()) {
        let c = &app.commits[i];
        let rect = row_rect(inner, i - app.commit_off);
        if i == app.commit_sel {
            f.buffer_mut().set_style(rect, t.selection(focused));
        }
        let [main, meta] = Layout::horizontal([Constraint::Min(0), Constraint::Length(meta_w)]).areas(rect);
        let mut spans = vec![Span::raw(" "), Span::styled(c.short().to_string(), Style::new().fg(t.hash)), Span::raw(" ")];
        let cells = &app.graph[i];
        let max_cells = 24;
        for cell in cells.iter().take(max_cells) {
            let ch = if cell.ch == '●' && c.is_head() { s.head } else { s.graph(cell.ch) };
            spans.push(Span::styled(ch.to_string(), Style::new().fg(t.lane(cell.lane))));
        }
        if cells.len() > max_cells {
            spans.push(Span::styled("…", t.dim_style()));
        }
        spans.push(Span::raw(" "));
        spans.extend(ref_spans(t, &c.refs, s.arrow));
        spans.push(Span::raw(clean(&c.subject)));
        f.render_widget(Line::from(spans), main);
        let author: String = clean(&c.author).chars().take(11).collect();
        f.render_widget(
            Line::styled(format!(" {author:<11} {:>4} ", relative_time(c.time)), t.dim_style()),
            meta,
        );
    }
    if !app.log_exhausted && app.commit_off + h >= app.commits.len() && h > 0 {
        let rect = row_rect(inner, h - 1);
        f.render_widget(Line::styled(format!(" {} loading more…", app.spinner()), t.dim_style()), rect);
    }
}

fn draw_commit_files(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::CommitFiles;
    let n = app.detail.as_ref().map(|d| d.files.len()).unwrap_or(0);
    let block = pane(&app.theme, &app.sym, format!("Files ({n})"), focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if focused {
        app.page = inner.height.saturating_sub(1) as usize;
    }
    let Some(d) = &app.detail else { return };
    let h = inner.height as usize;
    app.cfile_off = scroll_to(app.cfile_sel, app.cfile_off, h, d.files.len());
    app.hits.push((inner, Hit::List(Focus::CommitFiles, app.cfile_off)));
    let t = &app.theme;
    for (i, file) in d.files.iter().enumerate().skip(app.cfile_off).take(h) {
        let rect = row_rect(inner, i - app.cfile_off);
        if i == app.cfile_sel {
            f.buffer_mut().set_style(rect, t.selection(focused));
        }
        let color = match file.status {
            'A' => t.add,
            'D' => t.del,
            'R' | 'C' => t.info,
            _ => t.modified,
        };
        let (name, dir) = split_path(&file.path);
        let mut spans = vec![
            Span::styled(format!(" {} ", file.status), Style::new().fg(color).add_modifier(Modifier::BOLD)),
            Span::raw(name),
        ];
        if let Some(o) = &file.orig {
            spans.push(Span::styled(format!(" ← {}", clean(o)), t.dim_style()));
        }
        if !dir.is_empty() {
            spans.push(Span::styled(format!("  {dir}"), t.dim_style()));
        }
        match (file.adds, file.dels) {
            (Some(a), Some(dl)) => {
                spans.push(Span::styled(format!("  +{a}"), Style::new().fg(t.add)));
                spans.push(Span::styled(format!(" -{dl}"), Style::new().fg(t.del)));
            }
            _ => spans.push(Span::styled("  bin", t.dim_style())),
        }
        f.render_widget(Line::from(spans), rect);
    }
}

// ------------------------------------------------------------------- branches

fn draw_branches(f: &mut Frame, app: &mut App, area: Rect) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Min(30)]).areas(area);
    let title = if app.stashes.is_empty() {
        format!("Branches ({})", app.branches.len())
    } else {
        format!("Branches ({}) · stashes ({})", app.branches.len(), app.stashes.len())
    };
    let block = pane(&app.theme, &app.sym, title, true);
    let inner = block.inner(left);
    f.render_widget(block, left);
    app.page = inner.height.saturating_sub(1) as usize;
    let h = inner.height as usize;
    app.branch_off = scroll_to(app.branch_sel, app.branch_off, h, app.branch_rows.len());
    app.hits.push((inner, Hit::List(Focus::Branches, app.branch_off)));
    let t = &app.theme;
    let s = &app.sym;
    for (i, row) in app.branch_rows.iter().enumerate().skip(app.branch_off).take(h) {
        let rect = row_rect(inner, i - app.branch_off);
        if i == app.branch_sel {
            f.buffer_mut().set_style(rect, t.selection(true));
        }
        match *row {
            BranchRow::Header(title, n) => {
                f.render_widget(
                    Line::from(vec![
                        Span::styled(format!(" {title}"), Style::new().fg(t.header).add_modifier(Modifier::BOLD)),
                        Span::styled(format!(" {n}"), t.dim_style()),
                    ]),
                    rect,
                );
            }
            BranchRow::Branch(bi) => {
                let b = &app.branches[bi];
                let [main, meta] = Layout::horizontal([Constraint::Min(0), Constraint::Length(6)]).areas(rect);
                let marker = if b.head { Span::styled(" * ", Style::new().fg(t.accent).add_modifier(Modifier::BOLD)) } else { Span::raw("   ") };
                let name_style = if b.head {
                    Style::new().fg(t.accent).add_modifier(Modifier::BOLD)
                } else if b.remote {
                    Style::new().fg(t.info)
                } else {
                    Style::new()
                };
                let mut spans = vec![marker, Span::styled(clean(&b.name), name_style)];
                if b.gone {
                    spans.push(Span::styled("  upstream gone", Style::new().fg(t.warn)));
                } else if b.upstream.is_some() {
                    if b.ahead > 0 {
                        spans.push(Span::styled(format!(" {}{}", s.ahead, b.ahead), Style::new().fg(t.add)));
                    }
                    if b.behind > 0 {
                        spans.push(Span::styled(format!(" {}{}", s.behind, b.behind), Style::new().fg(t.del)));
                    }
                }
                spans.push(Span::styled(format!("  {}", clean(&b.subject)), t.dim_style()));
                f.render_widget(Line::from(spans), main);
                f.render_widget(Line::styled(format!("{:>5} ", relative_time(b.time)), t.dim_style()), meta);
            }
            BranchRow::Stash(si) => {
                let st = &app.stashes[si];
                let [main, meta] = Layout::horizontal([Constraint::Min(0), Constraint::Length(6)]).areas(rect);
                f.render_widget(
                    Line::from(vec![
                        Span::styled(format!("   {} ", st.name), Style::new().fg(t.warn)),
                        Span::raw(clean(&st.message)),
                    ]),
                    main,
                );
                f.render_widget(Line::styled(format!("{:>5} ", relative_time(st.time)), t.dim_style()), meta);
            }
        }
    }
    if app.selected_stash().is_some() {
        phase2::draw_stash_detail(f, app, right);
        return;
    }

    let title = app.selected_branch().map(|b| format!("Log · {}", clean(&b.name))).unwrap_or_else(|| "Log".into());
    let block = pane(t, s, title, false);
    let inner = block.inner(right);
    f.render_widget(block, right);
    let mut lines = Vec::new();
    if let Some(b) = app.selected_branch() {
        let mut info = vec![Span::styled(b.oid.clone(), Style::new().fg(t.hash))];
        match &b.upstream {
            Some(u) => info.push(Span::styled(format!("  tracks {}", clean(u)), t.dim_style())),
            None if !b.remote => info.push(Span::styled("  no upstream", t.dim_style())),
            None => {}
        }
        lines.push(Line::from(info));
        lines.push(Line::raw(""));
    }
    if app.branch_log_for.as_deref() == app.selected_branch().map(|b| b.name.as_str()) {
        for c in &app.branch_log {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", c.short()), Style::new().fg(t.hash)),
                Span::raw(clean(&c.subject)),
                Span::styled(format!("  {} · {}", clean(&c.author), relative_time(c.time)), t.dim_style()),
            ]));
        }
    } else {
        lines.push(Line::styled("Loading…", t.dim_style()));
    }
    f.render_widget(Paragraph::new(lines), inner.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 }));
}

// ------------------------------------------------------------------- popups

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 3, width: w, height: h }
}

fn popup_block<'a>(t: &Theme, s: &Symbols, title: String, error: bool) -> Block<'a> {
    let color = if error { t.del } else { t.accent };
    Block::bordered()
        .border_set(s.border)
        .border_style(Style::new().fg(color))
        .title(Span::styled(format!(" {title} "), Style::new().fg(color).add_modifier(Modifier::BOLD)))
}

fn footer(t: &Theme, items: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (k, l) in items {
        spans.push(Span::styled(k.to_string(), Style::new().fg(t.info).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {l}   "), t.dim_style()));
    }
    Line::from(spans)
}

fn help_lines(app: &App) -> Vec<Line<'static>> {
    let groups: [(&str, &[Action]); 10] = [
        ("Everywhere", keys::GLOBAL),
        ("Files", keys::FILES),
        ("Diff", keys::DIFF),
        ("Conflicts", keys::CONFLICT),
        ("History", keys::COMMITS),
        ("Commit files", keys::COMMIT_FILES),
        ("Branches & stashes", keys::BRANCHES),
        ("Insight health", keys::HEALTH),
        ("Insight lists", keys::INSIGHT_LIST),
        ("Commit message", &[]),
    ];
    let t = &app.theme;
    let mut lines = Vec::new();
    let nav = [Action::Up, Action::Down, Action::PageUp, Action::PageDown, Action::Top, Action::Bottom];
    for (title, actions) in groups {
        lines.push(Line::styled(title.to_string(), Style::new().fg(t.header).add_modifier(Modifier::BOLD)));
        if title == "Commit message" {
            for (k, d) in [("enter", "Commit (in the summary field)"), ("tab", "Switch summary / description"), ("ctrl+s", "Commit from anywhere"), ("esc", "Cancel (keeps a draft)")] {
                lines.push(Line::from(vec![Span::styled(format!("  {k:<16}"), Style::new().fg(t.info)), Span::raw(d)]));
            }
            continue;
        }
        for a in actions.iter().filter(|a| title == "Files" || !nav.contains(a)) {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<16}", app.keys.all_keys(*a)), Style::new().fg(t.info)),
                Span::raw(keys::info(*a).desc),
            ]));
        }
        lines.push(Line::raw(""));
    }
    let cfg = crate::config::path().map(|p| p.display().to_string()).unwrap_or_default();
    lines.push(Line::styled(format!("Change keys in {cfg}"), t.dim_style()));
    lines
}

fn draw_popup(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let s = &app.sym;
    let Some(popup) = &app.popup else { return };
    match popup {
        Popup::Help { scroll } => {
            let rect = centered(area, 76, area.height.saturating_sub(4));
            f.render_widget(Clear, rect);
            let block = popup_block(t, s, "Keys".into(), false);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let [body, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(Paragraph::new(help_lines(app)).scroll((*scroll, 0)), body.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 }));
            f.render_widget(footer(t, &[("j/k", "scroll"), ("esc", "close")]), foot);
        }
        Popup::Confirm { title, message, .. } => {
            let lines: Vec<Line> = message.lines().map(|l| Line::raw(clean(l))).collect();
            let rect = centered(area, 64, lines.len() as u16 + 5);
            f.render_widget(Clear, rect);
            let block = popup_block(t, s, title.clone(), false);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let [body, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 }));
            f.render_widget(footer(t, &[(" y", "confirm"), ("n", "cancel")]), foot);
        }
        Popup::Message { title, text, error, scroll, retry } => {
            let lines: Vec<Line> = text.lines().map(|l| Line::raw(clean(l))).collect();
            let rect = centered(area, 84, lines.len() as u16 + 5);
            f.render_widget(Clear, rect);
            let block = popup_block(t, s, title.clone(), *error);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let [body, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(
                Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((*scroll, 0)),
                body.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 }),
            );
            let mut items = vec![(" esc", "close"), ("j/k", "scroll")];
            if retry.is_some() {
                items.push(("!", "run in terminal"));
            }
            f.render_widget(footer(t, &items), foot);
        }
        Popup::OpMenu => phase2::draw_opmenu(f, app, area),
        Popup::Rebase(form) => phase2::draw_rebase(f, app, form, area),
        Popup::ConfirmRun { title, message, .. } => {
            let lines: Vec<Line> = message.lines().map(|l| Line::raw(clean(l))).collect();
            let rect = centered(area, 76, lines.len() as u16 + 6);
            f.render_widget(Clear, rect);
            let block = popup_block(t, s, title.clone(), true);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let [body, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 }));
            f.render_widget(footer(t, &[(" y", "run in terminal"), ("n", "cancel")]), foot);
        }
        Popup::Input { title, input, purpose } => {
            let rect = centered(area, 64, 5);
            f.render_widget(Clear, rect);
            let title = match purpose {
                InputPurpose::Stash { untracked } => {
                    format!("Stash changes · new files {}", if *untracked { "included" } else { "left in place" })
                }
                _ => title.clone(),
            };
            let block = popup_block(t, s, title, false);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let [field, _, foot] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
            let field = field.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });
            f.render_widget(Line::raw(input.text.clone()), field);
            f.set_cursor_position(Position { x: (field.x + input.cursor_x()).min(field.right().saturating_sub(1)), y: field.y });
            if matches!(purpose, InputPurpose::Stash { .. }) {
                if input.text.is_empty() {
                    f.render_widget(Line::styled("optional message", t.dim_style()), field);
                }
                f.render_widget(footer(t, &[(" enter", "stash"), ("tab", "toggle new files"), ("esc", "cancel")]), foot);
            } else {
                f.render_widget(footer(t, &[(" enter", "ok"), ("esc", "cancel")]), foot);
            }
        }
        Popup::Commit(form) => {
            let branch = app.status.branch.head.clone().unwrap_or_else(|| "HEAD".into());
            let staged = app.status.entries.iter().filter(|e| e.is_staged()).count();
            let title = if form.amend {
                "Amend last commit".to_string()
            } else {
                format!("Commit {staged} file{} to {}", if staged == 1 { "" } else { "s" }, clean(&branch))
            };
            let rect = centered(area, 78, 17);
            f.render_widget(Clear, rect);
            let block = popup_block(t, s, title, false);
            let inner = block.inner(rect).inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });
            f.render_widget(block, rect);
            let [l1, sum, _, l2, body, err, foot] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(2),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(inner);
            let label = |text: &str, active: bool| {
                Span::styled(text.to_string(), if active { Style::new().fg(t.accent).add_modifier(Modifier::BOLD) } else { t.dim_style() })
            };
            let n = form.summary.text.chars().count();
            f.render_widget(
                Line::from(vec![
                    label("Summary", form.field == 0),
                    Span::styled(format!("  {n}/72"), Style::new().fg(if n > 72 { t.warn } else { t.dim })),
                ]),
                l1,
            );
            f.render_widget(Line::raw(form.summary.text.clone()), sum);
            f.render_widget(Line::from(label("Description (optional)", form.field == 1)), l2);
            let bh = body.height as usize;
            let top = form.body.row.saturating_sub(bh.saturating_sub(1));
            let body_lines: Vec<Line> = form.body.lines.iter().skip(top).take(bh).map(|l| Line::raw(l.clone())).collect();
            f.render_widget(Paragraph::new(body_lines), body);
            if let Some(e) = &form.error {
                f.render_widget(Line::styled(e.clone(), Style::new().fg(t.del)), err);
            }
            f.render_widget(footer(t, &[("enter", "commit"), ("tab", "description"), ("ctrl+s", "commit"), ("esc", "cancel")]), foot);
            let pos = if form.field == 0 {
                Position { x: (sum.x + form.summary.cursor_x()).min(sum.right().saturating_sub(1)), y: sum.y }
            } else {
                Position {
                    x: (body.x + form.body.cursor_x()).min(body.right().saturating_sub(1)),
                    y: body.y + (form.body.row - top) as u16,
                }
            };
            f.set_cursor_position(pos);
        }
    }
}
