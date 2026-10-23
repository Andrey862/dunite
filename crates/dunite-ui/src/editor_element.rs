//! Элемент редактора (SPEC §8.2/§8.4): рендер строк с декорациями Live Preview.
//!
//! Виртуализация по видимым строкам, переменная высота (заголовки, hr),
//! сопоставление «видимый текст ↔ исходник» через [`PieceGeom`] для кликов
//! и курсора. Геометрия сохраняется в `Workspace::paint_state`.

use std::mem;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, Bounds, Element, ElementId, Entity, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PaintQuad, Pixels, ShapedLine, StrikethroughStyle, Style, TextRun, UnderlineStyle,
    Window, fill, point, px, relative, size,
};

use crate::decor::{self, LineQuad, SegStyle, StyleColor};
use crate::workspace::Workspace;

pub const GUTTER_WIDTH_PX: f32 = 56.;
pub const TEXT_PADDING_PX: f32 = 8.;

/// Геометрия кусочка строки: видимый текст ↔ исходные символы.
#[derive(Debug, Clone)]
pub struct PieceGeom {
    pub vis: Range<usize>,
    pub char: Range<usize>,
}

/// Геометрия видимой строки.
pub struct RowGeom {
    pub line_ix: usize,
    pub vis_text: String,
    pub shaped: ShapedLine,
    pub pieces: Vec<PieceGeom>,
    pub y: Pixels,
    pub height: Pixels,
    /// Char-диапазон содержимого строки в исходнике.
    pub line_chars: Range<usize>,
    /// Inline-картинки строки: (char-диапазон embed, цель) — поверх плашки
    /// рисуется миниатюра (SPEC §8.2).
    pub inline_images: Vec<(Range<usize>, String)>,
    pub quads: Vec<QuadGeom>,
}

/// Декорация строки, рисуемая квадом.
#[derive(Debug, Clone)]
pub struct QuadGeom {
    pub kind: LineQuad,
    pub bounds: Bounds<Pixels>,
}

impl RowGeom {
    /// Видимое byte-смещение для char-смещения исходника (clamped к границе).
    pub fn vis_byte_for_char(&self, char_off: usize) -> usize {
        vis_byte_for(&self.pieces, &self.vis_text, char_off)
    }

    /// Char-смещение исходника для видимого byte-смещения.
    pub fn char_for_vis_byte(&self, byte: usize) -> Option<usize> {
        char_for_vis(&self.pieces, &self.vis_text, byte, &self.line_chars)
    }
}

/// Видимое byte-смещение для char-смещения исходника.
pub fn vis_byte_for(pieces: &[PieceGeom], vis_text: &str, char_off: usize) -> usize {
    for piece in pieces {
        if char_off < piece.char.start {
            return piece.vis.start;
        }
        if char_off < piece.char.end {
            let col = char_off - piece.char.start;
            let byte = byte_of_col(&vis_text[piece.vis.clone()], col);
            return piece.vis.start + byte;
        }
    }
    pieces.last().map(|piece| piece.vis.end).unwrap_or(0)
}

/// Char-смещение исходника для видимого byte-смещения.
///
/// Клики мимо видимого текста (за его пределами) попадают на границы
/// СТРОКИ, а не ближайшего куска: иначе пустое поле правее ссылки
/// попадает в её диапазон (последний кусок может быть скрыт).
pub fn char_for_vis(
    pieces: &[PieceGeom],
    vis_text: &str,
    byte: usize,
    line_chars: &Range<usize>,
) -> Option<usize> {
    if pieces.is_empty() {
        return Some(line_chars.start);
    }
    for piece in pieces {
        if byte < piece.vis.start {
            return Some(piece.char.start);
        }
        if byte < piece.vis.end {
            let col = col_of_byte(&vis_text[piece.vis.clone()], byte - piece.vis.start);
            return Some(piece.char.start + col);
        }
    }
    Some(line_chars.end)
}

/// Геометрия последнего рендера панели — используется обработчиками мыши.
pub struct PaintState {
    pub bounds: Bounds<Pixels>,
    pub scroll_px: f32,
    pub gutter_width: Pixels,
    /// Вертикальные позиции строк (общие для документа).
    pub tops: Rc<Vec<f32>>,
    pub rows: Vec<RowGeom>,
}

impl PaintState {
    /// X начала текста (правее колонки нумерации).
    pub fn text_left(&self) -> Pixels {
        self.bounds.left() + self.gutter_width + px(TEXT_PADDING_PX)
    }

