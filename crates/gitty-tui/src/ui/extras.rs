//! Drawing for ignore rules and the file-type breakdown.

use gitty_core::insight::{FileType, TypeStat, human_size};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::{centered, footer, pane, popup_block, row_rect, scroll_to};
use crate::app::{App, IgnoreMenu, IgnoreRulesView, RulesInput};
use crate::theme::{SymbolSet, Theme};
use crate::util::clean;

fn type_color(t: &Theme, k: Option<FileType>) -> Color {
    match k {
        Some(FileType::Code) => t.info,
        Some(FileType::Images) => t.warn,
        Some(FileType::Audio) => t.header,
        Some(FileType::Video) => t.del,
        Some(FileType::Models) => t.lanes[4],
        Some(FileType::EngineAssets) => t.accent,
        Some(FileType::Fonts) => t.modified,
        Some(FileType::Docs) => t.lanes[3],
        Some(FileType::Binaries) => t.lanes[5],
        Some(FileType::Other) | None => t.dim,
    }
}

fn type_letter(k: Option<FileType>) -> char {
    match k {
        Some(FileType::Code) => 'C',
        Some(FileType::Images) => 'I',
        Some(FileType::Audio) => 'A',
        Some(FileType::Video) => 'V',
        Some(FileType::Models) => 'M',
        Some(FileType::EngineAssets) => 'S',
        Some(FileType::Fonts) => 'F',
        Some(FileType::Docs) => 'D',
        Some(FileType::Binaries) => 'B',
        Some(FileType::Other) | None => '.',
    }
}

/// Rows the file-type pane wants (including borders).
pub(super) fn types_height(types: &[TypeStat]) -> u16 {
    (types.len().min(7) as u16) + 4
}

pub(super) fn draw_types(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let block = pane(t, &app.sym, "Composition".into(), false);
    let inner = block.inner(area).inner(Margin { horizontal: 1, vertical: 0 });
    f.render_widget(block, area);
    let Some(ins) = &app.insight else { return };
    let total: u64 = ins.types.iter().map(|x| x.bytes).sum::<u64>().max(1);
    let ascii = app.sym.set == SymbolSet::Ascii;

    // stacked bar: every type gets at least one cell if it is visible at all
    let w = inner.width as usize;
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, ty) in ins.types.iter().enumerate() {
        let remaining = w.saturating_sub(used);
        let mut cells = ((ty.bytes as f64 / total as f64) * w as f64).round() as usize;
        if i == ins.types.len() - 1 {
            cells = remaining;
        }
        let cells = cells.min(remaining);
        if cells == 0 {
            continue;
        }
        used += cells;
        let ch = if ascii { type_letter(ty.kind).to_string() } else { "█".to_string() };
        spans.push(Span::styled(ch.repeat(cells), Style::new().fg(type_color(t, ty.kind))));
    }
    let [bar, _, list] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    f.render_widget(Line::from(spans), bar);

    let swatch = if ascii { None } else { Some("■ ") };
    for (i, ty) in ins.types.iter().take(list.height as usize).enumerate() {
        let rect = row_rect(list, i);
        let pct = ty.bytes * 100 / total;
        let name = ty.kind.map(FileType::name).unwrap_or("Other");
        let mark = match swatch {
            Some(s) => s.to_string(),
            None => format!("{} ", type_letter(ty.kind)),
        };
        let exts: Vec<String> = ty.top.iter().map(|(e, _)| format!(".{e}")).collect();
        let [main, meta] = Layout::horizontal([Constraint::Min(0), Constraint::Length(20)]).areas(rect);
        f.render_widget(
            Line::from(vec![
                Span::styled(mark, Style::new().fg(type_color(t, ty.kind))),
                Span::raw(name.to_string()),
                Span::styled(format!("  {}", exts.join(" ")), t.dim_style()),
            ]),
            main,
        );
        f.render_widget(
            Line::from(vec![
                Span::raw(format!("{:>8}", human_size(ty.bytes))),
                Span::styled(if pct == 0 && ty.bytes > 0 { "  <1%".to_string() } else { format!(" {pct:>3}%") }, t.dim_style()),
                Span::styled(format!(" {:>6}", ty.files), t.dim_style()),
            ]),
            meta,
        );
    }
}

