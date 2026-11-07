//! Colors and symbols, adapted to what the terminal can show.

use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorDepth {
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

pub fn detect_depth(setting: &str) -> ColorDepth {
    match setting {
        "truecolor" | "24bit" => return ColorDepth::TrueColor,
        "256" => return ColorDepth::Ansi256,
        "16" => return ColorDepth::Ansi16,
        "none" => return ColorDepth::None,
        _ => {}
    }
    if env("NO_COLOR").is_some() {
        return ColorDepth::None;
    }
    let colorterm = env("COLORTERM").unwrap_or_default().to_lowercase();
    if colorterm.contains("truecolor") || colorterm.contains("24bit") || env("WT_SESSION").is_some() {
        return ColorDepth::TrueColor;
    }
    if let Some(p) = env("TERM_PROGRAM") {
        if ["iTerm.app", "WezTerm", "vscode", "ghostty", "Hyper", "tabby"].contains(&p.as_str()) {
            return ColorDepth::TrueColor;
        }
    }
    let term = env("TERM").unwrap_or_default();
    if term.contains("256color") || term.contains("kitty") || term.contains("alacritty") {
        return ColorDepth::Ansi256;
    }
    ColorDepth::Ansi16
}

fn to_256((r, g, b): (u8, u8, u8)) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| {
        LEVELS.iter().enumerate().min_by_key(|(_, l)| (i32::from(**l) - i32::from(v)).abs()).map(|(i, _)| i).unwrap_or(0)
    };
    let dist = |a: (u8, u8, u8)| {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = (16 + 36 * ri + 6 * gi + bi) as u8;
    let cube_d = dist((LEVELS[ri], LEVELS[gi], LEVELS[bi]));
    let avg = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    let step = (avg.saturating_sub(8) / 10).min(23);
    let gray = (8 + 10 * step) as u8;
    if dist((gray, gray, gray)) < cube_d { 232 + step as u8 } else { cube }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub depth: ColorDepth,
    pub accent: Color,
    pub dim: Color,
    pub add: Color,
    pub del: Color,
    pub modified: Color,
    pub info: Color,
    pub warn: Color,
    pub border: Color,
    pub header: Color,
    pub hash: Color,
    pub add_bg: Option<Color>,
    pub del_bg: Option<Color>,
    pub hunk_bg: Option<Color>,
    sel_bg: Color,
    sel_dim_bg: Color,
    range_bg: Color,
    pub lanes: [Color; 6],
}

impl Theme {
    pub fn new(depth: ColorDepth) -> Self {
        let c = |rgb: (u8, u8, u8), ansi: Color| match depth {
            ColorDepth::TrueColor => Color::Rgb(rgb.0, rgb.1, rgb.2),
            ColorDepth::Ansi256 => Color::Indexed(to_256(rgb)),
            ColorDepth::Ansi16 => ansi,
            ColorDepth::None => Color::Reset,
        };
        // backgrounds only where they can be subtle enough to keep text readable
        let bg = |rgb: (u8, u8, u8)| match depth {
            ColorDepth::TrueColor => Some(Color::Rgb(rgb.0, rgb.1, rgb.2)),
            ColorDepth::Ansi256 => Some(Color::Indexed(to_256(rgb))),
            _ => None,
        };
        Self {
            depth,
            accent: c((63, 185, 80), Color::Green),
            dim: c((125, 133, 144), Color::DarkGray),
            add: c((63, 185, 80), Color::Green),
            del: c((248, 81, 73), Color::Red),
            modified: c((210, 153, 34), Color::Yellow),
            info: c((88, 166, 255), Color::Blue),
            warn: c((240, 136, 62), Color::Yellow),
            border: c((60, 66, 75), Color::DarkGray),
            header: c((188, 140, 255), Color::Magenta),
            hash: c((210, 153, 34), Color::Yellow),
            add_bg: bg((18, 46, 27)),
            del_bg: bg((60, 20, 24)),
            hunk_bg: bg((22, 34, 52)),
            sel_bg: c((38, 79, 120), Color::Blue),
            sel_dim_bg: c((44, 49, 58), Color::DarkGray),
            range_bg: c((72, 60, 18), Color::DarkGray),
            lanes: [
                c((63, 185, 80), Color::Green),
                c((188, 140, 255), Color::Magenta),
                c((240, 136, 62), Color::Yellow),
                c((88, 166, 255), Color::Blue),
                c((57, 197, 207), Color::Cyan),
                c((248, 81, 73), Color::Red),
            ],
        }
    }

    pub fn dim_style(&self) -> Style {
        if self.depth == ColorDepth::None { Style::new().add_modifier(Modifier::DIM) } else { Style::new().fg(self.dim) }
    }

    pub fn selection(&self, focused: bool) -> Style {
        match (self.depth, focused) {
            (ColorDepth::None, true) => Style::new().add_modifier(Modifier::REVERSED),
            (ColorDepth::None, false) => Style::new().add_modifier(Modifier::UNDERLINED),
            (_, true) => Style::new().bg(self.sel_bg),
            (_, false) => Style::new().bg(self.sel_dim_bg),
        }
    }

    pub fn range(&self) -> Style {
        if self.depth == ColorDepth::None { Style::new().add_modifier(Modifier::BOLD) } else { Style::new().bg(self.range_bg) }
    }

    pub fn lane(&self, i: u16) -> Color {
        self.lanes[i as usize % self.lanes.len()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolSet {
    Unicode,
    Nerd,
    Ascii,
}

#[derive(Clone, Debug)]
pub struct Symbols {
    pub set: SymbolSet,
    pub branch: &'static str,
    pub ahead: &'static str,
    pub behind: &'static str,
    pub synced: &'static str,
    pub head: char,
    pub bar: &'static str,
    pub prompt: &'static str,
    pub arrow: &'static str,
    pub spinner: &'static [&'static str],
    pub border: border::Set<'static>,
}

const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

impl Symbols {
    pub fn detect(setting: &str) -> Self {
        let set = match setting {
            "unicode" => SymbolSet::Unicode,
            "nerd" | "nerd-font" | "nerdfont" => SymbolSet::Nerd,
            "ascii" => SymbolSet::Ascii,
            _ => Self::auto(),
        };
        Self::for_set(set)
    }

    /// The classic Windows console and the Linux VT lack most symbol glyphs.
    fn auto() -> SymbolSet {
        let term = env("TERM").unwrap_or_default();
        let windows_console = cfg!(windows) && env("WT_SESSION").is_none() && env("TERM_PROGRAM").is_none();
        if windows_console || term == "linux" { SymbolSet::Ascii } else { SymbolSet::Unicode }
    }

    pub fn for_set(set: SymbolSet) -> Self {
        match set {
            SymbolSet::Unicode => Self {
                set,
                branch: "⎇",
                ahead: "↑",
                behind: "↓",
                synced: "✓",
                head: '◉',
                bar: "▌",
                prompt: "›",
                arrow: "→",
                spinner: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
                border: border::ROUNDED,
            },
            SymbolSet::Nerd => Self {
                set,
                branch: "\u{e0a0}",
                ahead: "\u{f062}",
                behind: "\u{f063}",
                synced: "\u{f00c}",
                head: '\u{f444}',
                bar: "▌",
                prompt: "\u{f054}",
                arrow: "→",
                spinner: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
                border: border::ROUNDED,
            },
            SymbolSet::Ascii => Self {
                set,
                branch: "*",
                ahead: "^",
                behind: "v",
                synced: "=",
                head: '@',
                bar: "|",
                prompt: ">",
                arrow: "->",
                spinner: &["|", "/", "-", "\\"],
                border: ASCII_BORDER,
            },
        }
    }

    /// Map a graph cell character to this symbol set.
    pub fn graph(&self, ch: char) -> char {
        if self.set != SymbolSet::Ascii {
            return ch;
        }
        match ch {
            '│' | '┤' | '├' => '|',
            '─' => '-',
            '╮' | '╭' => '.',
            '╯' | '╰' => '\'',
            '┼' | '┴' | '┬' => '+',
            '●' => '*',
            c => c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_256() {
        assert_eq!(to_256((0, 0, 0)), 16);
        assert_eq!(to_256((255, 255, 255)), 231);
        assert_eq!(to_256((128, 128, 128)), 244);
    }
}
