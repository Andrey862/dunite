//! Движения курсора (SPEC §8.3).
//!
//! Все смещения — char-индексы. Вертикальные движения помнят «запомненную»
//! колонку (`preferred_col`), чтобы не дрожать на коротких строках.

use unicode_segmentation::UnicodeSegmentation;

use crate::buffer::Buffer;

/// Направление движения курсора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Up,
    Down,
    PageUp { lines: usize },
    PageDown { lines: usize },
    DocStart,
    DocEnd,
}

/// Цель движения: новое смещение и (для вертикали) обновлённая preferred_col.
pub fn target(
    buffer: &Buffer,
    offset: usize,
    movement: Movement,
    preferred_col: Option<usize>,
) -> (usize, Option<usize>) {
    let line = buffer.line_of(offset);
    let content = buffer.line_content_range(line);
    let col = offset.clamp(content.start, content.end) - content.start;
    let line_len = content.end - content.start;
    let line_text = buffer.line(line);

    match movement {
        Movement::Left => {
            if col > 0 {
                (content.start + prev_grapheme_col(&line_text, col), None)
            } else if line > 0 {
                (buffer.line_content_range(line - 1).end, None)
            } else {
                (0, None)
            }
        }
        Movement::Right => {
            if col < line_len {
                (content.start + next_grapheme_col(&line_text, col), None)
            } else if line + 1 < buffer.len_lines() {
                (buffer.line_content_range(line + 1).start, None)
            } else {
                (buffer.len_chars(), None)
            }
        }
        Movement::WordLeft => (content.start + word_left(&line_text, col), None),
        Movement::WordRight => (content.start + word_right(&line_text, col), None),
        Movement::LineStart => (content.start, None),
        Movement::LineEnd => (content.end, None),
        Movement::DocStart => (0, None),
        Movement::DocEnd => (buffer.len_chars(), None),
        Movement::Up => vertical(buffer, line, col, preferred_col, -1),
        Movement::Down => vertical(buffer, line, col, preferred_col, 1),
        Movement::PageUp { lines } => vertical(buffer, line, col, preferred_col, -(lines as i64)),
        Movement::PageDown { lines } => vertical(buffer, line, col, preferred_col, lines as i64),
    }
}

fn vertical(
    buffer: &Buffer,
    line: usize,
    col: usize,
    preferred_col: Option<usize>,
    delta: i64,
) -> (usize, Option<usize>) {
    let preferred = preferred_col.unwrap_or(col);
    let max_line = buffer.len_lines().saturating_sub(1) as i64;
    let new_line = (line as i64 + delta).clamp(0, max_line) as usize;
    let content = buffer.line_content_range(new_line);
    let line_len = content.end - content.start;
    (content.start + preferred.min(line_len), Some(preferred))
}

/// Char-смещение начала предыдущего графем-кластера в строке.
fn prev_grapheme_col(line_text: &str, col: usize) -> usize {
    let prefix: String = line_text.chars().take(col).collect();
    prefix
        .graphemes(true)
        .last()
        .map(|g| col - g.chars().count())
        .unwrap_or(0)
}

/// Char-смещение начала следующего графем-кластера в строке.
fn next_grapheme_col(line_text: &str, col: usize) -> usize {
    let suffix: String = line_text.chars().skip(col).collect();
    suffix
        .graphemes(true)
        .next()
        .map(|g| col + g.chars().count())
        .unwrap_or(col)
}

/// Сегменты строки по границам слов (char-смещения внутри строки).
fn segments(line_text: &str) -> Vec<(usize, usize, String)> {
    line_text
        .split_word_bound_indices()
        .map(|(byte_ix, seg)| {
            let start = line_text[..byte_ix].chars().count();
            let end = start + seg.chars().count();
            (start, end, seg.to_string())
        })
        .collect()
}

