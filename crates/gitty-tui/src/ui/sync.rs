//! Drawing for the push preview and the About screen.

use ratatui::Frame;
use ratatui::layout::{Alignment, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::{centered, footer, popup_block};
use crate::app::{App, PushPreview, cat_frame, plural};
use crate::util::{clean, relative_time};

pub(super) fn draw_push(f: &mut Frame, app: &App, p: &PushPreview, area: Rect) {
    let t = &app.theme;
    let branch = clean(&p.branch);
    let mut lines: Vec<Line> = Vec::new();
    let count = p.commits.len() + p.more;
    let title = match &p.upstream {
        Some(up) => {
            lines.push(Line::raw(format!("Send {} from {branch} to {}:", plural(count as u32, "commit"), clean(up))));
            format!("Push to {}", clean(up))
        }
        None => {
            lines.push(Line::raw(format!("{branch} isn't on {} yet. Pushing creates it there,", clean(&p.remote))));
            lines.push(Line::raw(format!("so others can see it. {} will go up:", plural(count as u32, "commit"))));
            format!("Publish {branch}")
        }
    };
    lines.push(Line::raw(""));
    let width = 72usize;
    for c in &p.commits {
        let when = relative_time(c.time);
        let room = width.saturating_sub(12 + when.width() + 2);
        let mut subject = clean(&c.subject);
        if subject.width() > room {
            subject = subject.chars().take(room.saturating_sub(1)).collect::<String>() + "…";
        }
        let pad = room.saturating_sub(subject.width());
        lines.push(Line::from(vec![
            Span::styled(format!("  {}  ", c.short()), Style::new().fg(t.hash)),
            Span::raw(subject),
            Span::raw(" ".repeat(pad + 2)),
            Span::styled(when, t.dim_style()),
        ]));
    }
    if p.commits.is_empty() {
        lines.push(Line::styled("  (no new commits — the branch points at a commit the remote already has)", t.dim_style()));
    }
    if p.more > 0 {
        lines.push(Line::styled(format!("  … and {} more", p.more), t.dim_style()));
    }
    if p.behind > 0 {
        let pull = app.keys.key(crate::keys::Action::Pull);
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("The remote has {} you don't have yet, so git will refuse this push.", plural(p.behind, "commit")),
            Style::new().fg(t.warn),
        ));
        lines.push(Line::styled(format!("Pull first ({pull}), then push."), Style::new().fg(t.warn)));
    }
    let rect = centered(area, width as u16 + 4, lines.len() as u16 + 5);
    f.render_widget(Clear, rect);
    let block = popup_block(t, &app.sym, title, false);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let body = Rect { height: inner.height.saturating_sub(1), ..inner };
    let foot = Rect { y: inner.bottom().saturating_sub(1), height: 1, ..inner };
    f.render_widget(Paragraph::new(lines), body.inner(Margin { horizontal: 1, vertical: 1 }));
    let verb = if p.upstream.is_some() { "push" } else { "publish" };
    f.render_widget(footer(t, &[(" enter", verb), ("esc", "cancel")]), foot);
}

pub(super) fn draw_about(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let cat = cat_frame(app.elapsed_ms());
    let mut lines: Vec<Line> = vec![Line::raw("")];
    for row in &cat {
        lines.push(Line::styled(row.clone(), Style::new().fg(t.accent)));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("gitty ", Style::new().fg(t.header).add_modifier(Modifier::BOLD)),
        Span::styled(concat!("v", env!("CARGO_PKG_VERSION")), t.dim_style()),
    ]));
    lines.push(Line::styled("git + kitty — a friendly git client", t.dim_style()));
    lines.push(Line::raw(""));
    lines.push(Line::styled(env!("CARGO_PKG_REPOSITORY"), Style::new().fg(t.info)));
    if let Some(cfg) = crate::config::path() {
        lines.push(Line::styled(format!("settings: {}", cfg.display()), t.dim_style()));
    }
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 6;
    let rect = centered(area, w.max(44), lines.len() as u16 + 4);
    f.render_widget(Clear, rect);
    let block = popup_block(t, &app.sym, "About".into(), false);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let body = Rect { height: inner.height.saturating_sub(1), ..inner };
    let foot = Rect { y: inner.bottom().saturating_sub(1), height: 1, ..inner };
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), body);
    f.render_widget(footer(t, &[(" esc", "close")]), foot);
}