    /// Строка под Y-координатой (clamped).
    pub fn line_for_y(&self, y: Pixels) -> usize {
        let content_y = y.to_f64() as f32 - self.bounds.top().to_f64() as f32 + self.scroll_px;
        let mut lo = 0usize;
        let mut hi = self.tops.len().saturating_sub(1);
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if self.tops[mid] <= content_y {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo.min(self.tops.len().saturating_sub(2))
    }

    /// Высота вьюпорта в пикселях (f32).
    pub fn viewport_height(&self) -> f32 {
        self.bounds.size.height.to_f64() as f32
    }

    /// Строка под точкой (для кликов).
    pub fn row_at(&self, line_ix: usize) -> Option<&RowGeom> {
        self.rows.iter().find(|row| row.line_ix == line_ix)
    }
}

/// Элемент панели редактора.
pub struct EditorElement {
    pub workspace: Entity<Workspace>,
}

pub struct EditorPrepaintState {
    rows: Vec<RowPaint>,
    selections: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    quads: Vec<QuadGeom>,
}

struct RowPaint {
    geom: RowGeom,
    gutter_shaped: ShapedLine,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // раскладка (парсинг markdown + позиции строк) — кэшируется в Workspace;
        // заодно запрашиваем фоновую загрузку картинок (SPEC §7.5)
        let pane_width = bounds.size.width.to_f64() as f32;
        self.workspace.update(cx, |workspace, cx| {
            workspace.ensure_layout(pane_width);
            let missing: Vec<String> = workspace
                .layout
                .as_ref()
                .map(|layout| {
                    let mut targets: Vec<String> = layout
                        .images
                        .iter()
                        .map(|(_, target)| target.clone())
                        .collect();
                    // inline-миниатюры тоже догружаем
                    if let Some(tab) = workspace.tabs.get(workspace.active_tab) {
                        let buffer = tab.editor.buffer();
                        for block in &layout.parsed.blocks {
                            for inline in &block.inlines {
                                if let Some(target_range) = crate::decor::embed_target_range(inline)
                                {
                                    let target = buffer.slice(target_range);
                                    if crate::decor::looks_like_image(&target) {
                                        targets.push(target);
                                    }
                                }
                            }
                        }
                    }
                    targets.sort();
                    targets.dedup();
                    targets.retain(|target| !workspace.images.contains_key(target));
                    targets
                })
                .unwrap_or_default();
            for target in missing {
                workspace.load_missing_image(target, cx);
            }
        });

        let workspace = self.workspace.read(cx);
        let palette = workspace.palette;
        let text_style = window.text_style();
        let base_font = text_style.font();
        let gutter_width = px(GUTTER_WIDTH_PX);

        let mut rows = Vec::new();
        let mut selections = Vec::new();
        let mut quads = Vec::new();
        let mut cursor = None;

        let Some(layout) = workspace.layout.as_ref() else {
            return EditorPrepaintState {
                rows,
                selections,
                cursor,
                quads,
            };
        };
        let Some(tab) = workspace.tabs.get(workspace.active_tab) else {
            return EditorPrepaintState {
                rows,
                selections,
                cursor,
                quads,
            };
        };
        let editor = &tab.editor;
        let buffer = editor.buffer();
        let total_lines = buffer.len_lines();
        let tops = layout.tops.clone();
        let scroll_px = workspace.scroll_px;

        // раскрытие маркеров (SPEC §8.2): блок с курсором и блоки выделения.
        // При мышином выделении раскрытие — только после отпускания кнопки,
        // чтобы текст не прыгал в процессе выделения.
        let selection = editor.selection().range();
        let mut revealed: Vec<usize> = Vec::new();
        if !workspace.selecting {
            if let Some(ix) = workspace.active_block_index() {
                revealed.push(ix);
            }
            if !selection.is_empty() {
                for (ix, block) in layout.parsed.blocks.iter().enumerate() {
                    if block.range.start < selection.end && selection.start < block.range.end {
                        revealed.push(ix);
                    }
                }
            }
        }

