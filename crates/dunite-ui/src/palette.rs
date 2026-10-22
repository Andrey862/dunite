//! Палитра (SPEC §9.4): quick switcher (Ctrl+P) и переименование (F2).
//!
//! Оверлей с текстовым вводом и списком результатов. Ввод текста идёт
//! через штатный IME-механизм gpui (EntityInputHandler маршрутизирует
//! ввод в палитру, пока она открыта).

use dunite_vault::NoteId;
use gpui::{
    App, Bounds, Context, Element, ElementId, Entity, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, ShapedLine, SharedString, Style, TextRun, Window, div, fill,
    point, prelude::*, px, relative, size,
};

use crate::actions::{PaletteBackspace, PaletteCancel, PaletteConfirm, PaletteNext, PalettePrev};
use crate::workspace::Workspace;

/// Режим палитры.
pub enum PaletteMode {
    /// Быстрый переход по заметкам.
    QuickSwitcher,
    /// Переименование заметки (с переписыванием ссылок).
    Rename { note: NoteId },
}

/// Состояние открытой палитры.
pub struct PaletteState {
    pub mode: PaletteMode,
    pub query: String,
    pub selected: usize,
    pub matches: Vec<NoteId>,
}

impl Workspace {
    /// Открывает quick switcher (Ctrl+P).
    pub(crate) fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.switcher_matches("");
        self.palette_state = Some(PaletteState {
            mode: PaletteMode::QuickSwitcher,
            query: String::new(),
            selected: 0,
            matches,
        });
        self.palette_focus.focus(window);
        cx.notify();
    }

    /// Открывает диалог переименования активной заметки (F2).
    pub(crate) fn open_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        let note = tab.note_id.clone();
        let query = note.file_stem().to_string();
        self.palette_state = Some(PaletteState {
            mode: PaletteMode::Rename { note },
            query,
            selected: 0,
            matches: Vec::new(),
        });
        self.palette_focus.focus(window);
        cx.notify();
    }

    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_state.take().is_some() {
            self.focus_handle.focus(window);
            cx.notify();
        }
    }

    /// Ввод текста в палитру (из IME-обработчика).
    pub(crate) fn palette_insert(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        state.query.push_str(text);
        state.selected = 0;
        let query = state.query.clone();
        let matches = self.switcher_matches(&query);
        if let Some(state) = self.palette_state.as_mut() {
            state.matches = matches;
        }
        cx.notify();
    }

    pub(crate) fn palette_backspace(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        state.query.pop();
        state.selected = 0;
        let query = state.query.clone();
        let matches = self.switcher_matches(&query);
        if let Some(state) = self.palette_state.as_mut() {
            state.matches = matches;
        }
        cx.notify();
    }

    /// Перемещение выбора в списке результатов.
    pub(crate) fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        if state.matches.is_empty() {
            return;
        }
        let len = state.matches.len() as isize;
        let next = (state.selected as isize + delta).rem_euclid(len) as usize;
        state.selected = next;
        cx.notify();
    }

    /// Enter: открыть выбранную заметку или выполнить переименование.
    pub(crate) fn palette_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.take() else {
            return;
        };
        self.focus_handle.focus(window);
        match state.mode {
            PaletteMode::QuickSwitcher => {
                if let Some(id) = state.matches.into_iter().nth(state.selected) {
                    self.open_note(id.as_str(), cx);
                }
            }
            PaletteMode::Rename { note } => {
                self.rename_note_to(&note, &state.query, cx);
            }
        }
        cx.notify();
    }

    /// Fuzzy-подбор заметок по заголовку и пути (подпоследовательность
    /// с бонусами за границы слов; nucleo — позже, SPEC §5).
    fn switcher_matches(&self, query: &str) -> Vec<NoteId> {
        let query = query.trim().to_lowercase();
        let mut scored: Vec<(i64, NoteId)> = Vec::new();
        for (id, meta) in self.graph.notes() {
            let title = meta.title.to_lowercase();
            let path = id.as_str().to_lowercase();
            let score = if query.is_empty() {
                0
            } else if let Some(score) = fuzzy_score(&query, &title) {
                score + 100
            } else if let Some(score) = fuzzy_score(&query, &path) {
                score
            } else {
                continue;
            };
            scored.push((score, id.clone()));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.into_iter().take(50).map(|(_, id)| id).collect()
    }

    /// Переименование с переписыванием ссылающихся заметок (SPEC §7.2).
    fn rename_note_to(&mut self, from: &NoteId, query: &str, cx: &mut Context<Self>) {
        let mut name = query.trim().to_string();
        if name.is_empty() {
            self.status_message = "пустое имя заметки".to_string();
            return;
        }
        if !name.ends_with(".md") {
            name.push_str(".md");
        }
        let to = NoteId::new(name);

        // буферы сначала на диск: переименование переписывает файлы
        self.save_all();
        match self.graph.rename_note(&self.vault, from, &to) {
            Ok(updated) => {
                // дерево файлов тоже должно увидеть новое имя
                let _ = self.vault.rescan();
                self.reload_tabs_after_rename(from, &to);
                self.status_message = format!(
                    "переименовано в {}: обновлено ссылок: {updated}",
                    to.as_str()
                );
            }
            Err(err) => {
                self.status_message = format!("не удалось переименовать: {err}");
            }
        }
        cx.notify();
    }

    /// Вкладки после переименования: новое имя и перечитанный текст.
    fn reload_tabs_after_rename(&mut self, from: &NoteId, to: &NoteId) {
        for tab in &mut self.tabs {
            if &tab.note_id == from {
                tab.note_id = to.clone();
            }
        }
        self.reload_all_tabs();
    }

    /// Перечитывает все вкладки с диска, сохраняя позиции курсора.
    pub(crate) fn reload_all_tabs(&mut self) {
        for tab in &mut self.tabs {
            let Ok(text) = self.vault.read_note(tab.note_id.as_str()) else {
                continue;
            };
            if text != tab.editor.text() {
                let cursor = tab.editor.selection().head;
                tab.editor = dunite_editor::Editor::from_text(text);
                let max = tab.editor.buffer().len_chars();
                tab.editor.set_cursor(cursor.min(max), false);
            }
        }
    }
}

