//! Сканер блоков markdown (SPEC §8.2).

use std::ops::Range;

use crate::types::{Block, BlockKind, Checkbox};

/// Строка: содержимое без перевода строки и полный диапазон с ним.
pub(super) struct Line {
    pub content: Range<usize>,
    pub full: Range<usize>,
}

/// Разбивает символы на строки.
fn split_lines(chars: &[char]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (ix, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            let mut content_end = ix;
            if content_end > start && chars[content_end - 1] == '\r' {
                content_end -= 1;
            }
            lines.push(Line {
                content: start..content_end,
                full: start..ix + 1,
            });
            start = ix + 1;
        }
    }
    if start < chars.len() || chars.is_empty() {
        lines.push(Line {
            content: start..chars.len(),
            full: start..chars.len(),
        });
    }
    lines
}

fn is_blank(chars: &[char], range: &Range<usize>) -> bool {
    chars[range.clone()].iter().all(|c| c.is_whitespace())
}

fn line_str(chars: &[char], range: &Range<usize>) -> String {
    chars[range.clone()].iter().collect()
}

/// Сканирует блоки: frontmatter, code-блоки, заголовки, цитаты, списки, hr, абзацы.
pub(super) fn scan_blocks(chars: &[char]) -> Vec<Block> {
    let lines = split_lines(chars);
    let mut blocks = Vec::new();
    let mut ix = 0usize;

    // frontmatter — только в самом начале заметки
    if !lines.is_empty() && line_str(chars, &lines[0].content).trim_end() == "---" {
        if let Some(end) = (1..lines.len()).find(|i| {
            let t = line_str(chars, &lines[*i].content);
            t.trim_end() == "---" || t.trim_end() == "..."
        }) {
            blocks.push(Block {
                range: lines[0].full.start..lines[end].full.end,
                kind: BlockKind::Frontmatter,
                inlines: Vec::new(),
            });
            ix = end + 1;
        }
    }

    while ix < lines.len() {
        let line = &lines[ix];
        if is_blank(chars, &line.content) {
            ix += 1;
            continue;
        }

        let text = line_str(chars, &line.content);

        // fenced code-блок
        if let Some((fence_char, fence_len, lang)) = parse_fence(&text) {
            let fence_start = line.content.start
                + text.chars().position(|c| c == fence_char).unwrap_or(0)
                ..line.content.end;
            let mut fence_end = None;
            let mut end_ix = ix;
            for (jx, candidate) in lines.iter().enumerate().skip(ix + 1) {
                let t = line_str(chars, &candidate.content);
                if closes_fence(&t, fence_char, fence_len) {
                    fence_end = Some(
                        candidate.content.start
                            + t.chars().position(|c| c == fence_char).unwrap_or(0)
                            ..candidate.content.end,
                    );
                    end_ix = jx;
                    break;
                }
            }
            let end_line = &lines[end_ix];
            blocks.push(Block {
                range: line.full.start..end_line.full.end,
                kind: BlockKind::CodeBlock {
                    fence_start,
                    fence_end,
                    lang,
                },
                inlines: Vec::new(),
            });
            ix = end_ix + 1;
            continue;
        }

        // заголовок
        if let Some((level, marker_end_col)) = parse_heading(&text) {
            let marker_end = line.content.start + marker_end_col;
            blocks.push(Block {
                range: line.full.clone(),
                kind: BlockKind::Heading {
                    level,
                    marker: line.content.start..marker_end,
                },
                inlines: Vec::new(),
            });
            ix += 1;
            continue;
        }

        // горизонтальная линия
        if is_rule(&text) {
            blocks.push(Block {
                range: line.full.clone(),
                kind: BlockKind::Rule,
                inlines: Vec::new(),
            });
            ix += 1;
            continue;
        }

        // цитата: подряд идущие строки с `>`
        if trimmed_start_offset(&text).is_some_and(|off| text[off..].starts_with('>')) {
            let mut markers = Vec::new();
            let start = line.full.start;
            let mut end = line.full.end;
            let mut jx = ix;
            while jx < lines.len() {
                let t = line_str(chars, &lines[jx].content);
                match quote_marker(&t, lines[jx].content.start) {
                    Some(marker) => {
                        markers.push(marker);
                        end = lines[jx].full.end;
                        jx += 1;
                    }
                    None => break,
                }
            }
            blocks.push(Block {
                range: start..end,
                kind: BlockKind::Quote { markers },
                inlines: Vec::new(),
            });
            ix = jx;
            continue;
        }

        // пункт списка
        if let Some((marker_col, _content_col, ordered, checkbox)) =
            parse_list_item(&text, line.content.start)
        {
            blocks.push(Block {
                range: line.full.clone(),
                kind: BlockKind::ListItem {
                    marker: line.content.start..line.content.start + marker_col,
                    ordered,
                    checkbox,
                },
                inlines: Vec::new(),
            });
            ix += 1;
            continue;
        }

        // абзац: подряд идущие «обычные» строки
        let start = line.full.start;
        let mut end = line.full.end;
        let mut jx = ix + 1;
        while jx < lines.len() {
            let t = line_str(chars, &lines[jx].content);
            if is_blank(chars, &lines[jx].content)
                || parse_heading(&t).is_some()
                || is_rule(&t)
                || parse_fence(&t).is_some()
                || quote_marker(&t, 0).is_some()
                || parse_list_item(&t, 0).is_some()
            {
                break;
            }
            end = lines[jx].full.end;
            jx += 1;
        }
        blocks.push(Block {
            range: start..end,
            kind: BlockKind::Paragraph,
            inlines: Vec::new(),
        });
        ix = jx;
    }

    blocks
}

