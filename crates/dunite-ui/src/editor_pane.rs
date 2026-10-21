//! Панель редактора (SPEC §8.3): ввод, действия, мышь, скролл, автосейв.

use std::ops::Range;
use std::time::{Duration, Instant};

use dunite_editor::{Editor, Movement};
use gpui::{
    ClipboardItem, Context, EntityInputHandler, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, UTF16Selection, Window, div,
    prelude::*, px,
};

use crate::actions::*;
use crate::decor::DEFAULT_HEIGHT;
use crate::editor_element::{EditorElement, PaintState};
use crate::workspace::Workspace;

/// Клик: распознавание двойного/тройного нажатия.
const CLICK_INTERVAL: Duration = Duration::from_millis(400);
const CLICK_SLOP_F64: f64 = 4.;
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub(crate) struct ClickTracker {
    pub count: usize,
    pub position: Point<Pixels>,
    pub time: Instant,
}

impl Default for ClickTracker {
    fn default() -> Self {
        Self {
            count: 0,
            position: Point::new(px(0.), px(0.)),
            time: Instant::now(),
        }
    }
}

impl Workspace {
    /// Панель редактора: контейнер с действиями/мышью + [`EditorElement`].
    pub(crate) fn render_editor_pane(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .key_context("editor")
            .track_focus(&self.focus_handle)
            .cursor(gpui::CursorStyle::IBeam)
            .size_full()
            .font_family("monospace")
            .text_size(px(13.))
            // движения курсора
            .on_action(
                cx.listener(|this, _: &CursorLeft, _, cx| {
                    this.act_cursor(Movement::Left, false, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &CursorRight, _, cx| {
                this.act_cursor(Movement::Right, false, cx)
            }))
            .on_action(
                cx.listener(|this, _: &CursorUp, _, cx| this.act_cursor(Movement::Up, false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CursorDown, _, cx| {
                    this.act_cursor(Movement::Down, false, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &CursorWordLeft, _, cx| {
                this.act_cursor(Movement::WordLeft, false, cx)
            }))
            .on_action(cx.listener(|this, _: &CursorWordRight, _, cx| {
                this.act_cursor(Movement::WordRight, false, cx)
            }))
            .on_action(cx.listener(|this, _: &CursorLineStart, _, cx| {
                this.act_cursor(Movement::LineStart, false, cx)
            }))
            .on_action(cx.listener(|this, _: &CursorLineEnd, _, cx| {
                this.act_cursor(Movement::LineEnd, false, cx)
            }))
            .on_action(cx.listener(|this, _: &CursorPageUp, _, cx| {
                this.act_cursor(
                    Movement::PageUp {
                        lines: page_lines(),
                    },
                    false,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &CursorPageDown, _, cx| {
                this.act_cursor(
                    Movement::PageDown {
                        lines: page_lines(),
                    },
                    false,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &CursorDocStart, _, cx| {
                this.act_cursor(Movement::DocStart, false, cx)
            }))
            .on_action(cx.listener(|this, _: &CursorDocEnd, _, cx| {
                this.act_cursor(Movement::DocEnd, false, cx)
            }))
            // выделение
            .on_action(
                cx.listener(|this, _: &SelectLeft, _, cx| {
                    this.act_cursor(Movement::Left, true, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
                this.act_cursor(Movement::Right, true, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectUp, _, cx| this.act_cursor(Movement::Up, true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectDown, _, cx| {
                    this.act_cursor(Movement::Down, true, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| {
                this.act_cursor(Movement::WordLeft, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| {
                this.act_cursor(Movement::WordRight, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| {
                this.act_cursor(Movement::LineStart, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| {
                this.act_cursor(Movement::LineEnd, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                this.act_cursor(
                    Movement::PageUp {
                        lines: page_lines(),
                    },
                    true,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                this.act_cursor(
                    Movement::PageDown {
                        lines: page_lines(),
                    },
                    true,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &SelectDocStart, _, cx| {
                this.act_cursor(Movement::DocStart, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectDocEnd, _, cx| {
                this.act_cursor(Movement::DocEnd, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.select_all();
                    cx.notify();
                })
            }))
            .on_action(cx.listener(|this, _: &CancelSelection, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.collapse_to_head();
                    cx.notify();
                })
            }))
            // правки
            .on_action(cx.listener(|this, _: &DeleteBackward, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.delete_backward();
                    cx.notify();
                });
                this.after_edit(cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteForward, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.delete_forward();
                    cx.notify();
                });
                this.after_edit(cx);
            }))
            .on_action(cx.listener(|this, _: &InsertNewline, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.insert_newline();
                    cx.notify();
                });
                this.after_edit(cx);
            }))
            .on_action(cx.listener(|this, _: &InsertTab, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.insert_tab();
                    cx.notify();
                });
                this.after_edit(cx);
            }))
            .on_action(cx.listener(|this, _: &Undo, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.undo();
                    cx.notify();
                });
                this.after_edit(cx);
            }))
            .on_action(cx.listener(|this, _: &Redo, _, cx| {
                this.with_editor(cx, |editor, cx| {
                    editor.redo();
                    cx.notify();
                });
                this.after_edit(cx);
            }))
            // буфер обмена
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.act_copy(cx)))
            .on_action(cx.listener(|this, _: &Cut, _, cx| this.act_cut(cx)))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.act_paste(cx)))
            // мышь и скролл
            .on_mouse_down(MouseButton::Left, cx.listener(Self::editor_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::editor_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::editor_mouse_up))
            .on_mouse_move(cx.listener(Self::editor_mouse_move))
            .on_scroll_wheel(cx.listener(Self::editor_scroll))
            .child(EditorElement {
                workspace: cx.entity(),
            })
    }

    /// Выполнить мутацию активного редактора.
    pub(crate) fn with_editor(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Editor, &mut Context<Self>),
    ) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            f(&mut tab.editor, cx);
        }
    }

    /// Движение курсора с последующим доскроллом.
    pub(crate) fn act_cursor(&mut self, movement: Movement, extend: bool, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor, _| editor.move_cursor(movement, extend));
        self.scroll_cursor_into_view();
        cx.notify();
    }

    pub(crate) fn act_copy(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        let text = tab.editor.selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(crate) fn act_cut(&mut self, cx: &mut Context<Self>) {
        self.act_copy(cx);
        let cut = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| !tab.editor.selection().is_empty());
        if cut {
            self.with_editor(cx, |editor, _| {
                editor.delete_backward();
            });
            self.after_edit(cx);
        }
    }

    pub(crate) fn act_paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.with_editor(cx, |editor, _| editor.insert_pasted_text(&text));
        self.after_edit(cx);
    }

    // ── мышь ─────────────────────────────────────────────────────────────

    pub(crate) fn editor_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(offset) = self.offset_for_position(event.position) else {
            return;
        };

        // клик по чекбоксу — переключить (SPEC §8.2)
        if let Some(range) = self.checkbox_at(offset) {
            self.toggle_checkbox(range, cx);
            return;
        }
        // клик по ссылке: в неактивном блоке или с Ctrl — переход (SPEC §8.2)
        if let Some(target) = self.link_target_at(offset) {
            if event.modifiers.control || !self.block_active_at(offset) {
                self.open_link(&target, cx);
                return;
            }
        }

        let now = Instant::now();
        let same_spot = (event.position - self.click.position).magnitude() < CLICK_SLOP_F64
            && now.duration_since(self.click.time) < CLICK_INTERVAL;
        self.click.count = if same_spot {
            self.click.count % 3 + 1
        } else {
            1
        };
        self.click.position = event.position;
        self.click.time = now;

        match self.click.count {
            2 => self.with_editor(cx, |editor, _| editor.select_word_at(offset)),
            3 => self.with_editor(cx, |editor, _| editor.select_line_at(offset)),
            _ => self.with_editor(cx, |editor, _| {
                editor.set_cursor(offset, event.modifiers.shift);
            }),
        }
        self.selecting = true;
        cx.notify();
    }

