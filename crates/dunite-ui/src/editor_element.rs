//! Элемент редактора (SPEC §8): рендер строк, нумерации, выделения, курсора.
//!
//! Собственный `gpui::Element`: виртуализация по видимым строкам, метрики
//! после рендера сохраняются в `Workspace::paint_state` для hit-testing мыши.

use std::mem;

use gpui::{
    App, Bounds, Element, ElementId, Entity, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PaintQuad, Pixels, ShapedLine, Style, TextRun, Window, fill, point, px, relative,
    size,
};

use crate::workspace::Workspace;

pub const LINE_HEIGHT_PX: f32 = 20.;
pub const GUTTER_WIDTH_PX: f32 = 56.;
pub const TEXT_PADDING_PX: f32 = 8.;

/// Геометрия одной видимой строки (для рендера и hit-testing).
pub struct RowGeom {
    pub line_ix: usize,
    pub line_text: String,
    pub shaped: ShapedLine,
    /// Верх строки относительно верха элемента.
    pub y: Pixels,
}

/// Геометрия последнего рендера панели — используется обработчиками мыши.
pub struct PaintState {
    pub bounds: Bounds<Pixels>,
    pub scroll_px: f32,
    pub line_height: Pixels,
    pub gutter_width: Pixels,
    pub rows: Vec<RowGeom>,
}

impl PaintState {
    /// X начала текста (правее колонки нумерации).
    pub fn text_left(&self) -> Pixels {
        self.bounds.left() + self.gutter_width + px(TEXT_PADDING_PX)
    }

    /// Строка под Y-координатой (clamped).
    pub fn line_for_y(&self, y: Pixels, total_lines: usize) -> usize {
        let content_y = y - self.bounds.top() + px(self.scroll_px);
        let line = (content_y / self.line_height).floor() as isize;
        line.clamp(0, total_lines.saturating_sub(1) as isize) as usize
    }

    /// Высота вьюпорта в пикселях (f32).
    pub fn viewport_height(&self) -> f32 {
        self.bounds.size.height.to_f64() as f32
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
        let workspace = self.workspace.read(cx);
        let palette = workspace.palette;
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = px(LINE_HEIGHT_PX);
        let gutter_width = px(GUTTER_WIDTH_PX);

        let mut rows = Vec::new();
        let mut selections = Vec::new();
        let mut cursor = None;

        let Some(tab) = workspace.tabs.get(workspace.active_tab) else {
            return EditorPrepaintState {
                rows,
                selections,
                cursor,
            };
        };
        let editor = &tab.editor;
        let buffer = editor.buffer();
        let total_lines = buffer.len_lines();
        let scroll_px = workspace.scroll_px;

        let first_line = (scroll_px / LINE_HEIGHT_PX).floor().max(0.) as usize;
        let visible_count =
            (bounds.size.height.to_f64() as f32 / LINE_HEIGHT_PX).ceil() as usize + 1;
        let last_line = (first_line + visible_count).min(total_lines);

        let selection = editor.selection().range();

        for line_ix in first_line..last_line {
            let line_text = buffer.line(line_ix);
            let y = px(line_ix as f32 * LINE_HEIGHT_PX - scroll_px);

            let run = TextRun {
                len: line_text.len(),
                font: text_style.font(),
                color: palette.text,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped =
                window
                    .text_system()
                    .shape_line(line_text.clone().into(), font_size, &[run], None);

            let number = format!("{}", line_ix + 1);
            let gutter_run = TextRun {
                len: number.len(),
                font: text_style.font(),
                color: palette.text_muted,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let gutter_shaped =
                window
                    .text_system()
                    .shape_line(number.into(), font_size, &[gutter_run], None);

            // выделение: пересечение выделения с содержимым строки
            let content = buffer.line_content_range(line_ix);
            let sel_start = selection.start.max(content.start);
            let sel_end = selection.end.min(content.end);
            if sel_start < sel_end {
                let from = byte_of_col(&line_text, sel_start - content.start);
                let to = byte_of_col(&line_text, sel_end - content.start);
                let quad = fill(
                    Bounds::from_corners(
                        point(
                            bounds.left()
                                + gutter_width
                                + px(TEXT_PADDING_PX)
                                + shaped.x_for_index(from),
                            bounds.top() + y,
                        ),
                        point(
                            bounds.left()
                                + gutter_width
                                + px(TEXT_PADDING_PX)
                                + shaped.x_for_index(to),
                            bounds.top() + y + line_height,
                        ),
                    ),
                    palette.selection,
                );
                selections.push(quad);
            }

            // курсор
            let head = editor.selection().head;
            if head >= content.start && head <= content.end {
                let col = (head - content.start).min(line_text.chars().count());
                let x = shaped.x_for_index(byte_of_col(&line_text, col));
                cursor = Some(fill(
                    Bounds::new(
                        point(
                            bounds.left() + gutter_width + px(TEXT_PADDING_PX) + x,
                            bounds.top() + y,
                        ),
                        size(px(2.), line_height),
                    ),
                    palette.cursor,
                ));
            }

            rows.push(RowPaint {
                geom: RowGeom {
                    line_ix,
                    line_text,
                    shaped,
                    y,
                },
                gutter_shaped,
            });
        }

        EditorPrepaintState {
            rows,
            selections,
            cursor,
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
        let line_height = px(LINE_HEIGHT_PX);
        let gutter_width = px(GUTTER_WIDTH_PX);

        // фон колонки нумерации
        window.paint_quad(fill(
            Bounds::new(bounds.origin, size(gutter_width, bounds.size.height)),
            palette.gutter,
        ));

        for quad in mem::take(&mut prepaint.selections) {
            window.paint_quad(quad);
        }

        let rows = mem::take(&mut prepaint.rows);
        for row in &rows {
            let origin = point(bounds.left(), bounds.top() + row.geom.y);
            row.gutter_shaped
                .paint(
                    point(origin.x + px(TEXT_PADDING_PX), origin.y),
                    line_height,
                    window,
                    cx,
                )
                .ok();
            row.geom
                .shaped
                .paint(
                    point(origin.x + gutter_width + px(TEXT_PADDING_PX), origin.y),
                    line_height,
                    window,
                    cx,
                )
                .ok();
        }

        if self.workspace.read(cx).focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        // сохраняем геометрию для hit-testing мышью
        let geom_rows: Vec<RowGeom> = rows.into_iter().map(|row| row.geom).collect();
        self.workspace.update(cx, |workspace, _| {
            workspace.paint_state = Some(PaintState {
                bounds,
                scroll_px: workspace.scroll_px,
                line_height,
                gutter_width,
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
    line_text[..byte_ix].chars().count()
}
