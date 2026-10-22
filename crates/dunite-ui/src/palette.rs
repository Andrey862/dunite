//! Палитра (SPEC §9.4): quick switcher (Ctrl+P) и переименование (F2).
//!
//! Оверлей с текстовым вводом и списком результатов. Ввод текста идёт
//! через штатный IME-механизм gpui (EntityInputHandler маршрутизирует
//! ввод в палитру, пока она открыта).

use dunite_index::SearchHit;
use dunite_vault::NoteId;
use gpui::{
    App, Bounds, Context, Element, ElementId, Entity, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, ShapedLine, Style, TextRun, Window, div, fill, point,
    prelude::*, px, relative, size,
};

use crate::actions::{
    Copy, CursorLeft, CursorLineEnd, CursorLineStart, CursorRight, Cut, DeleteForward,
    PaletteBackspace, PaletteCancel, PaletteConfirm, PaletteNext, PalettePrev, Paste, SelectAll,
    SelectLeft, SelectLineEnd, SelectLineStart, SelectRight,
};
use crate::workspace::Workspace;

/// Режим палитры.
pub enum PaletteMode {
    /// Быстрый переход по заметкам.
    QuickSwitcher,
    /// Полнотекстовый поиск (SPEC §11).
    Search,
    /// Переименование заметки (с переписыванием ссылок).
    Rename { note: NoteId },
}

/// Результаты палитры.
pub enum PaletteResults {
    /// Заметки (quick switcher).
    Notes(Vec<NoteId>),
    /// Результаты поиска со сниппетами.
    Hits(Vec<SearchHit>),
}

impl PaletteResults {
    fn len(&self) -> usize {
        match self {
            PaletteResults::Notes(ids) => ids.len(),
            PaletteResults::Hits(hits) => hits.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Состояние открытой палитры.
pub struct PaletteState {
    pub mode: PaletteMode,
    pub query: String,
    /// Каретка в символах.
    pub cursor: usize,
    /// Якорь выделения (равен курсору, когда выделения нет).
    pub anchor: usize,
    pub selected: usize,
    pub results: PaletteResults,
}

impl PaletteState {
    /// Новое состояние с кареткой в конце запроса.
    pub fn new(mode: PaletteMode, query: String, results: PaletteResults) -> Self {
        let end = query.chars().count();
        Self {
            mode,
            query,
            cursor: end,
            anchor: end,
            selected: 0,
            results,
        }
    }

    /// Выделение (min..max, в символах).
    pub fn selection(&self) -> std::ops::Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.cursor
    }

    pub fn selected_text(&self) -> String {
        let range = self.selection();
        self.query
            .chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect()
    }

    /// Вставка текста: заменяет выделение, каретка — за вставленным.
    pub fn insert_text(&mut self, text: &str) {
        let range = self.selection();
        self.replace_range(range, text);
    }

    /// Backspace: выделение или символ слева.
    pub fn backspace(&mut self) {
        if self.has_selection() {
            let range = self.selection();
            self.replace_range(range, "");
        } else if self.cursor > 0 {
            self.replace_range(self.cursor - 1..self.cursor, "");
        }
    }

    /// Delete: выделение или символ справа.
    pub fn delete_forward(&mut self) {
        let len = self.query.chars().count();
        if self.has_selection() {
            let range = self.selection();
            self.replace_range(range, "");
        } else if self.cursor < len {
            self.replace_range(self.cursor..self.cursor + 1, "");
        }
    }

    /// Движение каретки; `extend` расширяет выделение.
    pub fn move_cursor(&mut self, offset: usize, extend: bool) {
        let len = self.query.chars().count();
        self.cursor = offset.min(len);
        if !extend {
            self.anchor = self.cursor;
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.query.chars().count();
    }

    fn replace_range(&mut self, range: std::ops::Range<usize>, text: &str) {
        let mut chars: Vec<char> = self.query.chars().collect();
        chars.splice(range.clone(), text.chars());
        self.query = chars.into_iter().collect();
        self.cursor = range.start + text.chars().count();
        self.anchor = self.cursor;
    }
}

impl Workspace {
    /// Открывает quick switcher (Ctrl+P).
    pub(crate) fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.switcher_matches("");
        self.palette_state = Some(PaletteState::new(
            PaletteMode::QuickSwitcher,
            String::new(),
            PaletteResults::Notes(matches),
        ));
        self.palette_focus.focus(window);
        cx.notify();
    }

    /// Открывает полнотекстовый поиск (Ctrl+Shift+F, SPEC §11).
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_state = Some(PaletteState::new(
            PaletteMode::Search,
            String::new(),
            PaletteResults::Hits(Vec::new()),
        ));
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
        self.palette_state = Some(PaletteState::new(
            PaletteMode::Rename { note },
            query,
            PaletteResults::Notes(Vec::new()),
        ));
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
        state.insert_text(text);
        self.refresh_palette_results();
        cx.notify();
    }

    pub(crate) fn palette_backspace(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        state.backspace();
        self.refresh_palette_results();
        cx.notify();
    }

    pub(crate) fn palette_delete_forward(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        state.delete_forward();
        self.refresh_palette_results();
        cx.notify();
    }

    /// Движение каретки в запросе палитры.
    pub(crate) fn palette_cursor(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        state.move_cursor(offset, extend);
        cx.notify();
    }

    pub(crate) fn palette_cursor_left(&mut self, extend: bool, cx: &mut Context<Self>) {
        let offset = self
            .palette_state
            .as_ref()
            .map(|state| state.cursor.saturating_sub(1))
            .unwrap_or(0);
        self.palette_cursor(offset, extend, cx);
    }

    pub(crate) fn palette_cursor_right(&mut self, extend: bool, cx: &mut Context<Self>) {
        let offset = self
            .palette_state
            .as_ref()
            .map(|state| state.cursor + 1)
            .unwrap_or(0);
        self.palette_cursor(offset, extend, cx);
    }

    pub(crate) fn palette_select_all(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.palette_state.as_mut() {
            state.select_all();
        }
        cx.notify();
    }

    /// Копирование выделения запроса в буфер обмена.
    pub(crate) fn palette_copy(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_ref() else {
            return;
        };
        let text = state.selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
    }

    pub(crate) fn palette_cut(&mut self, cx: &mut Context<Self>) {
        self.palette_copy(cx);
        if let Some(state) = self.palette_state.as_mut() {
            if state.has_selection() {
                let range = state.selection();
                state.insert_text("");
                let _ = range;
            }
        }
        self.refresh_palette_results();
        cx.notify();
    }

    pub(crate) fn palette_paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.palette_insert(&text, cx);
    }