/// Переход к началу предыдущего слова (Ctrl+Left).
///
/// «Слово» здесь — непробельный пробег (`hello`, `world,`); поведение
/// предсказуемо и совпадает с ощущениями от большинства редакторов.
fn word_left(line_text: &str, col: usize) -> usize {
    let chars: Vec<char> = line_text.chars().collect();
    let mut i = col.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

/// Переход к началу следующего слова (Ctrl+Right): через текущее слово и пробелы.
fn word_right(line_text: &str, col: usize) -> usize {
    let chars: Vec<char> = line_text.chars().collect();
    let mut i = col.min(chars.len());
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

/// Границы «слова» по мелким сегментам (для двойного клика):
/// `hello`, ` `, `world`, `,` — отдельные сегменты.
pub fn word_range_at(line_text: &str, col: usize) -> (usize, usize) {
    let segments = segments(line_text);
    let text_len = line_text.chars().count();
    for (start, end, _) in &segments {
        if *start <= col && col < *end {
            return (*start, *end);
        }
    }
    // за концом текста — последний сегмент
    match segments.last() {
        Some((start, _, _)) if text_len == col && *start < col => (*start, text_len),
        _ => (col.min(text_len), col.min(text_len)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> Buffer {
        Buffer::from_text(text)
    }

    #[test]
    fn left_right_across_lines() {
        let buffer = buf("ab\ncd");
        assert_eq!(target(&buffer, 0, Movement::Left, None).0, 0);
        assert_eq!(target(&buffer, 1, Movement::Left, None).0, 0);
        assert_eq!(target(&buffer, 3, Movement::Left, None).0, 2); // с конца первой строки — на content end
        assert_eq!(target(&buffer, 2, Movement::Right, None).0, 3); // через newline
        assert_eq!(target(&buffer, 5, Movement::Right, None).0, 5);
    }

    #[test]
    fn grapheme_movement() {
        // "я" + combining acute = один кластер
        let buffer = buf("а́б");
        assert_eq!(target(&buffer, 2, Movement::Right, None).0, 3); // 2 -> 3? я́ = 2 chars
    }

    #[test]
    fn word_movement() {
        let buffer = buf("hello world, тест");
        assert_eq!(target(&buffer, 0, Movement::WordRight, None).0, 6);
        assert_eq!(target(&buffer, 6, Movement::WordRight, None).0, 13);
        assert_eq!(target(&buffer, 13, Movement::WordLeft, None).0, 6);
        assert_eq!(target(&buffer, 6, Movement::WordLeft, None).0, 0);
    }

    #[test]
    fn word_movement_inside_word() {
        let buffer = buf("hello world");
        assert_eq!(target(&buffer, 3, Movement::WordLeft, None).0, 0);
        assert_eq!(target(&buffer, 3, Movement::WordRight, None).0, 6);
    }

    #[test]
    fn word_range_for_double_click() {
        assert_eq!(word_range_at("hello world", 0), (0, 5));
        assert_eq!(word_range_at("hello world", 4), (0, 5));
        assert_eq!(word_range_at("hello world", 5), (5, 6)); // пробел
        assert_eq!(word_range_at("hello world", 7), (6, 11));
        assert_eq!(word_range_at("hello world", 11), (6, 11)); // в конце — последнее слово
    }

    #[test]
    fn up_down_preserve_preferred_col() {
        let buffer = buf("12345\nab\n12345");
        // вниз из колонки 4 на короткую строку
        let (offset, pref) = target(&buffer, 4, Movement::Down, None);
        assert_eq!(offset, 8); // строка "ab": начало 6 + col 2
        assert_eq!(pref, Some(4));
        // вниз обратно — возвращаемся в колонку 4
        let (offset, pref) = target(&buffer, offset, Movement::Down, pref);
        assert_eq!(offset, 13);
        assert_eq!(pref, Some(4));
    }

    #[test]
    fn page_and_doc() {
        let buffer = buf("a\nb\nc\nd");
        // с колонки 0 строки "c" — на две строки вверх, в колонку 0
        assert_eq!(target(&buffer, 4, Movement::PageUp { lines: 2 }, None).0, 0);
        assert_eq!(target(&buffer, 0, Movement::DocEnd, None).0, 7);
        assert_eq!(target(&buffer, 5, Movement::DocStart, None).0, 0);
    }

    #[test]
    fn line_start_end() {
        let buffer = buf("ab\ncd");
        assert_eq!(target(&buffer, 4, Movement::LineStart, None).0, 3);
        assert_eq!(target(&buffer, 3, Movement::LineEnd, None).0, 5);
    }
}