/// `(символ забора, длина, язык)` для открывающей code-фенции.
fn parse_fence(text: &str) -> Option<(char, usize, Option<String>)> {
    let trimmed = text.trim_start();
    for fence_char in ['`', '~'] {
        let len = trimmed.chars().take_while(|c| *c == fence_char).count();
        if len >= 3 {
            let lang = trimmed[len..].trim();
            let lang = if lang.is_empty() {
                None
            } else {
                Some(lang.to_string())
            };
            return Some((fence_char, len, lang));
        }
    }
    None
}

fn closes_fence(text: &str, fence_char: char, fence_len: usize) -> bool {
    let trimmed = text.trim_start();
    let len = trimmed.chars().take_while(|c| *c == fence_char).count();
    len >= fence_len && trimmed[len..].trim().is_empty()
}

/// `(уровень, колонка конца маркера)` для `# Заголовок`.
fn parse_heading(text: &str) -> Option<(u8, usize)> {
    let hashes = text.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &text[hashes..];
    if rest.is_empty() {
        return Some((hashes as u8, hashes));
    }
    if rest.starts_with(' ') {
        return Some((hashes as u8, hashes + 1));
    }
    None
}

/// `---`, `***`, `___` (три и более одинаковых символа).
fn is_rule(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.len() >= 3
        && (trimmed.chars().all(|c| c == '-')
            || trimmed.chars().all(|c| c == '*')
            || trimmed.chars().all(|c| c == '_'))
}

/// Маркер `>` строки цитаты.
fn quote_marker(text: &str, base: usize) -> Option<Range<usize>> {
    let offset = trimmed_start_offset(text)?;
    if !text[offset..].starts_with('>') {
        return None;
    }
    let mut end = base + offset + 1;
    if text[offset..].starts_with("> ") {
        end += 1;
    }
    Some(base + offset..end)
}

/// `(конец маркера, начало содержимого, номер, чекбокс)` для пункта списка.
/// Колонки — позиции в строке.
fn parse_list_item(
    text: &str,
    base: usize,
) -> Option<(usize, usize, Option<u64>, Option<Checkbox>)> {
    let offset = trimmed_start_offset(text).unwrap_or(0);
    let rest = &text[offset..];

    // маркер: `-`/`*`/`+` или цифры с `.`/`)`
    let (marker_len, ordered) = {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let after = &rest[digits.len()..];
            let sep = after.chars().next()?;
            if (sep == '.' || sep == ')') && after[sep.len_utf8()..].starts_with(' ') {
                let num: u64 = digits.parse().ok()?;
                (digits.len() + 1 + 1, Some(num))
            } else {
                return None;
            }
        } else {
            let marker = rest.chars().next()?;
            if !matches!(marker, '-' | '*' | '+') || !rest[marker.len_utf8()..].starts_with(' ') {
                return None;
            }
            (1 + 1, None)
        }
    };

    // маркер занимает marker_len символов с позиции offset (маркер + ровно один пробел);
    // допускаем дополнительные пробелы как часть отступа содержимого
    let mut content_col = offset + marker_len;
    while text.get(content_col..).is_some_and(|r| r.starts_with(' ')) {
        content_col += 1;
    }
    let marker_col = content_col;

    // чекбокс
    let checkbox = {
        let rest = &text[content_col..];
        if rest.starts_with("[ ] ") {
            content_col += 4;
            Some(Checkbox {
                range: base + content_col - 4..base + content_col - 1,
                checked: false,
            })
        } else if rest.starts_with("[x] ") || rest.starts_with("[X] ") {
            content_col += 4;
            Some(Checkbox {
                range: base + content_col - 4..base + content_col - 1,
                checked: true,
            })
        } else {
            None
        }
    };

    // (конец маркера, начало содержимого, номер, чекбокс) — колонки в строке
    Some((marker_col, content_col, ordered, checkbox))
}

/// Смещение начала текста без ведущих пробелов.
fn trimmed_start_offset(text: &str) -> Option<usize> {
    let offset = text.len() - text.trim_start().len();
    (offset < text.len()).then_some(offset)
}
