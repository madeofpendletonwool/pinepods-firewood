//! Text helpers that measure in terminal columns, not bytes.

use unicode_width::UnicodeWidthStr;

/// Shortens `text` to at most `max_width` columns, ending in `…` when cut.
/// Slicing a `String` by byte index panics on multi-byte characters, which podcast
/// titles are full of (em-dashes, accents, emoji).
pub fn truncate(text: &str, max_width: usize) -> String {
    if text.width() <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    // Measure the growing prefix rather than summing per-char widths: a variation
    // selector can widen the character before it (🎙 is 1 column, 🎙️ is 2).
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        if out.width() > max_width - 1 {
            out.pop();
            break;
        }
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_short_text_alone() {
        assert_eq!(truncate("Hello", 10), "Hello");
    }

    #[test]
    fn cuts_on_character_boundaries() {
        // An em-dash at byte 4–6 used to panic with `&title[..5]`.
        assert_eq!(truncate("Ep 1 — The Start", 6), "Ep 1 …");
        assert_eq!(truncate("Café society", 5), "Café…");
    }

    #[test]
    fn counts_wide_characters_as_two_columns() {
        let out = truncate("🎙️🎙️🎙️ podcast", 5);
        assert!(out.width() <= 5, "{out:?} is {} wide", out.width());
        assert!(out.ends_with('…'));
    }

    #[test]
    fn zero_width_gives_empty() {
        assert_eq!(truncate("anything", 0), "");
    }
}
