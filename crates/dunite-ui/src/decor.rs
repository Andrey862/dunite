//! Декорации Live Preview (SPEC §8.2).
//!
//! Для каждой строки строится список «кусочков» (Piece): что показать вместо
//! исходного текста. Маркеры списков/чекбоксов подменяются глифами, скрытые
//! конструкции исчезают (concealment), стили расставляются по инлайнам.
//!
//! Сопоставление «символ исходника ↔ видимый текст» хранится в каждом кусочке
//! (`char`) — это нужно для кликов (ссылки, чекбоксы) и для курсора.

use std::ops::Range;

use dunite_markdown::{BlockKind, Checkbox, Inline, ParsedNote};

// ── стили ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleColor {
    Default,
    Muted,
    Accent,
    Heading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub color: StyleColor,
    pub code_bg: bool,
}

impl Default for SegStyle {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            strike: false,
            underline: false,
            color: StyleColor::Default,
            code_bg: false,
        }
    }
}

impl SegStyle {
    pub fn heading(_level: u8) -> Self {
        Self {
            bold: true,
            color: StyleColor::Heading,
            ..Self::default()
        }
    }

    pub fn muted() -> Self {
        Self {
            color: StyleColor::Muted,
            ..Self::default()
        }
    }

    pub fn accent(underline: bool) -> Self {
        Self {
            color: StyleColor::Accent,
            underline,
            ..Self::default()
        }
    }
}

// ── результат декорирования строки ───────────────────────────────────────

/// Кусок видимого текста строки.
#[derive(Debug, Clone)]
pub struct Piece {
    /// Char-диапазон в исходнике (для кликов и курсора).
    pub char: Range<usize>,
    /// Видимый текст куска (глиф-подмена или подстрока исходника).
    pub text: String,
    pub style: SegStyle,
}

/// Фоновая/векторная декорация строки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineQuad {
    /// Горизонтальная линия `---`.
    Rule,
    /// Вертикальная полоса цитаты.
    QuoteBar,
    /// Фон code-блока.
    CodeBg,
    /// Фон frontmatter.
    FrontmatterBg,
}

/// Полное декорирование строки.
#[derive(Debug, Clone)]
pub struct LineDecor {
    pub pieces: Vec<Piece>,
    /// Высота строки в пикселях.
    pub height: f32,
    /// Размер шрифта строки.
    pub font_size: f32,
    pub quads: Vec<LineQuad>,
    /// Стиль «по умолчанию» для незакрытых кусков (зависит от блока).
    pub base_style: SegStyle,
}

/// Метрики строки по умолчанию (константы SPEC §8.4).
pub const DEFAULT_HEIGHT: f32 = 20.;
pub const DEFAULT_FONT_SIZE: f32 = 13.;

/// Ширина плашки inline-картинки (в символах моноширинного шрифта):
/// фиксированная под миниатюру, чтобы не оставлять дыру после неё.
const IMAGE_CHIP_WIDTH: usize = 8;

/// Высота строки с inline-миниатюрой — картинка вписывается в строку.
const IMAGE_LINE_HEIGHT: f32 = 36.;

/// Цель standalone-картинки на строке (строка целиком занята embed'ом).
///
/// `line_text` — содержимое строки; возвращает char-диапазон цели embed'а.
pub fn standalone_image_target(
    parsed: &ParsedNote,
    line_range: &Range<usize>,
    line_text: &str,
) -> Option<Range<usize>> {
    let mut found = None;
    for block in &parsed.blocks {
        if block.range.end <= line_range.start || block.range.start >= line_range.end {
            continue;
        }
        for inline in &block.inlines {
            let is_image = matches!(
                inline,
                Inline::WikiLink { embed: true, .. } | Inline::MarkdownLink { image: true, .. }
            );
            if !is_image {
                continue;
            }
            let range = inline.range();
            if range.start < line_range.start || range.end > line_range.end {
                continue;
            }
            // embed — единственное содержимое строки (кроме пробелов)
            let before = range.start - line_range.start;
            let after = range.end - line_range.start;
            let only_embed = line_text.chars().take(before).all(|c| c.is_whitespace())
                && line_text.chars().skip(after).all(|c| c.is_whitespace());
            if only_embed && found.is_none() {
                found = Some(match inline {
                    Inline::WikiLink { target, .. } | Inline::MarkdownLink { target, .. } => {
                        target.clone()
                    }
                    _ => unreachable!("checked is_image"),
                });
            }
        }
    }
    found
}

