//! Сканер инлайн-декораций (SPEC §8.2).
//!
//! Плоский разбор поверх char-диапазона блока: первое совпадение «съедает»
//! свой диапазон, вложенность не разбирается (для декораций достаточно).

use std::ops::Range;

use crate::types::{Block, BlockKind, Inline};

/// Дополняет блок инлайн-декорациями (кроме code-блоков, frontmatter и hr).
pub(super) fn decorate_block(chars: &[char], mut block: Block) -> Block {
    let scan_start = match &block.kind {
        BlockKind::Frontmatter | BlockKind::CodeBlock { .. } | BlockKind::Rule => return block,
        BlockKind::Heading { marker, .. } => marker.end,
        BlockKind::ListItem { marker, .. } => marker.end,
        BlockKind::Paragraph | BlockKind::Quote { .. } => block.range.start,
    };
    block.inlines = scan_inlines(chars, scan_start..block.range.end);
    block
}

/// Сканирует инлайны в диапазоне символов.
fn scan_inlines(chars: &[char], range: Range<usize>) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut i = range.start;

    while i < range.end {
        // экранированный символ — никогда не начинает декорацию
        if chars[i] == '\\' && i + 1 < range.end {
            i += 2;
            continue;
        }

        // `код`
        if chars[i] == '`' {
            let n = run_len(chars, i, range.end, '`');
            if let Some(close) = find_backtick_close(chars, i + n, range.end, n) {
                out.push(Inline::CodeSpan {
                    range: i..close + n,
                    markers: vec![i..i + n, close..close + n],
                });
                i = close + n;
                continue;
            }
            i += n;
            continue;
        }

        // ![[embed]] и [[ссылка]]
        if starts_with(chars, i, range.end, "![[") {
            if let Some((end, target, display)) = parse_wikilink(chars, i + 3, range.end) {
                out.push(Inline::WikiLink {
                    range: i..end,
                    target,
                    display,
                    embed: true,
                });
                i = end;
                continue;
            }
            i += 3;
            continue;
        }
        if starts_with(chars, i, range.end, "[[") {
            if let Some((end, target, display)) = parse_wikilink(chars, i + 2, range.end) {
                out.push(Inline::WikiLink {
                    range: i..end,
                    target,
                    display,
                    embed: false,
                });
                i = end;
                continue;
            }
            i += 2;
            continue;
        }

        // ~~зачёркнутый~~
        if starts_with(chars, i, range.end, "~~") {
            if let Some(close) = find_literal(chars, i + 2, range.end, "~~") {
                if close > i + 2 {
                    out.push(Inline::Strikethrough {
                        range: i..close + 2,
                        marker_start: i..i + 2,
                        marker_end: close..close + 2,
                    });
                    i = close + 2;
                    continue;
                }
            }
            i += 2;
            continue;
        }

        // **жирный** / __жирный__
        if starts_with(chars, i, range.end, "**") || starts_with(chars, i, range.end, "__") {
            let marker: String = chars[i..i + 2].iter().collect();
            if let Some(close) = find_literal(chars, i + 2, range.end, &marker) {
                if close > i + 2 {
                    out.push(Inline::Emphasis {
                        range: i..close + 2,
                        marker_start: i..i + 2,
                        marker_end: close..close + 2,
                        strong: true,
                    });
                    i = close + 2;
                    continue;
                }
            }
            i += 2;
            continue;
        }

        // *курсив* / _курсив_
        if chars[i] == '*' || chars[i] == '_' {
            let marker = chars[i];
            let boundary_ok = marker == '*' || i == range.start || is_boundary(chars[i - 1]);
            if boundary_ok {
                if let Some(close) = find_single_marker(chars, i + 1, range.end, marker) {
                    out.push(Inline::Emphasis {
                        range: i..close + 1,
                        marker_start: i..i + 1,
                        marker_end: close..close + 1,
                        strong: false,
                    });
                    i = close + 1;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        // #тег
        if chars[i] == '#' && tag_boundary(chars, i, range.start) {
            if let Some(end) = parse_tag(chars, i + 1, range.end) {
                out.push(Inline::Tag { range: i..end });
                i = end;
                continue;
            }
        }

        // [надпись](адрес) и ![alt](картинка)
        if chars[i] == '[' || (chars[i] == '!' && i + 1 < range.end && chars[i + 1] == '[') {
            let image = chars[i] == '!';
            let open = if image { i + 1 } else { i };
            if let Some((end, target, display)) = parse_md_link(chars, open, range.end) {
                out.push(Inline::MarkdownLink {
                    range: i..end,
                    target,
                    display,
                    image,
                });
                i = end;
                continue;
            }
            i += 1;
            continue;
        }

        // ^id блока
        if chars[i] == '^' && (i == range.start || chars[i - 1].is_whitespace()) {
            if let Some(end) = parse_block_id(chars, i + 1, range.end) {
                out.push(Inline::BlockId { range: i..end });
                i = end;
                continue;
            }
        }

        i += 1;
    }

    out
}

// ── хелперы ──────────────────────────────────────────────────────────────

fn starts_with(chars: &[char], i: usize, end: usize, s: &str) -> bool {
    let expected: Vec<char> = s.chars().collect();
    i + expected.len() <= end && chars[i..i + expected.len()] == expected[..]
}

/// Длина репита одного символа с позиции `i`.
fn run_len(chars: &[char], i: usize, end: usize, ch: char) -> usize {
    chars[i..end].iter().take_while(|c| **c == ch).count()
}

/// Закрывающая ровно `n` обратных кавычек (не часть более длинного репита).
fn find_backtick_close(chars: &[char], from: usize, end: usize, n: usize) -> Option<usize> {
    let mut i = from;
    while i < end {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == '`' {
            let run = run_len(chars, i, end, '`');
            if run == n {
                return Some(i);
            }
            i += run;
            continue;
        }
        i += 1;
    }
    None
}

/// Поиск литерала с учётом экранирования.
fn find_literal(chars: &[char], from: usize, end: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    let mut i = from;
    while i + needle.len() <= end {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i..i + needle.len()] == needle[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Закрывающий одиночный маркер (`*`/`_`), не `**`.
fn find_single_marker(chars: &[char], from: usize, end: usize, marker: char) -> Option<usize> {
    let mut i = from;
    while i < end {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == marker {
            let is_double = i + 1 < end && chars[i + 1] == marker;
            if !is_double && i > from {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Разбор `target|display]]`; возвращает (конец, target, display).
fn parse_wikilink(
    chars: &[char],
    content_start: usize,
    end: usize,
) -> Option<(usize, Range<usize>, Option<Range<usize>>)> {
    let close = find_literal(chars, content_start, end, "]]")?;
    if close == content_start {
        return None;
    }
    let pipe = (content_start..close).find(|i| chars[*i] == '|');
    let (target, display) = match pipe {
        Some(pipe) => (content_start..pipe, Some(pipe + 1..close)),
        None => (content_start..close, None),
    };
    if target.is_empty() {
        return None;
    }
    Some((close + 2, target, display))
}

/// Разбор `[display](target)`; `open` — позиция `[`.
fn parse_md_link(
    chars: &[char],
    open: usize,
    end: usize,
) -> Option<(usize, Range<usize>, Option<Range<usize>>)> {
    let mut i = open + 1;
    while i + 1 < end {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == ']' && chars[i + 1] == '(' {
            let display = open + 1..i;
            let target_start = i + 2;
            let mut j = target_start;
            while j < end {
                if chars[j] == '\\' {
                    j += 2;
                    continue;
                }
                if chars[j] == ')' {
                    let target = target_start..j;
                    if target.is_empty() {
                        return None;
                    }
                    let display = if display.is_empty() {
                        None
                    } else {
                        Some(display)
                    };
                    return Some((j + 1, target, display));
                }
                j += 1;
            }
            return None;
        }
        i += 1;
    }
    None
}

/// `#` начинает тег только на границе слова.
fn tag_boundary(chars: &[char], i: usize, start: usize) -> bool {
    if i == start {
        return true;
    }
    let prev = chars[i - 1];
    prev.is_whitespace() || matches!(prev, '(' | '[' | '{' | '>' | '"' | '\'' | '`')
}

/// Конец тега: `[alnum _ \- / + ;]+`, минимум одна буква/цифра, не только цифры.
fn parse_tag(chars: &[char], from: usize, end: usize) -> Option<usize> {
    let mut i = from;
    let mut has_alnum = false;
    let mut all_digits = true;
    while i < end {
        let c = chars[i];
        if c.is_alphanumeric() {
            has_alnum = true;
            all_digits = all_digits && c.is_ascii_digit();
        } else if c == '_' || c == '-' || c == '/' || c == '+' || c == ';' {
            // допустимые символы тега
        } else {
            break;
        }
        i += 1;
    }
    if has_alnum && !all_digits && i > from {
        Some(i)
    } else {
        None
    }
}

/// `^id` в конце строки/блока.
fn parse_block_id(chars: &[char], from: usize, end: usize) -> Option<usize> {
    let mut i = from;
    while i < end {
        let c = chars[i];
        if c.is_alphanumeric() || c == '-' || c == '_' {
            i += 1;
        } else {
            break;
        }
    }
    if i == from {
        return None;
    }
    // id уместен только в конце строки/блока
    if i >= end || chars[i].is_whitespace() {
        Some(i)
    } else {
        None
    }
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | '[' | '{' | '>' | '"' | '\'' | '`' | '=' | ':')
}
