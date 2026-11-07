use std::io::Write;

use unicode_normalization::UnicodeNormalization;

/// Make repository text safe to draw: expand tabs, drop control characters
/// (which could inject terminal escapes) and normalize to NFC so decomposed
/// Vietnamese filenames from macOS line up correctly.
pub fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfc() {
        match c {
            '\t' => out.push_str("    "),
            '\r' | '\n' => {}
            c if c.is_control() => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

pub fn relative_time(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(ts);
    let d = (now - ts).max(0);
    match d {
        0..60 => "now".into(),
        60..3600 => format!("{}m", d / 60),
        3600..86_400 => format!("{}h", d / 3600),
        86_400..604_800 => format!("{}d", d / 86_400),
        604_800..2_592_000 => format!("{}w", d / 604_800),
        2_592_000..31_536_000 => format!("{}mo", d / 2_592_000),
        _ => format!("{}y", d / 31_536_000),
    }
}

/// Split a command line like a POSIX shell would (quotes and backslashes).
pub fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = s.chars();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    in_word = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Copy to the system clipboard through the terminal (OSC 52). Works in most
/// modern terminals, including over SSH.
pub fn copy_to_clipboard(text: &str) {
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split() {
        assert_eq!(shell_split(r#"commit -m "two words" --x='a b' c\ d"#), vec!["commit", "-m", "two words", "--x=a b", "c d"]);
        assert!(shell_split("   ").is_empty());
    }

    #[test]
    fn b64() {
        assert_eq!(base64(b"hello"), "aGVsbG8=");
        assert_eq!(base64(b"hi!"), "aGkh");
        assert_eq!(base64(b"a"), "YQ==");
    }

    #[test]
    fn nfc_and_controls() {
        let decomposed = "Thie\u{0302}\u{0301}t";
        assert_eq!(clean(decomposed), "Thiết");
        assert_eq!(clean("a\x1b[31mb"), "a\u{fffd}[31mb");
    }
}