/// Высота и размер шрифта строки по типу блока.
pub fn line_metrics(kind: Option<&BlockKind>) -> (f32, f32) {
    match kind {
        Some(BlockKind::Heading { level, .. }) => match level {
            1 => (34., 22.),
            2 => (30., 18.),
            3 => (26., 15.),
            _ => (23., 13.5),
        },
        Some(BlockKind::Rule) => (24., DEFAULT_FONT_SIZE),
        Some(BlockKind::CodeBlock { .. }) => (20., 12.5),
        Some(BlockKind::Frontmatter) => (18., 11.5),
        _ => (DEFAULT_HEIGHT, DEFAULT_FONT_SIZE),
    }
}

// ── построение декораций ─────────────────────────────────────────────────

/// Декорирует строку: `line_range` — char-диапазон содержимого строки,
/// `revealed_blocks` — индексы блоков, чьи маркеры показаны (блок с курсором
/// и блоки выделения; SPEC §8.2).
pub fn decorate_line(
    parsed: &ParsedNote,
    line_range: Range<usize>,
    line_text: &str,
    revealed_blocks: &[usize],
) -> LineDecor {
    let block_ix = parsed.blocks.iter().position(|block| {
        block.range.start <= line_range.start && line_range.start < block.range.end
    });
    let block = block_ix.and_then(|ix| parsed.blocks.get(ix));
    let active = block_ix.is_some_and(|ix| revealed_blocks.contains(&ix));

    let (mut height, font_size) = line_metrics(block.map(|b| &b.kind));
    let mut has_image_chip = false;
    let base_style = match block.map(|b| &b.kind) {
        Some(BlockKind::Heading { level, .. }) => SegStyle::heading(*level),
        Some(BlockKind::Quote { .. }) | Some(BlockKind::Frontmatter) => SegStyle::muted(),
        _ => SegStyle::default(),
    };

    let mut plans: Vec<Plan> = Vec::new();
    let mut quads: Vec<LineQuad> = Vec::new();

    // ── блочные маркеры ──
    if let Some(block) = block {
        match &block.kind {
            BlockKind::Heading { marker, .. } => {
                push_plan(
                    &mut plans,
                    intersect(marker, &line_range),
                    if active {
                        Action::Keep(base_style)
                    } else {
                        Action::Drop
                    },
                );
            }
            BlockKind::Quote { markers } => {
                quads.push(LineQuad::QuoteBar);
                for marker in markers {
                    let Some(range) = intersect(marker, &line_range) else {
                        continue;
                    };
                    let width = range.end - range.start;
                    push_plan(
                        &mut plans,
                        Some(range),
                        Action::Replace(" ".repeat(width), SegStyle::muted()),
                    );
                }
            }
            BlockKind::ListItem {
                marker,
                ordered,
                checkbox,
            } => {
                // отступ остаётся видимым, маркер подменяется глифом
                let indent = marker_start_indent(line_text);
                if let Some(range) = intersect(marker, &line_range) {
                    let proper_start = range.start + indent.min(range.end - range.start);
                    if proper_start > range.start {
                        push_plan(
                            &mut plans,
                            Some(range.start..proper_start),
                            Action::Keep(base_style),
                        );
                    }
                    if range.end > proper_start {
                        let width = range.end - proper_start;
                        let (glyph, style) = match ordered {
                            Some(_) => (
                                line_text[byte_of_char(line_text, proper_start - range.start)..]
                                    .chars()
                                    .take(width)
                                    .collect::<String>(),
                                SegStyle::accent(false),
                            ),
                            None => (
                                format!("{}{}", '•', " ".repeat(width - 1)),
                                SegStyle::accent(false),
                            ),
                        };
                        push_plan(
                            &mut plans,
                            Some(proper_start..range.end),
                            Action::Replace(glyph, style),
                        );
                    }
                }
                if let Some(Checkbox { range, checked }) = checkbox {
                    if let Some(range) = intersect(range, &line_range) {
                        let width = range.end - range.start;
                        let glyph = if *checked { "☑" } else { "☐" };
                        push_plan(
                            &mut plans,
                            Some(range),
                            Action::Replace(
                                format!("{}{}", glyph, " ".repeat(width - 1)),
                                SegStyle::accent(false),
                            ),
                        );
                    }
                }
            }
            BlockKind::CodeBlock {
                fence_start,
                fence_end,
                ..
            } => {
                quads.push(LineQuad::CodeBg);
                let fence_style = SegStyle::muted();
                for fence in [Some(fence_start), fence_end.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    push_plan(
                        &mut plans,
                        intersect(fence, &line_range),
                        if active {
                            Action::Keep(fence_style)
                        } else {
                            Action::Drop
                        },
                    );
                }
            }
            BlockKind::Rule => {
                quads.push(LineQuad::Rule);
                push_plan(&mut plans, Some(line_range.clone()), Action::Drop);
            }
            BlockKind::Frontmatter => {
                quads.push(LineQuad::FrontmatterBg);
                push_plan(
                    &mut plans,
                    Some(line_range.clone()),
                    Action::Keep(SegStyle::muted()),
                );
            }
            BlockKind::Paragraph => {}
        }
    }

    // ── инлайны ──
    if let Some(block) = block {
        for inline in &block.inlines {
            let Some(range) = intersect(&inline.range(), &line_range) else {
                continue;
            };
            // inline-картинки (не занимающие всю строку): синтаксис
            // заменяется плашкой — поверх рисуется миниатюра (SPEC §8.2);
            // в активном блоке синтаксис виден для редактирования
            if !active
                && is_image_embed(inline)
                && embed_target_range(inline).is_some_and(|target| {
                    looks_like_image(&slice_chars(line_text, &line_range, &target))
                        && !is_only_content_on_line(&line_range, &inline.range(), line_text)
                })
            {
                // плашка фиксированной ширины под миниатюру: подмена той же
                // длины, что и синтаксис, оставляла бы дыру после картинки
                push_plan(
                    &mut plans,
                    Some(range),
                    Action::Replace(" ".repeat(IMAGE_CHIP_WIDTH), SegStyle::muted()),
                );
                has_image_chip = true;
                continue;
            }
            let style = inline_style(inline);
            if active {
                push_plan(&mut plans, Some(range), Action::Keep(style));
            } else {
                // видимой остаётся только visible_range, маркеры скрываются
                let visible = inline.visible_range();
                if visible.is_empty() {
                    push_plan(&mut plans, Some(range), Action::Drop);
                    continue;
                }
                let (start, end) = (range.start, range.end);
                if start < visible.start {
                    push_plan(
                        &mut plans,
                        Some(start..visible.start.min(end)),
                        Action::Drop,
                    );
                }
                let mid_start = start.max(visible.start);
                let mid_end = end.min(visible.end);
                if mid_start < mid_end {
                    push_plan(&mut plans, Some(mid_start..mid_end), Action::Keep(style));
                }
                if visible.end < end {
                    push_plan(&mut plans, Some(visible.end.max(start)..end), Action::Drop);
                }
            }
        }
    }

    let pieces = materialize(line_range, line_text, plans, base_style);
    // строки с inline-миниатюрами выше — картинка вписывается в строку
    if has_image_chip {
        height = height.max(IMAGE_LINE_HEIGHT);
    }
    LineDecor {
        pieces,
        height,
        font_size,
        quads,
        base_style,
    }
}

