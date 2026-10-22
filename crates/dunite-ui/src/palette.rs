//! Палитра (SPEC §9.4): quick switcher (Ctrl+P) и переименование (F2).
//!
//! Оверлей с текстовым вводом и списком результатов. Ввод текста идёт
//! через штатный IME-механизм gpui (EntityInputHandler маршрутизирует
//! ввод в палитру, пока она открыта).

use dunite_index::SearchHit;
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
    pub selected: usize,
    pub results: PaletteResults,
}

impl Workspace {
    /// Открывает quick switcher (Ctrl+P).
    pub(crate) fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.switcher_matches("");
        self.palette_state = Some(PaletteState {
            mode: PaletteMode::QuickSwitcher,
            query: String::new(),
            selected: 0,
            results: PaletteResults::Notes(matches),
        });
        self.palette_focus.focus(window);
        cx.notify();
    }

    /// Открывает полнотекстовый поиск (Ctrl+Shift+F, SPEC §11).
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_state = Some(PaletteState {
            mode: PaletteMode::Search,
            query: String::new(),
            selected: 0,
            results: PaletteResults::Hits(Vec::new()),
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
            results: PaletteResults::Notes(Vec::new()),
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
        self.refresh_palette_results();
        cx.notify();
    }

    pub(crate) fn palette_backspace(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.palette_state.as_mut() else {
            return;
        };
        state.query.pop();
        self.refresh_palette_results();
        cx.notify();
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
                        self.status_message = format!(
                            "{} изменён снаружи и не совпадает с редактором",
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
            })
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
