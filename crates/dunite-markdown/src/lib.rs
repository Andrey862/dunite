//! Парсинг markdown для Live Preview (SPEC §8.2).
//!
//! Результат — блоки и инлайны с точными char-диапазонами в исходном тексте:
//! этого достаточно для декораций, concealment и кликабельных элементов.
//!
//! Сканер «достаточно правильный», а не канонический: markdown в исходнике
//! остаётся нетронутым (правки идут только точечными диапазонами модели
//! редактора), поэтому полная корректность парсинга не критична.

mod blocks;
mod inlines;
mod types;

pub use types::{Block, BlockKind, Checkbox, Inline, Marker, ParsedNote};

use std::ops::Range;

/// Разбирает текст заметки в блоки с инлайн-декорациями.
pub fn parse(text: &str) -> ParsedNote {
    let chars: Vec<char> = text.chars().collect();
    let blocks = blocks::scan_blocks(&chars);
    let blocks = blocks
        .into_iter()
        .map(|block| inlines::decorate_block(&chars, block))
        .collect();
    ParsedNote { blocks }
}

/// Byte-диапазон → char-диапазон (для тестов и мостов с внешними API).
pub fn byte_range_to_char(text: &str, range: Range<usize>) -> Range<usize> {
    let start = text[..range.start].chars().count();
    let end = text[..range.end].chars().count();
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(text: &str, range: &Range<usize>) -> String {
        text.chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect()
    }

    #[test]
    fn heading_block_and_marker() {
        let text = "## Заголовок\n";
        let note = parse(text);
        assert_eq!(note.blocks.len(), 1);
        let block = &note.blocks[0];
        match &block.kind {
            BlockKind::Heading { level, marker } => {
                assert_eq!(*level, 2);
                assert_eq!(slice(text, marker), "## ");
            }
            other => panic!("ожидался заголовок, получено {other:?}"),
        }
    }

    #[test]
    fn paragraphs_split_by_blank_lines() {
        let text = "один\nдва\n\nтри\n";
        let note = parse(text);
        assert_eq!(note.blocks.len(), 2);
        assert!(matches!(note.blocks[0].kind, BlockKind::Paragraph));
        assert!(matches!(note.blocks[1].kind, BlockKind::Paragraph));
    }

    #[test]
    fn list_items_are_separate_blocks() {
        let text = "- один\n- два\n";
        let note = parse(text);
        assert_eq!(note.blocks.len(), 2);
        for block in &note.blocks {
            assert!(matches!(block.kind, BlockKind::ListItem { .. }));
        }
    }

    #[test]
    fn checkbox_parsing() {
        let text = "- [x] сделано\n- [ ] сделать\n- без чекбокса\n";
        let note = parse(text);
        let checks: Vec<Option<bool>> = note
            .blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::ListItem { checkbox, .. } => checkbox.as_ref().map(|c| c.checked),
                _ => panic!("ожидался пункт списка"),
            })
            .collect();
        assert_eq!(checks, vec![Some(true), Some(false), None]);
    }

    #[test]
    fn code_fence_with_language() {
        let text = "```rust\nfn main() {}\n```\n";
        let note = parse(text);
        assert_eq!(note.blocks.len(), 1);
        match &note.blocks[0].kind {
            BlockKind::CodeBlock {
                lang, fence_end, ..
            } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert!(fence_end.is_some());
            }
            other => panic!("ожидался code-блок, получено {other:?}"),
        }
        // внутри code-блока нет инлайн-декораций
        assert!(note.blocks[0].inlines.is_empty());
    }

    #[test]
    fn quote_and_rule() {
        let text = "> цитата\n\n---\n\nтекст\n";
        let note = parse(text);
        assert!(matches!(note.blocks[0].kind, BlockKind::Quote { .. }));
        assert!(matches!(note.blocks[1].kind, BlockKind::Rule));
        assert!(matches!(note.blocks[2].kind, BlockKind::Paragraph));
    }

    #[test]
    fn frontmatter_detected() {
        let text = "---\ntitle: Тест\ntags: [a]\n---\n\nтекст\n";
        let note = parse(text);
        assert!(matches!(note.blocks[0].kind, BlockKind::Frontmatter));
        assert!(matches!(note.blocks[1].kind, BlockKind::Paragraph));
    }

    #[test]
    fn inline_bold_italic_code() {
        let text = "обычный **жирный** и *курсив* и `код`\n";
        let note = parse(text);
        let kinds: Vec<&Inline> = note.blocks[0].inlines.iter().collect();
        assert_eq!(kinds.len(), 3);
        assert!(matches!(kinds[0], Inline::Emphasis { strong: true, .. }));
        assert!(matches!(kinds[1], Inline::Emphasis { strong: false, .. }));
        assert!(matches!(kinds[2], Inline::CodeSpan { .. }));
    }

    #[test]
    fn inline_wikilink_display_and_embed() {
        let text = "[[Alpha]] и [[Alpha|с надписью]] и ![[pic.png]]\n";
        let note = parse(text);
        let inlines = &note.blocks[0].inlines;
        assert_eq!(inlines.len(), 3);
        match &inlines[0] {
            Inline::WikiLink {
                embed,
                target,
                display,
                ..
            } => {
                assert!(!embed);
                assert_eq!(slice(text, target), "Alpha");
                assert!(display.is_none());
            }
            other => panic!("ожидалась ссылка: {other:?}"),
        }
        match &inlines[1] {
            Inline::WikiLink {
                display: Some(display),
                ..
            } => {
                assert_eq!(slice(text, display), "с надписью");
            }
            other => panic!("ожидалась ссылка с надписью: {other:?}"),
        }
        match &inlines[2] {
            Inline::WikiLink { embed, target, .. } => {
                assert!(embed);
                assert_eq!(slice(text, target), "pic.png");
            }
            other => panic!("ожидался embed: {other:?}"),
        }
    }

    #[test]
    fn inline_tag_and_markdown_link() {
        let text = "тег #work/sub и [сайт](https://example.com)\n";
        let note = parse(text);
        let inlines = &note.blocks[0].inlines;
        assert_eq!(inlines.len(), 2);
        assert!(matches!(inlines[0], Inline::Tag { .. }));
        match &inlines[1] {
            Inline::MarkdownLink {
                target,
                display,
                image,
                ..
            } => {
                assert!(!image);
                assert_eq!(slice(text, target), "https://example.com");
                assert_eq!(slice(text, display.as_ref().unwrap()), "сайт");
            }
            other => panic!("ожидалась ссылка: {other:?}"),
        }
    }

    #[test]
    fn escaped_markers_are_literal() {
        let text = "\\*не курсив\\* и \\#не-тег\n";
        let note = parse(text);
        assert!(note.blocks[0].inlines.is_empty());
    }

    #[test]
    fn incomplete_wikilink_not_a_link() {
        let text = "незакрытая [[ссылка\n";
        let note = parse(text);
        assert!(note.blocks[0].inlines.is_empty());
    }

    #[test]
    fn block_id_inline() {
        let text = "заметка ^abc123\n";
        let note = parse(text);
        assert!(matches!(note.blocks[0].inlines[0], Inline::BlockId { .. }));
    }

    #[test]
    fn unicode_ranges_are_char_correct() {
        let text = "привет **мир** #тег\n";
        let note = parse(text);
        let inlines = &note.blocks[0].inlines;
        assert_eq!(slice(text, &inlines[0].range()), "**мир**");
        assert_eq!(slice(text, &inlines[1].range()), "#тег");
    }
}