// ── внутренние детали ────────────────────────────────────────────────────

enum Action {
    Keep(SegStyle),
    /// Подмена текстом той же «ширины» (глиф маркера).
    Replace(String, SegStyle),
    Drop,
}

struct Plan {
    range: Range<usize>,
    action: Action,
}

fn push_plan(plans: &mut Vec<Plan>, range: Option<Range<usize>>, action: Action) {
    if let Some(range) = range {
        if range.start < range.end {
            plans.push(Plan { range, action });
        }
    }
}

fn intersect(range: &Range<usize>, line: &Range<usize>) -> Option<Range<usize>> {
    let start = range.start.max(line.start);
    let end = range.end.min(line.end);
    (start < end).then_some(start..end)
}

/// Сколько символов отступа (пробелов/табов) в начале строки.
fn marker_start_indent(line_text: &str) -> usize {
    line_text.chars().take_while(|c| c.is_whitespace()).count()
}

fn byte_of_char(text: &str, col: usize) -> usize {
    text.char_indices()
        .nth(col)
        .map(|(byte_ix, _)| byte_ix)
        .unwrap_or(text.len())
}

fn inline_style(inline: &Inline) -> SegStyle {
    match inline {
        Inline::Emphasis { strong, .. } => SegStyle {
            bold: *strong,
            italic: !*strong,
            ..SegStyle::default()
        },
        Inline::Strikethrough { .. } => SegStyle {
            strike: true,
            ..SegStyle::default()
        },
        Inline::CodeSpan { .. } => SegStyle {
            code_bg: true,
            color: StyleColor::Default,
            ..SegStyle::default()
        },
        Inline::WikiLink { embed, .. } => {
            if *embed {
                SegStyle::muted()
            } else {
                SegStyle::accent(true)
            }
        }
        Inline::MarkdownLink { image, .. } => {
            if *image {
                SegStyle::muted()
            } else {
                SegStyle::accent(true)
            }
        }
        Inline::Tag { .. } => SegStyle {
            code_bg: true,
            color: StyleColor::Accent,
            ..SegStyle::default()
        },
        Inline::BlockId { .. } => SegStyle::muted(),
    }
}