/// Скоринг фаззи-подпоследовательности: None — не совпало.
fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    let mut score = 0i64;
    let mut query_chars = query.chars().peekable();
    let mut word_boundary = true;
    for ch in text.chars() {
        if let Some(&expected) = query_chars.peek() {
            if expected == ch {
                score += if word_boundary { 5 } else { 1 };
                query_chars.next();
            }
        }
        word_boundary = matches!(ch, ' ' | '/' | '-' | '_' | '.');
    }
    query_chars.peek().is_none().then_some(score)
}

// ── рендер оверлея ───────────────────────────────────────────────────────

impl Workspace {
    /// Оверлей палитры (если открыта).
    pub(crate) fn render_palette(&self, cx: &Context<Self>) -> Option<gpui::AnyElement> {
        let state = self.palette_state.as_ref()?;
        let title = match state.mode {
            PaletteMode::QuickSwitcher => "Переход к заметке",
            PaletteMode::Rename { .. } => "Переименование (ссылки будут обновлены)",
        };

        let panel = div()
            .key_context("palette")
            .track_focus(&self.palette_focus)
            .flex()
            .flex_col()
            .w(px(560.))
            .max_h(px(420.))
            .border_1()
            .border_color(self.palette.border)
            .rounded_md()
            .bg(self.palette.background)
            .shadow_lg()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(self.palette.border)
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(self.palette.text_muted)
                            .child(title),
                    ),
            )
            .child(PaletteInput {
                workspace: cx.entity(),
            })
            .on_action(
                cx.listener(|this, _: &PaletteConfirm, window, cx| {
                    this.palette_confirm(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &PaletteCancel, window, cx| this.close_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &PaletteNext, _, cx| this.palette_move(1, cx)))
            .on_action(cx.listener(|this, _: &PalettePrev, _, cx| this.palette_move(-1, cx)))
            .on_action(cx.listener(|this, _: &PaletteBackspace, _, cx| this.palette_backspace(cx)))
            .children(state.matches.iter().enumerate().map(|(ix, id)| {
                let is_selected = ix == state.selected;
                let rel_path = id.as_str().to_string();
                let title = self
                    .graph
                    .meta(id)
                    .map(|meta| meta.title.clone())
                    .unwrap_or_else(|| id.file_stem().to_string());
                let open_path = rel_path.clone();
                div()
                    .id(gpui::SharedString::from(format!("palette-row:{rel_path}")))
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .bg(if is_selected {
                        self.palette.hover
                    } else {
                        self.palette.background
                    })
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.palette_state = None;
                        this.focus_handle.focus(window);
                        this.open_note(&open_path, cx);
                        cx.notify();
                    }))
                    .child(div().text_size(px(13.)).child(title))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(self.palette.text_muted)
                            .child(rel_path),
                    )
            }));

        Some(
            div()
                .id("palette-overlay")
                .absolute()
                .inset_0()
                .flex_col()
                .items_center()
                .justify_start()
                .pt(px(96.))
                .bg(gpui::hsla(0., 0., 0., 0.35))
                // клик мимо панели закрывает палитру; сама панель ниже
                // останавливает всплытие, чтобы не закрыться при клике внутри
                .on_click(cx.listener(|this, _event, window, cx| this.close_palette(window, cx)))
                .child(panel.id("palette-panel").on_click(
                    |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                        cx.stop_propagation();
                    },
                ))
                .into_any_element(),
        )
    }
}

/// Строка ввода палитры: текст запроса + каретка + IME-ввод.
struct PaletteInput {
    workspace: Entity<Workspace>,
}

struct PaletteInputPrepaint {
    line: Option<ShapedLine>,
    cursor: Option<gpui::PaintQuad>,
}

impl IntoElement for PaletteInput {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PaletteInput {
    type RequestLayoutState = ();
    type PrepaintState = PaletteInputPrepaint;

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
        style.size.height = px(38.).into();
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
        let query = workspace
            .palette_state
            .as_ref()
            .map(|state| state.query.clone())
            .unwrap_or_default();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());

        let run = TextRun {
            len: query.len(),
            font: text_style.font(),
            color: palette.text,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = (!query.is_empty()).then(|| {
            window
                .text_system()
                .shape_line(query.into(), font_size, &[run], None)
        });
        let cursor_x = line
            .as_ref()
            .map(|line| line.width)
            .unwrap_or_else(|| px(0.));
        let cursor = fill(
            Bounds::new(
                point(bounds.left() + px(12.) + cursor_x, bounds.top() + px(8.)),
                size(px(2.), px(22.)),
            ),
            palette.cursor,
        );

        PaletteInputPrepaint {
            line,
            cursor: Some(cursor),
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
        let focus_handle = self.workspace.read(cx).palette_focus.clone();
        window.handle_input(
            &focus_handle,
            gpui::ElementInputHandler::new(bounds, self.workspace.clone()),
            cx,
        );
        if let Some(line) = prepaint.line.take() {
            line.paint(
                point(bounds.left() + px(12.), bounds.top() + px(8.)),
                px(22.),
                window,
                cx,
            )
            .ok();
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
    }
}

/// Хелпер для имён элементов палитры.
#[allow(dead_code)]
fn palette_row_id(rel_path: &str) -> SharedString {
    SharedString::from(format!("palette-row:{rel_path}"))
}
