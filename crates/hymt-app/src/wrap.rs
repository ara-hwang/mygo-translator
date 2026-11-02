//! Soft wrapping for the text editor, kept free of GPUI types so it can be unit tested.
//!
//! GPUI shapes text for painting, but a caret needs to know which *visual* line a byte offset
//! belongs to. Instead of reverse engineering the shaped layout, the editor asks this module
//! for visual line ranges using a measurement callback (text width for a byte offset). The
//! editor supplies real measurements from `ShapedLine::x_for_index`; tests supply a fake
//! monospace measurement.

use std::ops::Range;

/// A wide-enough width in pixels (used for "do not wrap" cases).
pub const NO_LIMIT: f32 = f32::INFINITY;

/// Splits `text` into visual lines that fit into `max_width`.
///
/// `measure(offset)` must return the horizontal position of the byte offset *relative to the
/// start of `text`*. `base` is added to every returned range so callers can wrap one
/// paragraph of a larger buffer without translating indices.
///
/// Wrapping prefers whitespace: a line is broken at the last space that still fits, and only
/// hard-broken mid-word when a single word is wider than the available width.
pub fn wrap_line<F>(text: &str, base: usize, max_width: f32, measure: F) -> Vec<Range<usize>>
where
    F: Fn(usize) -> f32,
{
    let mut ranges = Vec::new();
    if text.is_empty() {
        ranges.push(base..base);
        return ranges;
    }

    let boundaries: Vec<usize> = char_boundaries(text);
    let mut start = 0usize;

    while start < text.len() {
        // Fast path: the rest fits on one line.
        let remaining_width = measure(text.len()) - measure(start);
        if remaining_width <= max_width || max_width <= 0.0 {
            ranges.push(base + start..base + text.len());
            return ranges;
        }

        // Find the furthest boundary that still fits.
        let start_x = measure(start);
        let mut fit_end = start;
        let mut last_space: Option<usize> = None;
        for &boundary in boundaries.iter().filter(|&&boundary| boundary > start) {
            let width = measure(boundary) - start_x;
            if width > max_width {
                break;
            }
            fit_end = boundary;
            if text[start..boundary].ends_with(char::is_whitespace) {
                last_space = Some(boundary);
            }
        }

        // Never make progress -> a single glyph is wider than the viewport; emit it anyway.
        if fit_end == start {
            let next = boundaries
                .iter()
                .copied()
                .find(|&boundary| boundary > start)
                .unwrap_or(text.len());
            ranges.push(base + start..base + next);
            start = next;
            continue;
        }

        // Prefer breaking after the last space, but keep at least one glyph on the line.
        let mut end = last_space.unwrap_or(fit_end);
        if end == start {
            end = fit_end;
        }
        // Do not leave a run of spaces at the start of the next line.
        while end < text.len() && text[end..].starts_with(' ') {
            end += 1;
        }
        ranges.push(base + start..base + end);
        start = end;
    }

    if ranges.is_empty() {
        ranges.push(base..base);
    }
    ranges
}

/// All char boundaries of `text`, including `0` and `text.len()`.
pub fn char_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    boundaries.push(text.len());
    boundaries
}

/// The visual line that contains `cursor`, preferring the following line at a soft break.
pub fn visual_line_for_cursor(lines: &[Range<usize>], cursor: usize) -> usize {
    for (index, line) in lines.iter().enumerate() {
        if cursor < line.end {
            return index;
        }
        if cursor == line.end {
            if line.is_empty() {
                return index;
            }
            // Soft-wrapped ranges meet at the same byte offset; that caret belongs to the next
            // visual line. An explicit newline leaves a one-byte gap, so its preceding caret
            // remains at the end of this line.
            if lines
                .get(index + 1)
                .is_some_and(|next| next.start == line.end)
            {
                return index + 1;
            }
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

/// Returns the start or end offset of the logical line containing `cursor`.
pub fn logical_line_boundary(text: &str, cursor: usize, end: bool) -> usize {
    let cursor = cursor.min(text.len());
    if end {
        cursor + text[cursor..].find('\n').unwrap_or(text.len() - cursor)
    } else {
        text[..cursor].rfind('\n').map_or(0, |newline| newline + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ASCII byte is 10px wide, every non-ASCII char 20px.
    fn measure(text: &str) -> impl Fn(usize) -> f32 {
        let mut cumulative = vec![(0usize, 0f32)];
        let mut width = 0f32;
        for (index, ch) in text.char_indices() {
            width += if ch.is_ascii() { 10.0 } else { 20.0 };
            cumulative.push((index + ch.len_utf8(), width));
        }
        move |offset: usize| {
            cumulative
                .iter()
                .find(|(index, _)| *index == offset)
                .map(|(_, width)| *width)
                .unwrap_or_default()
        }
    }

    fn wrapped(text: &str, width: f32) -> Vec<String> {
        wrap_line(text, 0, width, measure(text))
            .into_iter()
            .map(|range| text[range].to_string())
            .collect()
    }

    #[test]
    fn short_text_stays_on_one_line() {
        assert_eq!(wrapped("hello", 100.0), vec!["hello"]);
        assert_eq!(wrapped("", 100.0), vec![""]);
    }

    #[test]
    fn breaks_at_spaces() {
        assert_eq!(
            wrapped("hello world again", 60.0),
            vec!["hello ", "world ", "again"]
        );
    }

    #[test]
    fn hard_breaks_a_word_that_cannot_fit() {
        assert_eq!(wrapped("abcdefghij", 30.0), vec!["abc", "def", "ghi", "j"]);
    }

    #[test]
    fn counts_double_width_characters() {
        // 3 Korean characters at 20px each exactly fill 60px.
        assert_eq!(wrapped("가나다라", 60.0), vec!["가나다", "라"]);
    }

    #[test]
    fn keeps_offsets_absolute_with_a_base() {
        let ranges = wrap_line("hello world", 100, 60.0, measure("hello world"));
        assert_eq!(ranges, vec![100..106, 106..111]);
    }

    #[test]
    fn cursor_prefers_the_next_line_at_a_soft_wrap() {
        let lines = vec![0..6, 6..11];
        assert_eq!(visual_line_for_cursor(&lines, 3), 0);
        assert_eq!(visual_line_for_cursor(&lines, 6), 1);
        assert_eq!(visual_line_for_cursor(&lines, 11), 1);
    }

    #[test]
    fn cursor_maps_to_lines_including_empty_ones() {
        let lines = vec![0..0, 0..0];
        assert_eq!(visual_line_for_cursor(&lines, 0), 0);
    }

    #[test]
    fn cursor_before_explicit_newline_stays_on_previous_line() {
        let lines = vec![0..1, 2..3];
        assert_eq!(visual_line_for_cursor(&lines, 1), 0);
        assert_eq!(visual_line_for_cursor(&lines, 2), 1);
    }

    #[test]
    fn home_and_end_offsets_follow_the_current_logical_line() {
        let text = "first\nsecond\nthird";
        assert_eq!(logical_line_boundary(text, 9, false), 6);
        assert_eq!(logical_line_boundary(text, 9, true), 12);
        assert_eq!(logical_line_boundary(text, 6, false), 6);
    }

    #[test]
    fn an_unlimited_width_never_wraps() {
        assert_eq!(wrapped("hello world", NO_LIMIT), vec!["hello world"]);
    }
}