    /// Пересчёт результатов по текущему запросу (поиск — по мере ввода).
    fn refresh_palette_results(&mut self) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        let query = state.query.clone();
        let is_search = matches!(state.mode, PaletteMode::Search);
        let is_switcher = matches!(state.mode, PaletteMode::QuickSwitcher);
        let results = if is_search {
            PaletteResults::Hits(self.graph.search(&query))
        } else if is_switcher {
            PaletteResults::Notes(self.switcher_matches(&query))
        } else {
            PaletteResults::Notes(Vec::new())
        };
        if let Some(state) = self.palette_state.as_mut() {
            state.results = results;
            state.selected = 0;
        }
    }

    /// Перемещение выбора в списке результатов.
    pub(crate) fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        if state.results.is_empty() {
            return;
        }
        let len = state.results.len() as isize;
        let next = (state.selected as isize + delta).rem_euclid(len) as usize;
        state.selected = next;
        cx.notify();
    }

    /// Enter: открыть выбранное или выполнить переименование.
    pub(crate) fn palette_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.take() else {
            return;
        };
        self.focus_handle.focus(window);
        match (state.mode, state.results) {
            (PaletteMode::QuickSwitcher, PaletteResults::Notes(ids)) => {
                if let Some(id) = ids.into_iter().nth(state.selected) {
                    self.open_note(id.as_str(), cx);
                }
            }
            (PaletteMode::Search, PaletteResults::Hits(hits)) => {
                if let Some(hit) = hits.into_iter().nth(state.selected) {
                    // переход к первой строке со совпадением
                    let line = hit
                        .snippets
                        .first()
                        .map(|snippet| snippet.line)
                        .unwrap_or(0);
                    self.open_note_at_line(hit.id.as_str(), line, cx);
                }
            }
            (PaletteMode::Rename { note }, _) => {
                self.rename_note_to(&note, &state.query, cx);
            }
            _ => {}
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

    /// Перечитывает вкладки с диска, сохраняя позиции курсора.
    ///
    /// Грязные вкладки не трогаем: несохранённые правки важнее диска
    /// (конфликт показываем статус-сообщением, SPEC §14).
    pub(crate) fn reload_all_tabs(&mut self) {
        for tab in &mut self.tabs {
            if tab.editor.is_dirty() {
                if let Ok(text) = self.vault.read_note(tab.note_id.as_str()) {
                    if text != tab.editor.text() {
                        tab.conflict = true;
                        self.status_message = format!(
                            "{} изменён снаружи: автосейв остановлен; Ctrl+S — записать поверх, F5 — перечитать",
                            tab.note_id.as_str()
                        );
                    }
                }
                continue;
            }
            let Ok(text) = self.vault.read_note(tab.note_id.as_str()) else {
                continue;
            };
            if text != tab.editor.text() {
                let cursor = tab.editor.selection().head;
                tab.editor = dunite_editor::Editor::from_text(text);
                let max = tab.editor.buffer().len_chars();
                tab.editor.set_cursor(cursor.min(max), false);
                tracing::info!("watcher: вкладка перечитана: {}", tab.note_id.as_str());
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
            PaletteMode::Search => "Поиск: слова, \"фраза\", tag:, path:, link: — Enter открыть",
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
                    .flex_shrink_0()
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
            .child(div().flex_shrink_0().child(PaletteInput {
                workspace: cx.entity(),
            }))
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
            // редактирование запроса (контекст `palette`)
            .on_action(
                cx.listener(|this, _: &CursorLeft, _, cx| this.palette_cursor_left(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CursorRight, _, cx| this.palette_cursor_right(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectLeft, _, cx| this.palette_cursor_left(true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectRight, _, cx| this.palette_cursor_right(true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CursorLineStart, _, cx| this.palette_cursor(0, false, cx)),
            )
            .on_action(cx.listener(|this, _: &CursorLineEnd, _, cx| {
                let end = this
                    .palette_state
                    .as_ref()
                    .map(|state| state.query.chars().count())
                    .unwrap_or(0);
                this.palette_cursor(end, false, cx);
            }))
            .on_action(
                cx.listener(|this, _: &SelectLineStart, _, cx| this.palette_cursor(0, true, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| {
                let end = this
                    .palette_state
                    .as_ref()
                    .map(|state| state.query.chars().count())
                    .unwrap_or(0);
                this.palette_cursor(end, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.palette_select_all(cx)))
            .on_action(
                cx.listener(|this, _: &DeleteForward, _, cx| this.palette_delete_forward(cx)),
            )
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.palette_copy(cx)))
            .on_action(cx.listener(|this, _: &Cut, _, cx| this.palette_cut(cx)))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.palette_paste(cx)))
            .child(
                div()
                    .id("palette-results")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .children(match &state.results {
                        PaletteResults::Notes(ids) => ids
                            .iter()
                            .enumerate()
                            .map(|(ix, id)| {
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
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                        PaletteResults::Hits(hits) => hits
                            .iter()
                            .enumerate()
                            .map(|(ix, hit)| {
                                let is_selected = ix == state.selected;
                                let rel_path = hit.id.as_str().to_string();
                                let title = self
                                    .graph
                                    .meta(&hit.id)
                                    .map(|meta| meta.title.clone())
                                    .unwrap_or_else(|| hit.id.file_stem().to_string());
                                let open_path = rel_path.clone();
                                let line = hit.snippets.first().map(|s| s.line).unwrap_or(0);
                                div()
                                    .id(gpui::SharedString::from(format!("palette-hit:{rel_path}")))
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
                                        this.open_note_at_line(&open_path, line, cx);
                                        cx.notify();
                                    }))
                                    .child(div().text_size(px(13.)).child(title))
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(self.palette.text_muted)
                                            .child(rel_path),
                                    )
                                    .children(
                                        hit.snippets
                                            .iter()
                                            .map(|snippet| self.render_snippet(snippet)),
                                    )
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                    }),
            )
            .children(self.palette_hint(state));

        Some(
            div()
                .id("palette-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .flex()
                .justify_center()
                .items_start()
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

    /// Сниппет результата поиска: сегменты с подсветкой совпадений.
    fn render_snippet(&self, snippet: &dunite_index::Snippet) -> impl IntoElement {
        let mut spans = Vec::new();
        let mut pos = 0usize;
        for (start, end) in &snippet.ranges {
            if *start > pos {
                spans.push(self.snippet_span(&snippet.text[pos..*start], false));
            }
            spans.push(self.snippet_span(&snippet.text[*start..*end], true));
            pos = *end;
        }
        if pos < snippet.text.len() {
            spans.push(self.snippet_span(&snippet.text[pos..], false));
        }
        if spans.is_empty() {
            spans.push(self.snippet_span(&snippet.text, false));
        }
        div()
            .flex()
            .flex_wrap()
            .mt_1()
            .text_size(px(11.))
            .children(spans)
    }

    fn snippet_span(&self, text: &str, matched: bool) -> impl IntoElement {
        div()
            .px_0p5()
            .bg(if matched {
                self.palette.selection
            } else {
                self.palette.background
            })
            .text_color(if matched {
                self.palette.text
            } else {
                self.palette.text_muted
            })
            .child(text.to_string())
    }

    /// Подсказка внизу панели: пустой запрос или отсутствие результатов.
    fn palette_hint(&self, state: &PaletteState) -> Option<impl IntoElement> {
        if !state.results.is_empty() {
            return None;
        }
        let hint = if state.query.trim().is_empty() {
            match state.mode {
                PaletteMode::Search => "Введите запрос — поиск идёт по мере ввода",
                PaletteMode::QuickSwitcher => "Введите имя заметки",
                PaletteMode::Rename { .. } => return None,
            }
        } else {
            "Ничего не найдено"
        };
        Some(
            div()
                .px_3()
                .py_2()
                .text_size(px(11.))
                .text_color(self.palette.text_muted)
                .child(hint),
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
    selection: Option<gpui::PaintQuad>,
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
        let (query, cursor_char, anchor_char) = workspace
            .palette_state
            .as_ref()
            .map(|state| (state.query.clone(), state.cursor, state.anchor))
            .unwrap_or_default();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let text_left = bounds.left() + px(12.);

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
                .shape_line(query.clone().into(), font_size, &[run], None)
        });

        // позиции каретки и выделения по byte-смещениям shaped-строки
        let byte_at = |char_offset: usize| -> usize {
            crate::editor_element::byte_of_col(&query, char_offset)
        };
        let x_at = |char_offset: usize| -> Pixels {
            line.as_ref()
                .map(|line| line.x_for_index(byte_at(char_offset)))
                .unwrap_or(px(0.))
        };

        let selection = if anchor_char != cursor_char {
            let from = x_at(anchor_char.min(cursor_char));
            let to = x_at(anchor_char.max(cursor_char));
            Some(fill(
                Bounds::from_corners(
                    point(text_left + from, bounds.top() + px(8.)),
                    point(text_left + to, bounds.top() + px(30.)),
                ),
                palette.selection,
            ))
        } else {
            None
        };

        let cursor = fill(
            Bounds::new(
                point(text_left + x_at(cursor_char), bounds.top() + px(8.)),
                size(px(2.), px(22.)),
            ),
            palette.cursor,
        );

        PaletteInputPrepaint {
            line,
            cursor: Some(cursor),
            selection,
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
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state(query: &str) -> PaletteState {
        PaletteState::new(
            PaletteMode::QuickSwitcher,
            query.to_string(),
            PaletteResults::Notes(Vec::new()),
        )
    }

    #[test]
    fn insert_replaces_selection_and_moves_cursor() {
        let mut s = state("hello world");
        s.select_all();
        s.insert_text("привет");
        assert_eq!(s.query, "привет");
        assert_eq!(s.cursor, 6);
        assert!(!s.has_selection());
    }

    #[test]
    fn insert_in_middle() {
        let mut s = state("аб");
        s.move_cursor(1, false);
        s.insert_text("Х");
        assert_eq!(s.query, "аХб");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn backspace_deletes_selection_or_char() {
        let mut s = state("абв");
        s.move_cursor(2, false);
        s.move_cursor(0, true); // выделение "аб"
        s.backspace();
        assert_eq!(s.query, "в");

        let mut s = state("абв");
        s.backspace(); // каретка в конце — удалит "в"
        assert_eq!(s.query, "аб");
    }

    #[test]
    fn delete_forward_and_edges() {
        let mut s = state("аб");
        s.move_cursor(0, false);
        s.delete_forward();
        assert_eq!(s.query, "б");
        s.delete_forward();
        assert_eq!(s.query, "");
        s.delete_forward(); // не падает на пустом
        s.backspace(); // не падает в начале
        assert_eq!(s.query, "");
    }

    #[test]
    fn selection_extends_and_collapses() {
        let mut s = state("abcdef");
        s.move_cursor(2, false);
        s.move_cursor(5, true);
        assert_eq!(s.selected_text(), "cde");
        s.move_cursor(1, true); // реверс выделения
        assert_eq!(s.selection(), 1..2);
        s.move_cursor(3, false);
        assert!(!s.has_selection());
    }

    #[test]
    fn cut_removes_selected_text() {
        let mut s = state("длинный запрос");
        s.select_all();
        let text = s.selected_text();
        assert_eq!(text, "длинный запрос");
        s.insert_text(""); // cut = copy + delete
        assert_eq!(s.query, "");
    }
}