pub(super) fn draw_ignore_menu(f: &mut Frame, app: &App, m: &IgnoreMenu, area: Rect) {
    let t = &app.theme;
    let h = m.options.len() as u16 + 7;
    let rect = centered(area, 70, h);
    f.render_widget(Clear, rect);
    let block = popup_block(t, &app.sym, format!("Ignore {}", clean(&m.path)), false);
    let inner = block.inner(rect).inner(Margin { horizontal: 1, vertical: 1 });
    f.render_widget(block, rect);
    let [list, note, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(2), Constraint::Length(1)]).areas(inner);
    for (i, o) in m.options.iter().enumerate() {
        let r = row_rect(list, i);
        if i == m.sel {
            f.buffer_mut().set_style(r, t.selection(true));
        }
        f.render_widget(
            Line::from(vec![
                Span::styled(format!(" {} ", i + 1), Style::new().fg(t.info).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<28}", clean(&o.pattern)), Style::new().add_modifier(Modifier::BOLD)),
                Span::styled(o.label.clone(), t.dim_style()),
            ]),
            r,
        );
    }
    let text = if m.tracked {
        "The file is tracked: it will also be removed from git (it stays on disk). u undoes it."
    } else {
        "Adds the rule to the root .gitignore. u undoes it."
    };
    f.render_widget(Paragraph::new(text).style(t.dim_style()).wrap(Wrap { trim: true }), note);
    f.render_widget(footer(t, &[("enter/1-9", "add rule"), ("I", "all rules"), ("esc", "cancel")]), foot);
}

pub(super) fn draw_ignore_rules(f: &mut Frame, app: &App, v: &IgnoreRulesView, area: Rect) {
    let t = &app.theme;
    let rect = centered(area, 96, area.height.saturating_sub(2));
    f.render_widget(Clear, rect);
    let block = popup_block(t, &app.sym, "Ignore rules".into(), false);
    let inner = block.inner(rect).inner(Margin { horizontal: 1, vertical: 0 });
    f.render_widget(block, rect);
    let [list, result, input, foot] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(2), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    let h = list.height as usize;
    let off = scroll_to(v.sel, 0, h, v.rows.len());
    for (i, (si, ri)) in v.rows.iter().enumerate().skip(off).take(h) {
        let r = row_rect(list, i - off);
        if i == v.sel {
            f.buffer_mut().set_style(r, t.selection(true));
        }
        let src = &v.sources[*si];
        let line = match ri {
            None => {
                let mut spans = vec![Span::styled(clean(&src.label), Style::new().fg(t.header).add_modifier(Modifier::BOLD))];
                if !src.exists {
                    spans.push(Span::styled("  not created yet", t.dim_style()));
                } else {
                    let n = src.rules.iter().filter(|r| !r.comment).count();
                    spans.push(Span::styled(format!("  {n} rule{}", if n == 1 { "" } else { "s" }), t.dim_style()));
                }
                Line::from(spans)
            }
            Some(ri) => {
                let rule = &src.rules[*ri];
                let style = if rule.comment {
                    t.dim_style()
                } else if rule.text.trim_start().starts_with('!') {
                    Style::new().fg(t.add)
                } else {
                    Style::new()
                };
                Line::from(vec![Span::styled(format!("  {:>4}  ", rule.line), t.dim_style()), Span::styled(clean(&rule.text), style)])
            }
        };
        f.render_widget(line, r);
    }
    if let Some((text, ignored)) = &v.result {
        let color = if *ignored { t.warn } else { t.accent };
        f.render_widget(Paragraph::new(clean(text)).style(Style::new().fg(color)).wrap(Wrap { trim: true }), result);
    }
    match &v.input {
        Some((mode, li)) => {
            let label = match mode {
                RulesInput::Add => {
                    let target = v.current_source().and_then(|s| s.rel.clone()).unwrap_or_else(|| ".gitignore".into());
                    format!("add to {target}: ")
                }
                RulesInput::Check => "why is this path ignored? ".to_string(),
            };
            let lw = label.chars().count() as u16;
            f.render_widget(
                Line::from(vec![Span::styled(label, Style::new().fg(t.info)), Span::raw(li.text.clone())]),
                input,
            );
            f.set_cursor_position(Position { x: (input.x + lw + li.cursor_x()).min(input.right().saturating_sub(1)), y: input.y });
            f.render_widget(footer(t, &[("enter", "ok"), ("esc", "cancel")]), foot);
        }
        None => {
            f.render_widget(
                footer(t, &[("a", "add rule"), ("d", "delete rule"), ("e", "edit file"), ("/", "why ignored?"), ("esc", "close")]),
                foot,
            );
        }
    }
}
