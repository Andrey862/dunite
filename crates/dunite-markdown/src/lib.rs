//! Парсинг markdown (SPEC §8.2).
//!
//! M0: блочное разбиение текста — единица инкрементального репарсинга в M2.
//! M2: декорации, concealment, wikilinks/теги/embeds.

use std::ops::Range;

/// Разбивает текст на блоки — максимальные runs непустых строк.
///
/// Контракт:
/// - диапазоны — байтовые смещения в исходном тексте;
/// - пустые строки-разделители не входят ни в один диапазон;
/// - диапазоны идут по возрастанию и не пересекаются;
/// - каждая непустая строка покрыта ровно одним диапазоном.
pub fn block_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut block: Option<Range<usize>> = None;
    let mut offset = 0usize;

    for line in text.split_inclusive('\n') {
        let line_range = offset..offset + line.len();
        offset += line.len();

        if line.trim().is_empty() {
            if let Some(range) = block.take() {
                ranges.push(range);
            }
        } else {
            match &mut block {
                Some(range) => range.end = line_range.end,
                None => block = Some(line_range),
            }
        }
    }
    if let Some(range) = block {
        ranges.push(range);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_has_no_blocks() {
        assert!(block_ranges("").is_empty());
        assert!(block_ranges("\n\n  \n").is_empty());
    }

    #[test]
    fn single_block() {
        let text = "# Заголовок\nтекст\n";
        assert_eq!(block_ranges(text), vec![0..text.len()]);
    }

    #[test]
    fn blocks_split_on_blank_lines() {
        let text = "a\nb\n\nc\n\n\nd";
        let ranges = block_ranges(text);
        assert_eq!(ranges.len(), 3);
        assert_eq!(&text[ranges[0].clone()], "a\nb\n");
        assert_eq!(&text[ranges[1].clone()], "c\n");
        assert_eq!(&text[ranges[2].clone()], "d");
    }

    #[test]
    fn unicode_offsets_are_byte_correct() {
        let text = "привет\nмир\n\nещё";
        let ranges = block_ranges(text);
        assert_eq!(&text[ranges[0].clone()], "привет\nмир\n");
        assert_eq!(&text[ranges[1].clone()], "ещё");
    }

    #[test]
    fn non_empty_lines_fully_covered() {
        let text = "a\n\nbb\n\n\nccc\n";
        let ranges = block_ranges(text);
        let mut covered = String::new();
        for r in &ranges {
            covered.push_str(&text[r.clone()]);
        }
        assert_eq!(covered, "a\nbb\nccc\n");
    }
}
