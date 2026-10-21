//! Типы разбора markdown (SPEC §8.2).
//!
//! Все диапазоны — char-смещения в исходном тексте заметки.

use std::ops::Range;

/// Результат разбора заметки.
#[derive(Debug, Clone)]
pub struct ParsedNote {
    pub blocks: Vec<Block>,
}

/// Блок markdown — единица layout и concealment.
#[derive(Debug, Clone)]
pub struct Block {
    /// Диапазон блока (включая маркеры; пустые строки-разделители не входят).
    pub range: Range<usize>,
    pub kind: BlockKind,
    /// Инлайн-декорации внутри блока (по возрастанию, не пересекаются).
    pub inlines: Vec<Inline>,
}

/// Синтаксический маркер блока — скрывается в режиме concealment.
#[derive(Debug, Clone)]
pub struct Marker {
    pub range: Range<usize>,
}

#[derive(Debug, Clone)]
pub enum BlockKind {
    /// YAML-frontmatter в начале заметки.
    Frontmatter,
    /// Заголовок: `#`–`######`.
    Heading {
        level: u8,
        /// `## ` — префикс (диапазон скрывается вне активного блока).
        marker: Range<usize>,
    },
    Paragraph,
    /// Цитата: `>` в начале строк.
    Quote {
        /// Маркеры `> ` каждой строки блока.
        markers: Vec<Range<usize>>,
    },
    /// Fenced code-блок.
    CodeBlock {
        fence_start: Range<usize>,
        fence_end: Option<Range<usize>>,
        lang: Option<String>,
    },
    /// Горизонтальная линия `---`, `***`, `___`.
    Rule,
    /// Пункт списка.
    ListItem {
        /// Маркер `- `, `1. ` и т.п. (скрывается, рисуется декорацией).
        marker: Range<usize>,
        /// Номер для нумерованных списков.
        ordered: Option<u64>,
        checkbox: Option<Checkbox>,
    },
}

/// Чекбокс задачи `- [ ]` / `- [x]`.
#[derive(Debug, Clone)]
pub struct Checkbox {
    /// Диапазон `[x]` (скрывается, рисуется кликабельным виджетом).
    pub range: Range<usize>,
    pub checked: bool,
}

/// Инлайн-декорация.
#[derive(Debug, Clone)]
pub enum Inline {
    /// `**жирный**` / `*курсив*`.
    Emphasis {
        range: Range<usize>,
        marker_start: Range<usize>,
        marker_end: Range<usize>,
        strong: bool,
    },
    /// `~~зачёркнутый~~`.
    Strikethrough {
        range: Range<usize>,
        marker_start: Range<usize>,
        marker_end: Range<usize>,
    },
    /// `` `код` ``.
    CodeSpan {
        range: Range<usize>,
        /// Маркеры обратных кавычек (открывающий и закрывающий).
        markers: Vec<Range<usize>>,
    },
    /// `[[Заметка]]`, `[[Заметка|надпись]]`, `![[pic.png]]`.
    WikiLink {
        range: Range<usize>,
        target: Range<usize>,
        display: Option<Range<usize>>,
        embed: bool,
    },
    /// `[надпись](target)`, `![alt](pic.png)`.
    MarkdownLink {
        range: Range<usize>,
        target: Range<usize>,
        display: Option<Range<usize>>,
        image: bool,
    },
    /// `#тег/подтег`.
    Tag { range: Range<usize> },
    /// `^block-id`.
    BlockId { range: Range<usize> },
}

impl Inline {
    /// Полный диапазон инлайна в исходном тексте.
    pub fn range(&self) -> Range<usize> {
        match self {
            Inline::Emphasis { range, .. }
            | Inline::Strikethrough { range, .. }
            | Inline::CodeSpan { range, .. }
            | Inline::WikiLink { range, .. }
            | Inline::MarkdownLink { range, .. }
            | Inline::Tag { range }
            | Inline::BlockId { range } => range.clone(),
        }
    }

    /// Диапазон, остающийся видимым при concealment (остальное скрывается).
    ///
    /// `**жирный**` → видно `жирный`; `[[Alpha|X]]` → видно `X`;
    /// `#тег` → видно `тег`; `^id` → скрыт целиком.
    pub fn visible_range(&self) -> Range<usize> {
        match self {
            Inline::Emphasis {
                marker_start,
                marker_end,
                ..
            }
            | Inline::Strikethrough {
                marker_start,
                marker_end,
                ..
            } => marker_start.end..marker_end.start,
            Inline::CodeSpan { markers, .. } => {
                let start = markers.first().map(|m| m.end).unwrap_or(0);
                let end = markers.last().map(|m| m.start).unwrap_or(0);
                start..end
            }
            Inline::WikiLink {
                target, display, ..
            } => display.clone().unwrap_or_else(|| target.clone()),
            Inline::MarkdownLink {
                target, display, ..
            } => display.clone().unwrap_or_else(|| target.clone()),
            Inline::Tag { range } => range.start + 1..range.end,
            Inline::BlockId { range } => range.end..range.end,
        }
    }
}