    pub(crate) fn editor_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selecting = false;
        cx.notify();
    }

    pub(crate) fn editor_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selecting {
            return;
        }
        if let Some(offset) = self.offset_for_position(event.position) {
            self.with_editor(cx, |editor, _| editor.set_cursor(offset, true));
            self.scroll_cursor_into_view();
            cx.notify();
        }
    }

    pub(crate) fn editor_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta_y = match event.delta {
            ScrollDelta::Pixels(pixels) => pixels.y.to_f64() as f32,
            ScrollDelta::Lines(lines) => lines.y * DEFAULT_HEIGHT * 3.,
        };
        self.scroll_px -= delta_y;
        self.clamp_scroll();
        cx.notify();
    }

    /// Смещение (char) под точкой окна.
    pub(crate) fn offset_for_position(&self, position: Point<Pixels>) -> Option<usize> {
        let paint = self.paint_state.as_ref()?;
        let tab = self.tabs.get(self.active_tab)?;
        let buffer = tab.editor.buffer();
        let line_ix = paint.line_for_y(position.y);
        let content = buffer.line_content_range(line_ix);

        match paint.row_at(line_ix) {
            Some(row) => {
                let x = position.x - paint.text_left();
                let vis_byte = row.shaped.closest_index_for_x(x);
                Some(row.char_for_vis_byte(vis_byte).unwrap_or(content.start))
            }
            None => {
                // строка за пределами видимой области (drag)
                Some(if position.x < paint.text_left() {
                    content.start
                } else {
                    content.end
                })
            }
        }
    }

    fn clamp_scroll(&mut self) {
        let viewport_h = self
            .paint_state
            .as_ref()
            .map(|paint| paint.viewport_height())
            .unwrap_or(0.);
        let total_h = self
            .layout
            .as_ref()
            .map(|layout| layout.tops.last().copied().unwrap_or(0.))
            .unwrap_or(0.);
        let max_scroll = (total_h - viewport_h).max(0.);
        self.scroll_px = self.scroll_px.clamp(0., max_scroll);
    }

    /// Доскроллить так, чтобы курсор был виден.
    pub(crate) fn scroll_cursor_into_view(&mut self) {
        self.ensure_layout();
        let viewport_h = self
            .paint_state
            .as_ref()
            .map(|paint| paint.viewport_height())
            .unwrap_or(0.);
        let Some(layout) = self.layout.as_ref() else {
            return;
        };
        let line = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.editor.cursor_line())
            .unwrap_or(0);
        let top = layout.tops.get(line).copied().unwrap_or(0.);
        let height = layout.heights.get(line).copied().unwrap_or(20.);
        if top < self.scroll_px {
            self.scroll_px = top;
        } else if top + height > self.scroll_px + viewport_h {
            self.scroll_px = top + height - viewport_h;
        }
        self.clamp_scroll();
    }

    // ── кликабельные элементы (SPEC §8.2) ─────────────────────────────────

    /// Диапазон чекбокса `[x]` под смещением.
    fn checkbox_at(&self, offset: usize) -> Option<std::ops::Range<usize>> {
        let layout = self.layout.as_ref()?;
        for block in &layout.parsed.blocks {
            if let dunite_markdown::BlockKind::ListItem {
                checkbox: Some(checkbox),
                ..
            } = &block.kind
            {
                if checkbox.range.start <= offset && offset < checkbox.range.end {
                    return Some(checkbox.range.clone());
                }
            }
        }
        None
    }

    /// Цель ссылки под смещением (`[[target]]` / `[текст](target)`).
    fn link_target_at(&self, offset: usize) -> Option<String> {
        let layout = self.layout.as_ref()?;
        let tab = self.tabs.get(self.active_tab)?;
        for block in &layout.parsed.blocks {
            for inline in &block.inlines {
                let range = inline.range();
                if range.start <= offset && offset < range.end {
                    let target = match inline {
                        dunite_markdown::Inline::WikiLink {
                            target,
                            embed: false,
                            ..
                        } => target.clone(),
                        dunite_markdown::Inline::MarkdownLink {
                            target,
                            image: false,
                            ..
                        } => target.clone(),
                        _ => return None,
                    };
                    return Some(tab.editor.buffer().slice(target));
                }
            }
        }
        None
    }

    /// Находится ли смещение в активном блоке (блоке с курсором).
    fn block_active_at(&self, offset: usize) -> bool {
        let Some(layout) = self.layout.as_ref() else {
            return false;
        };
        let Some(cursor) = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.editor.selection().head)
        else {
            return false;
        };
        let block_of = |offset: usize| {
            layout
                .parsed
                .blocks
                .iter()
                .position(|block| block.range.start <= offset && offset <= block.range.end)
        };
        block_of(offset).is_some_and(|ix| Some(ix) == block_of(cursor))
    }

    /// Переход по ссылке: резолвинг цели и открытие заметки (упрощённый,
    /// полный резолвинг ссылок — M3, SPEC §7.2).
    fn open_link(&mut self, target: &str, cx: &mut Context<Self>) {
        let target = target.trim();
        let resolved = [format!("{target}.md"), target.to_string()]
            .into_iter()
            .find(|rel| self.vault.absolute(rel).is_file())
            .or_else(|| {
                self.vault
                    .find_note_by_name(target)
                    .map(|id| id.as_str().to_string())
            });
        match resolved {
            Some(rel) => self.open_note(&rel, cx),
            None => {
                self.status_message = format!("ссылка не найдена: {target}");
                cx.notify();
            }
        }
    }

    /// Переключение чекбокса `[x]` ↔ `[ ]` через модель редактора.
    fn toggle_checkbox(&mut self, range: std::ops::Range<usize>, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        let current = tab.editor.buffer().slice(range.clone());
        let new = if current.starts_with("[ ]") {
            "[x]"
        } else {
            "[ ]"
        };
        self.with_editor(cx, |editor, _| {
            editor.set_cursor(range.start, false);
            editor.set_cursor(range.end, true);
            editor.insert_text(new);
        });
        self.after_edit(cx);
    }

    // ── автосейв (SPEC §10.3) ────────────────────────────────────────────

    /// Вызвать после любой правки: перерисовка + отложенный автосейв.
    pub(crate) fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.save_generation += 1;
        let generation = self.save_generation;
        self.scroll_cursor_into_view();

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DEBOUNCE).await;
            let prepared = this
                .update(cx, |workspace, _| {
                    if workspace.save_generation != generation {
                        return None;
                    }
                    let tab = workspace.tabs.get(workspace.active_tab)?;
                    let text = tab.editor.text();
                    let path = workspace.vault.absolute(tab.note_id.as_str());
                    Some((text, path))
                })
                .ok()
                .flatten();
            let Some((text, path)) = prepared else {
                return;
            };
            let tmp = path.with_extension("dunite-tmp");
            let saved = std::fs::write(&tmp, &text).is_ok() && std::fs::rename(&tmp, &path).is_ok();
            if saved {
                this.update(cx, |workspace, cx| {
                    if workspace.save_generation == generation {
                        if let Some(tab) = workspace.tabs.get_mut(workspace.active_tab) {
                            tab.editor.mark_saved();
                        }
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();

        cx.notify();
    }

    /// Немедленно сохранить вкладку (закрытие, выход).
    pub(crate) fn save_tab_now(&mut self, ix: usize) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        if !tab.editor.is_dirty() {
            return;
        }
        let path = self.vault.absolute(tab.note_id.as_str());
        let text = tab.editor.text();
        let tmp = path.with_extension("dunite-tmp");
        if std::fs::write(&tmp, &text).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
            if let Some(tab) = self.tabs.get_mut(ix) {
                tab.editor.mark_saved();
            }
        }
    }

    /// Сохранить все открытые вкладки.
    pub(crate) fn save_all(&mut self) {
        for ix in 0..self.tabs.len() {
            self.save_tab_now(ix);
        }
    }
}