/// Embed-ссылка (встраивание): `![[...]]` / `![...](...)`.
pub fn is_image_embed(inline: &Inline) -> bool {
    matches!(
        inline,
        Inline::WikiLink { embed: true, .. } | Inline::MarkdownLink { image: true, .. }
    )
}

/// Char-диапазон цели embed'а.
pub fn embed_target_range(inline: &Inline) -> Option<Range<usize>> {
    match inline {
        Inline::WikiLink {
            target,
            embed: true,
            ..
        }
        | Inline::MarkdownLink {
            target,
            image: true,
            ..
        } => Some(target.clone()),
        _ => None,
    }
}

/// По расширению — изображение ли это (для миниатюр и кэша).
pub fn looks_like_image(target: &str) -> bool {
    let lower = target.to_lowercase();
    ["png", "jpg", "jpeg", "gif", "webp", "svg"]
        .iter()
        .any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// Фрагмент строки по глобальному char-диапазону.
fn slice_chars(line_text: &str, line_range: &Range<usize>, range: &Range<usize>) -> String {
    let from = range.start.saturating_sub(line_range.start);
    let to = range.end.clamp(range.start, line_range.end) - line_range.start;
    line_text.chars().skip(from).take(to - from).collect()
}

/// Диапазон — единственное содержимое строки (кроме пробелов).
fn is_only_content_on_line(
    line_range: &Range<usize>,
    range: &Range<usize>,
    line_text: &str,
) -> bool {
    let blank = |span: Range<usize>| {
        let from = span.start.saturating_sub(line_range.start);
        let to = span.end.clamp(span.start, line_range.end) - line_range.start;
        line_text
            .chars()
            .skip(from)
            .take(to - from)
            .all(|c| c.is_whitespace())
    };
    blank(line_range.start..range.start) && blank(range.end..line_range.end)
}

/// Собирает куски: сортирует планы, устраняет пересечения, заполняет пробелы.
///
/// `line_range` — глобальный char-диапазон строки; `line_text` — её содержимое,
/// поэтому срезы идут по смещениям, локальным для строки (`base`).
fn materialize(
    line_range: Range<usize>,
    line_text: &str,
    mut plans: Vec<Plan>,
    base_style: SegStyle,
) -> Vec<Piece> {
    let base = line_range.start;
    plans.sort_by_key(|plan| plan.range.start);
    let mut pieces = Vec::new();
    let mut pos = line_range.start;

    for plan in plans {
        if plan.range.start < pos {
            continue;
        }
        let plan_end = plan.range.end;
        if plan.range.start > pos {
            pieces.push(text_piece(
                line_text,
                base,
                pos..plan.range.start,
                base_style,
            ));
        }
        match plan.action {
            Action::Keep(style) => {
                pieces.push(text_piece(line_text, base, plan.range.clone(), style))
            }
            Action::Replace(text, style) => pieces.push(Piece {
                char: plan.range,
                text,
                style,
            }),
            Action::Drop => {}
        }
        pos = plan_end;
    }
    if pos < line_range.end {
        pieces.push(text_piece(line_text, base, pos..line_range.end, base_style));
    }
    pieces
}

fn text_piece(line_text: &str, base: usize, range: Range<usize>, style: SegStyle) -> Piece {
    let from = byte_of_char(line_text, range.start - base);
    let to = byte_of_char(line_text, range.end - base);
    Piece {
        char: range,
        text: line_text[from..to].chars().map(safe_char).collect(),
        style,
    }
}

/// Убирает символы, ломающие шейпер текста (cosmic-text паникует на строках
/// со смешанными bidi-абзацами: U+0085, U+2029 и управляющие коды).
/// Число символов сохраняется — сопоставление «курсор ↔ видимый текст»
/// остаётся корректным.
fn safe_char(c: char) -> char {
    if c == '\t' {
        '\t'
    } else if c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
        ' '
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decor_for(text: &str, cursor: usize) -> (ParsedNote, LineDecor) {
        let parsed = dunite_markdown::parse(text);
        let line_range = 0..text.trim_end_matches('\n').chars().count();
        let revealed: Vec<usize> = parsed.block_index_at(cursor).into_iter().collect();
        let decor = decorate_line(&parsed, line_range, text.trim_end_matches('\n'), &revealed);
        (parsed, decor)
    }

    #[test]
    fn bullet_replaced_with_glyph() {
        let (parsed, decor) = decor_for("- пункт\n", 3);
        assert_eq!(decor.pieces[0].text, "• ");
        assert_eq!(decor.pieces[1].text, "пункт");
        // маркер скрыт: глиф ссылается на исходный диапазон
        assert_eq!(decor.pieces[0].char, 0..2);
        assert_eq!(parsed.blocks.len(), 1);
    }

    #[test]
    fn checkbox_replaced_with_glyph() {
        let (_, decor) = decor_for("- [x] задача\n", 10);
        assert!(decor.pieces.iter().any(|p| p.text.starts_with('☑')));
    }

    #[test]
    fn heading_marker_hidden_when_inactive() {
        let text = "# Заголовок\nтекст\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..11, "# Заголовок", &[]);
        // блок неактивен: `# ` скрыт, видно только текст
        assert_eq!(decor.pieces.len(), 1);
        assert_eq!(decor.pieces[0].text, "Заголовок");
    }

    #[test]
    fn heading_marker_shown_when_active() {
        let text = "# Заголовок\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..11, "# Заголовок", &[0]);
        assert_eq!(decor.pieces[0].text, "# ");
        assert_eq!(decor.pieces.len(), 2);
    }

    #[test]
    fn bold_markers_hidden_when_inactive() {
        let text = "текст **жирный** конец\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..22, "текст **жирный** конец", &[]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "текст жирный конец");
    }

    #[test]
    fn bold_markers_shown_when_active() {
        let text = "текст **жирный** конец\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..22, "текст **жирный** конец", &[0]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "текст **жирный** конец");
    }

    #[test]
    fn wikilink_shows_display_when_inactive() {
        let text = "[[Alpha|Страница]]\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..18, "[[Alpha|Страница]]", &[]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "Страница");
    }

    #[test]
    fn rule_becomes_line() {
        let text = "---\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..3, "---", &[]);
        assert_eq!(decor.quads, vec![LineQuad::Rule]);
        assert!(decor.pieces.iter().all(|p| p.text.is_empty()));
    }

    #[test]
    fn quote_marker_replaced_with_spaces() {
        let text = "> цитата\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..8, "> цитата", &[]);
        assert!(decor.quads.contains(&LineQuad::QuoteBar));
        assert_eq!(decor.pieces[0].text, "  ");
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "  цитата");
    }

    #[test]
    fn second_line_offsets_are_local() {
        // регрессия: строки после первой имеют ненулевую глобальную базу
        let text = "первая строка\n# Заголовок\n";
        let parsed = dunite_markdown::parse(text);
        assert_eq!(parsed.blocks.len(), 2);

        // курсор в другом блоке: маркер скрыт
        let decor = decorate_line(&parsed, 14..25, "# Заголовок", &[]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "Заголовок");

        // курсор в этом блоке: маркер виден
        let decor = decorate_line(&parsed, 14..25, "# Заголовок", &[1]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "# Заголовок");
    }

    #[test]
    fn inline_image_embed_becomes_blanked_chip() {
        // inline-картинка: синтаксис заменяется плашкой фиксированной ширины
        // (миниатюру рисует рендер), текст вокруг сохраняется
        let text = "Картинка: ![[logo.png]] тут\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..27, "Картинка: ![[logo.png]] тут", &[]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            visible,
            format!("Картинка: {} тут", " ".repeat(IMAGE_CHIP_WIDTH))
        );
        // строка с миниатюрой выше обычной — картинка вписывается в неё
        assert_eq!(decor.height, IMAGE_LINE_HEIGHT);
    }

    #[test]
    fn standalone_image_keeps_chip_label() {
        let text = "![[logo.png]]\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..13, "![[logo.png]]", &[]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "logo.png");
    }

    #[test]
    fn active_block_shows_raw_image_syntax() {
        let text = "картинка ![[logo.png]]\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..23, "картинка ![[logo.png]]", &[0]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "картинка ![[logo.png]]");
    }

    #[test]
    fn control_chars_sanitized_but_positions_kept() {
        // U+0085 — bidi-разделитель абзацев: на смешанных строках cosmic-text
        // паникует; символ заменяется пробелом с сохранением числа символов
        let text = "до\u{85}после\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 0..8, "до\u{85}после", &[]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "до после");
        let total_chars: usize = decor.pieces.iter().map(|p| p.text.chars().count()).sum();
        assert_eq!(total_chars, 8);
    }

    #[test]
    fn second_line_plain_text_intact() {
        let text = "aaaa\nbbbb cccc\n";
        let parsed = dunite_markdown::parse(text);
        let decor = decorate_line(&parsed, 5..14, "bbbb cccc", &[0]);
        let visible: String = decor.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(visible, "bbbb cccc");
    }
}