        // видимые строки
        let safe_total = total_lines.min(tops.len().saturating_sub(1));
        let first_line = {
            let mut lo = 0;
            let mut hi = safe_total;
            let target = scroll_px;
            while lo < hi {
                let mid = (lo + hi) / 2;
                if tops[mid + 1] <= target {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            lo
        };
        let mut last_line = first_line;
        let view_bottom = scroll_px + bounds.size.height.to_f64() as f32;
        while last_line < safe_total && tops[last_line] < view_bottom {
            last_line += 1;
        }
        let last_line = (last_line + 1).min(safe_total);

        for line_ix in first_line..last_line {
            let content = buffer.line_content_range(line_ix);
            let line_text = buffer.line(line_ix);
            let decor =
                decor::decorate_line(&layout.parsed, content.clone(), &line_text, &revealed);

            let y = px(tops.get(line_ix).copied().unwrap_or(0.) - scroll_px);
            let height = px(decor.height);

            // собираем видимый текст и куски
            let mut vis_text = String::new();
            let mut pieces = Vec::new();
            let mut runs = Vec::new();
            for piece in &decor.pieces {
                let vis_start = vis_text.len();
                vis_text.push_str(&piece.text);
                let vis = vis_start..vis_text.len();
                if !piece.text.is_empty() {
                    runs.push(style_run(
                        &piece.style,
                        piece.text.len(),
                        &base_font,
                        palette,
                    ));
                }
                pieces.push(PieceGeom {
                    vis,
                    char: piece.char.clone(),
                });
            }

            let shaped = window.text_system().shape_line(
                vis_text.clone().into(),
                px(decor.font_size),
                &runs,
                None,
            );

            // декорации-квады
            let text_left = bounds.left() + gutter_width + px(TEXT_PADDING_PX);
            for quad in &decor.quads {
                let quad_bounds = match quad {
                    LineQuad::Rule => Bounds::new(
                        point(text_left, bounds.top() + y + height / 2.),
                        size(bounds.size.width - gutter_width - px(32.), px(1.)),
                    ),
                    LineQuad::QuoteBar => Bounds::new(
                        point(text_left - px(6.), bounds.top() + y),
                        size(px(2.), height),
                    ),
                    LineQuad::CodeBg | LineQuad::FrontmatterBg => Bounds::new(
                        point(bounds.left() + gutter_width, bounds.top() + y),
                        size(bounds.size.width - gutter_width, height),
                    ),
                };
                quads.push(QuadGeom {
                    kind: quad.clone(),
                    bounds: quad_bounds,
                });
            }

            // выделение
            let sel_start = selection.start.max(content.start);
            let sel_end = selection.end.min(content.end);
            if sel_start < sel_end {
                let from = vis_byte_for(&pieces, &vis_text, sel_start);
                let to = vis_byte_for(&pieces, &vis_text, sel_end);
                if from < to {
                    selections.push(fill(
                        Bounds::from_corners(
                            point(text_left + shaped.x_for_index(from), bounds.top() + y),
                            point(
                                text_left + shaped.x_for_index(to),
                                bounds.top() + y + height,
                            ),
                        ),
                        palette.selection,
                    ));
                }
            }

            // курсор
            let head = editor.selection().head;
            if head >= content.start && head <= content.end {
                let x = text_left + shaped.x_for_index(vis_byte_for(&pieces, &vis_text, head));
                cursor = Some(fill(
                    Bounds::new(point(x, bounds.top() + y), size(px(2.), height)),
                    palette.cursor,
                ));
            }

            // inline-миниатюры (SPEC §8.2): standalone-строки рисуются
            // крупной картинкой ниже — их пропускаем
            let mut inline_images = Vec::new();
            for block in &layout.parsed.blocks {
                if block.range.end <= content.start || block.range.start >= content.end {
                    continue;
                }
                for inline in &block.inlines {
                    if !crate::decor::is_image_embed(inline) {
                        continue;
                    }
                    let range = inline.range();
                    if range.start >= content.end || range.end <= content.start {
                        continue;
                    }
                    let Some(target_range) = crate::decor::embed_target_range(inline) else {
                        continue;
                    };
                    let target = buffer.slice(target_range);
                    let standalone = layout
                        .images
                        .iter()
                        .any(|(line, t)| *line == line_ix && *t == target);
                    if crate::decor::looks_like_image(&target) && !standalone {
                        inline_images.push((range, target));
                    }
                }
            }

            let number = format!("{}", line_ix + 1);
            let gutter_run = TextRun {
                len: number.len(),
                font: base_font.clone(),
                color: palette.text_muted,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let gutter_shaped =
                window
                    .text_system()
                    .shape_line(number.into(), px(11.), &[gutter_run], None);

            rows.push(RowPaint {
                geom: RowGeom {
                    line_ix,
                    vis_text,
                    shaped,
                    pieces,
                    y,
                    height,
                    line_chars: content,
                    inline_images,
                    quads: Vec::new(),
                },
                gutter_shaped,
            });
        }

        EditorPrepaintState {
            rows,
            selections,
            cursor,
            quads,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let palette = self.workspace.read(cx).palette;
        let gutter_width = px(GUTTER_WIDTH_PX);
        let text_left = bounds.left() + gutter_width + px(TEXT_PADDING_PX);
        let avail_width =
            (bounds.size.width - gutter_width - px(2. * TEXT_PADDING_PX)).to_f64() as f32;
        let image_targets: std::collections::HashMap<usize, String> = self
            .workspace
            .read(cx)
            .layout
            .as_ref()
            .map(|layout| layout.images.iter().cloned().collect())
            .unwrap_or_default();

        let rows = mem::take(&mut prepaint.rows);

        // весь рендер клиппится по границам панели: без маски строки (номера,
        // текст, картинки) вылезают на таб-бар и статус-бар (SPEC §8.4)
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            // фон колонки нумерации
            window.paint_quad(fill(
                Bounds::new(bounds.origin, size(gutter_width, bounds.size.height)),
                palette.gutter,
            ));

            // фоновые декорации (code-блоки, frontmatter, hr, полосы цитат)
            for quad in mem::take(&mut prepaint.quads) {
                let color = match quad.kind {
                    LineQuad::Rule => palette.border,
                    LineQuad::QuoteBar => palette.text_muted,
                    LineQuad::CodeBg => palette.panel,
                    LineQuad::FrontmatterBg => palette.hover,
                };
                window.paint_quad(fill(quad.bounds, color));
            }

            for quad in mem::take(&mut prepaint.selections) {
                window.paint_quad(quad);
            }

            for row in &rows {
                let origin = point(bounds.left(), bounds.top() + row.geom.y);
                let center = (row.geom.height.to_f64() as f32 - 16.) / 2.;
                row.gutter_shaped
                    .paint(
                        point(
                            origin.x + px(TEXT_PADDING_PX),
                            origin.y + px(center.max(0.)),
                        ),
                        px(16.),
                        window,
                        cx,
                    )
                    .ok();
                row.geom
                    .shaped
                    .paint(
                        point(origin.x + gutter_width + px(TEXT_PADDING_PX), origin.y),
                        row.geom.height,
                        window,
                        cx,
                    )
                    .ok();

                // inline-миниатюры поверх плашки (SPEC §8.2)
                for (range, target) in &row.geom.inline_images {
                    let workspace = self.workspace.read(cx);
                    let from = vis_byte_for(&row.geom.pieces, &row.geom.vis_text, range.start);
                    let to = vis_byte_for(&row.geom.pieces, &row.geom.vis_text, range.end);
                    let x = text_left + row.geom.shaped.x_for_index(from);
                    let x_end = text_left + row.geom.shaped.x_for_index(to);
                    let avail_w = ((x_end - x).to_f64() as f32).max(8.);
                    let max_h = (row.geom.height.to_f64() as f32 - 4.).max(8.);
                    if let Some(crate::workspace::ImageState::Loaded {
                        render,
                        width,
                        height,
                    }) = workspace.images.get(target)
                    {
                        let (image_w, image_h) =
                            crate::images::fit_size(*width, *height, avail_w, max_h);
                        let image_y =
                            origin.y + px((row.geom.height.to_f64() as f32 - image_h) / 2.);
                        window
                            .paint_image(
                                Bounds::new(point(x, image_y), size(px(image_w), px(image_h))),
                                (px(0.)).into(),
                                render.clone(),
                                0,
                                false,
                            )
                            .ok();
                    } else {
                        // плашка, пока изображение грузится
                        window.paint_quad(fill(
                            Bounds::new(point(x, origin.y + px(2.)), size(px(avail_w), px(max_h))),
                            palette.hover,
                        ));
                    }
                }

                // standalone-картинки строк (SPEC §8.2)
                if let Some(target) = image_targets.get(&row.geom.line_ix) {
                    let workspace = self.workspace.read(cx);
                    if let Some((image_w, image_h)) =
                        workspace.image_display_size(target, avail_width)
                        && let Some(crate::workspace::ImageState::Loaded { render, .. }) =
                            workspace.images.get(target)
                    {
                        window
                            .paint_image(
                                Bounds::new(
                                    point(text_left, origin.y + px(24.)),
                                    size(px(image_w), px(image_h)),
                                ),
                                (px(0.)).into(),
                                render.clone(),
                                0,
                                false,
                            )
                            .ok();
                    }
                }
            }

            if self.workspace.read(cx).focus_handle.is_focused(window) {
                if let Some(cursor) = prepaint.cursor.take() {
                    window.paint_quad(cursor);
                }
            }
        });

        // сохраняем геометрию для hit-testing мышью
        let geom_rows: Vec<RowGeom> = rows.into_iter().map(|row| row.geom).collect();
        self.workspace.update(cx, |workspace, _| {
            let tops = workspace
                .layout
                .as_ref()
                .map(|layout| layout.tops.clone())
                .unwrap_or_default();
            workspace.paint_state = Some(PaintState {
                bounds,
                scroll_px: workspace.scroll_px,
                gutter_width,
                tops,
                rows: geom_rows,
            });
        });

        // IME-ввод (SPEC §8.3)
        let focus_handle = self.workspace.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            gpui::ElementInputHandler::new(bounds, self.workspace.clone()),
            cx,
        );
    }
}

/// TextRun для кусочка стиля.
fn style_run(
    style: &SegStyle,
    len: usize,
    base_font: &gpui::Font,
    palette: crate::theme::Palette,
) -> TextRun {
    let color = match style.color {
        StyleColor::Default => palette.text,
        StyleColor::Muted => palette.text_muted,
        StyleColor::Accent => palette.accent,
        StyleColor::Heading => palette.text,
    };
    let font = gpui::Font {
        weight: if style.bold {
            gpui::FontWeight::BOLD
        } else {
            gpui::FontWeight::NORMAL
        },
        style: if style.italic {
            gpui::FontStyle::Italic
        } else {
            gpui::FontStyle::Normal
        },
        ..base_font.clone()
    };
    TextRun {
        len,
        font,
        color,
        background_color: style.code_bg.then_some(palette.hover),
        underline: style.underline.then_some(UnderlineStyle {
            color: Some(color),
            thickness: px(1.),
            wavy: false,
        }),
        strikethrough: style.strike.then_some(StrikethroughStyle {
            thickness: px(1.),
            color: Some(color),
        }),
    }
}

/// Byte-смещение char-колонки внутри строки.
pub fn byte_of_col(line_text: &str, col: usize) -> usize {
    line_text
        .char_indices()
        .nth(col)
        .map(|(byte_ix, _)| byte_ix)
        .unwrap_or(line_text.len())
}

/// Char-колонка byte-смещения внутри строки.
pub fn col_of_byte(line_text: &str, byte_ix: usize) -> usize {
    let byte_ix = byte_ix.min(line_text.len());
    line_text
        .get(..byte_ix)
        .unwrap_or(line_text)
        .chars()
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(vis: Range<usize>, char: Range<usize>) -> PieceGeom {
        PieceGeom { vis, char }
    }

    #[test]
    fn click_beyond_text_maps_to_line_end_not_link() {
        // строка "link: [[Alpha]]" (15 символов): видимо «link: Alpha»;
        // клик в пустое поле правее текста не должен попадать в ссылку 6..15
        let pieces = vec![
            piece(0..6, 0..6),   // "link: "
            piece(6..11, 8..13), // "Alpha" — внутри скрытых [[ ]]
        ];
        let vis_text = "link: Alpha";
        let line_chars = 0..15;

        // правее всего → конец строки: 15 не входит в ссылку 6..15
        assert_eq!(
            char_for_vis(&pieces, vis_text, vis_text.len(), &line_chars),
            Some(15)
        );
        // левее всего → начало строки
        assert_eq!(char_for_vis(&pieces, vis_text, 0, &line_chars), Some(0));
        // клик по видимой «Alpha» → позиция внутри ссылки (ожидаемо)
        assert_eq!(char_for_vis(&pieces, vis_text, 7, &line_chars), Some(9));
    }

    #[test]
    fn click_on_empty_line_maps_to_line_start() {
        assert_eq!(char_for_vis(&[], "", 0, &(0..0)), Some(0));
        assert_eq!(char_for_vis(&[], "", 5, &(3..3)), Some(3));
    }
}