fn page_lines() -> usize {
    20
}

// ── IME / текстовый ввод (EntityInputHandler) ────────────────────────────

impl EntityInputHandler for Workspace {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let tab = self.tabs.get(self.active_tab)?;
        let text = tab.editor.text();
        let start = utf16_to_char(&text, range_utf16.start);
        let end = utf16_to_char(&text, range_utf16.end).max(start);
        actual_range.replace(char_to_utf16(&text, start)..char_to_utf16(&text, end));
        let (from, to) = (char_to_byte(&text, start), char_to_byte(&text, end));
        Some(text[from..to].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let tab = self.tabs.get(self.active_tab)?;
        let text = tab.editor.text();
        let selection = tab.editor.selection();
        let range = selection.range();
        Some(UTF16Selection {
            range: char_to_utf16(&text, range.start)..char_to_utf16(&text, range.end),
            reversed: selection.head < selection.anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        // preedit IME — минимальная поддержка: без маркированного текста
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(range_utf16) = range_utf16 {
            let range = {
                let Some(tab) = self.tabs.get(self.active_tab) else {
                    return;
                };
                let text = tab.editor.text();
                let start = utf16_to_char(&text, range_utf16.start);
                let end = utf16_to_char(&text, range_utf16.end).max(start);
                start..end
            };
            self.with_editor(cx, |editor, _| {
                editor.set_cursor(range.start, false);
                editor.set_cursor(range.end, true);
                editor.insert_text(new_text);
            });
        } else {
            self.with_editor(cx, |editor, _| editor.insert_text(new_text));
        }
        self.after_edit(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // маркированный (preedit) текст не поддержан — вставляем как обычный ввод
        self.replace_text_in_range(range_utf16, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: gpui::Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<Pixels>> {
        let paint = self.paint_state.as_ref()?;
        let tab = self.tabs.get(self.active_tab)?;
        let text = tab.editor.text();
        let buffer = tab.editor.buffer();
        let start = utf16_to_char(&text, range_utf16.start);
        let line = buffer.line_of(start);
        let content = buffer.line_content_range(line);
        let row = paint.row_at(line)?;
        let vis_byte = row.vis_byte_for_char(start.clamp(content.start, content.end));
        let x = paint.text_left() + row.shaped.x_for_index(vis_byte);
        let y = bounds.top() + row.y;
        Some(gpui::Bounds::new(
            gpui::point(x, y),
            gpui::size(px(0.), row.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_for_position(point)?;
        let text = self.tabs.get(self.active_tab)?.editor.text();
        Some(char_to_utf16(&text, offset))
    }
}

fn char_to_byte(text: &str, char_off: usize) -> usize {
    text.char_indices()
        .nth(char_off)
        .map(|(byte_ix, _)| byte_ix)
        .unwrap_or(text.len())
}

fn char_to_utf16(text: &str, char_off: usize) -> usize {
    text.chars().take(char_off).map(char::len_utf16).sum()
}

fn utf16_to_char(text: &str, utf16: usize) -> usize {
    let mut consumed = 0;
    for (ix, ch) in text.chars().enumerate() {
        if consumed >= utf16 {
            return ix;
        }
        consumed += ch.len_utf16();
    }
    text.chars().count()
}

/// Хелпер для PaintState-геометрии (используется в tests-модулях).
#[allow(dead_code)]
fn _assert_paint_state(_: &PaintState) {}
