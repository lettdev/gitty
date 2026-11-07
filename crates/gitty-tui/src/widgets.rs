//! Minimal text inputs for popups.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

fn is_text_input(key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    // AltGr on Windows arrives as ctrl+alt and produces real characters
    !(ctrl || alt) || (ctrl && alt)
}

fn byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(s.len())
}

#[derive(Clone, Debug, Default)]
pub struct LineInput {
    pub text: String,
    /// Cursor position in characters.
    pub cursor: usize,
}

impl LineInput {
    pub fn with(text: &str) -> Self {
        Self { text: text.to_string(), cursor: text.chars().count() }
    }

    pub fn insert_str(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        let i = byte_index(&self.text, self.cursor);
        self.text.insert_str(i, &s);
        self.cursor += s.chars().count();
    }

    /// Display width of the text before the cursor.
    pub fn cursor_x(&self) -> u16 {
        let i = byte_index(&self.text, self.cursor);
        self.text[..i].width() as u16
    }

    pub fn handle(&mut self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.text.chars().count();
        match key.code {
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = len,
            KeyCode::Char('u') if ctrl => {
                let i = byte_index(&self.text, self.cursor);
                self.text.drain(..i);
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let chars: Vec<char> = self.text.chars().collect();
                let mut start = self.cursor;
                while start > 0 && chars[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && chars[start - 1] != ' ' {
                    start -= 1;
                }
                let (a, b) = (byte_index(&self.text, start), byte_index(&self.text, self.cursor));
                self.text.drain(a..b);
                self.cursor = start;
            }
            KeyCode::Char(c) if is_text_input(key) => {
                let i = byte_index(&self.text, self.cursor);
                self.text.insert(i, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let i = byte_index(&self.text, self.cursor - 1);
                self.text.remove(i);
                self.cursor -= 1;
            }
            KeyCode::Delete if self.cursor < len => {
                let i = byte_index(&self.text, self.cursor);
                self.text.remove(i);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct TextArea {
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
}

impl Default for TextArea {
    fn default() -> Self {
        Self { lines: vec![String::new()], row: 0, col: 0 }
    }
}

impl TextArea {
    pub fn with(text: &str) -> Self {
        let lines: Vec<String> = if text.is_empty() { vec![String::new()] } else { text.split('\n').map(str::to_string).collect() };
        let row = lines.len() - 1;
        let col = lines[row].chars().count();
        Self { lines, row, col }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn cursor_x(&self) -> u16 {
        let l = &self.lines[self.row];
        l[..byte_index(l, self.col)].width() as u16
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                self.newline();
            } else if !c.is_control() {
                self.insert(c);
            }
        }
    }

    fn insert(&mut self, c: char) {
        let l = &mut self.lines[self.row];
        let i = byte_index(l, self.col);
        l.insert(i, c);
        self.col += 1;
    }

    fn newline(&mut self) {
        let l = &mut self.lines[self.row];
        let i = byte_index(l, self.col);
        let rest = l.split_off(i);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines[row].chars().count()
    }

    pub fn handle(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => self.newline(),
            KeyCode::Char(c) if is_text_input(key) => self.insert(c),
            KeyCode::Backspace => {
                if self.col > 0 {
                    let l = &mut self.lines[self.row];
                    let i = byte_index(l, self.col - 1);
                    l.remove(i);
                    self.col -= 1;
                } else if self.row > 0 {
                    let cur = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.line_len(self.row);
                    self.lines[self.row].push_str(&cur);
                }
            }
            KeyCode::Delete => {
                if self.col < self.line_len(self.row) {
                    let l = &mut self.lines[self.row];
                    let i = byte_index(l, self.col);
                    l.remove(i);
                } else if self.row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.row + 1);
                    self.lines[self.row].push_str(&next);
                }
            }
            KeyCode::Left => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.line_len(self.row);
                }
            }
            KeyCode::Right => {
                if self.col < self.line_len(self.row) {
                    self.col += 1;
                } else if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }
            KeyCode::Up if self.row > 0 => {
                self.row -= 1;
                self.col = self.col.min(self.line_len(self.row));
            }
            KeyCode::Down if self.row + 1 < self.lines.len() => {
                self.row += 1;
                self.col = self.col.min(self.line_len(self.row));
            }
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = self.line_len(self.row),
            KeyCode::Up | KeyCode::Down => {}
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn line_input_unicode() {
        let mut i = LineInput::with("Sửa");
        i.handle(&k(KeyCode::Left));
        i.handle(&k(KeyCode::Backspace));
        assert_eq!(i.text, "Sa");
        i.handle(&k(KeyCode::Char('ử')));
        assert_eq!(i.text, "Sửa");
        assert_eq!(i.cursor_x(), 2);
    }

    #[test]
    fn textarea_newline_and_join() {
        let mut t = TextArea::with("ab");
        t.handle(&k(KeyCode::Left));
        t.handle(&k(KeyCode::Enter));
        assert_eq!(t.text(), "a\nb");
        t.handle(&k(KeyCode::Backspace));
        assert_eq!(t.text(), "ab");
    }
}
