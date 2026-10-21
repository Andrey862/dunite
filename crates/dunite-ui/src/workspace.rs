//! Корневое представление рабочей области (SPEC §9).
//!
//! Состояние M1: дерево, вкладки с редакторами, автосейв, статус-бар.
//! Дальше — сплиты, сайдбары (outline/backlinks/теги), палитры команд.

use std::collections::HashSet;
use std::rc::Rc;

use dunite_editor::Editor;
use dunite_markdown::ParsedNote;
use dunite_settings::Settings;
use dunite_vault::{NoteId, Vault};
use gpui::{Context, FocusHandle, FontWeight, Render, Window, div, prelude::*, px};

use crate::actions::{CloseTab, NextTab, PrevTab, Quit};
use crate::decor;
use crate::editor_element::PaintState;
use crate::editor_pane::ClickTracker;
use crate::theme::Palette;

/// Кэш раскладки редактора (SPEC §8.2): парсинг markdown и позиции строк.
///
/// Пересчитывается при смене версии буфера. Инкрементальный репарсинг
/// отдельных блоков — будущая оптимизация; полный парсинг заметки быстр.
pub struct EditorLayout {
    pub version: u64,
    pub parsed: ParsedNote,
    /// Верх каждой строки (пиксели контента) + итоговая высота.
    pub tops: Rc<Vec<f32>>,
    /// Высоты строк.
    pub heights: Rc<Vec<f32>>,
}

/// Открытая вкладка.
pub struct Tab {
    pub note_id: NoteId,
    pub editor: Editor,
}

/// Рабочая область dunite.
pub struct Workspace {
    pub(crate) vault: Vault,
    pub(crate) settings: Settings,
    pub(crate) palette: Palette,
    /// Развернутые папки дерева (`rel_path`).
    pub(crate) expanded: HashSet<String>,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active_tab: usize,
    pub(crate) status_message: String,
    /// Скролл панели редактора (пиксели контента).
    pub(crate) scroll_px: f32,
    /// Геометрия последнего рендера (hit-testing мыши).
    pub(crate) paint_state: Option<PaintState>,
    /// Кэш раскладки (парсинг + вертикальные позиции строк).
    pub(crate) layout: Option<EditorLayout>,
    /// Идёт выделение мышью (drag).
    pub(crate) selecting: bool,
    /// Распознавание двойного/тройного клика.
    pub(crate) click: ClickTracker,
    pub(crate) focus_handle: FocusHandle,
    /// Поколение правок: автосейв пишет только последнее.
    pub(crate) save_generation: u64,
}

impl Workspace {
    pub fn new(vault: Vault, settings: Settings, focus_handle: FocusHandle) -> Self {
        let palette = Palette::for_theme(settings.theme);
        Self {
            vault,
            settings,
            palette,
            expanded: HashSet::new(),
            tabs: Vec::new(),
            active_tab: 0,
            status_message: String::new(),
            scroll_px: 0.,
            paint_state: None,
            layout: None,
            selecting: false,
            click: ClickTracker::default(),
            focus_handle,
            save_generation: 0,
        }
    }

    /// Открывает заметку во вкладке (или фокусирует уже открытую).
    pub(crate) fn open_note(&mut self, rel_path: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|tab| tab.note_id.as_str() == rel_path)
        {
            self.active_tab = ix;
            self.status_message.clear();
            cx.notify();
            return;
        }
        match self.vault.read_note(rel_path) {
            Ok(text) => {
                self.tabs.push(Tab {
                    note_id: NoteId::new(rel_path),
                    editor: Editor::from_text(text),
                });
                self.active_tab = self.tabs.len() - 1;
                self.scroll_px = 0.;
                self.status_message.clear();
            }
            Err(err) => {
                self.status_message = format!("не удалось открыть {rel_path}: {err}");
            }
        }
        cx.notify();
    }

    pub(crate) fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active_tab = ix;
            self.scroll_px = 0.;
            cx.notify();
        }
    }

    pub(crate) fn close_tab_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.save_tab_now(ix);
        self.tabs.remove(ix);
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else if ix <= self.active_tab {
            self.active_tab = (self.active_tab - 1).min(self.tabs.len() - 1);
        }
        cx.notify();
    }

    pub(crate) fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.close_tab_at(self.active_tab, cx);
        }
    }

    pub(crate) fn activate_next_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
            self.scroll_px = 0.;
            cx.notify();
        }
    }

    pub(crate) fn activate_prev_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
            self.scroll_px = 0.;
            cx.notify();
        }
    }

    /// Разворачивает/сворачивает папку дерева.
    pub(crate) fn toggle_folder(&mut self, rel_path: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(rel_path) {
            self.expanded.insert(rel_path.to_string());
        }
        cx.notify();
    }

    /// Пересчитывает кэш раскладки, если буфер изменился.
    pub(crate) fn ensure_layout(&mut self) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            self.layout = None;
            return;
        };
        let version = tab.editor.version();
        if self
            .layout
            .as_ref()
            .is_some_and(|layout| layout.version == version)
        {
            return;
        }
        let text = tab.editor.text();
        let parsed = dunite_markdown::parse(&text);
        let buffer = tab.editor.buffer();
        let total_lines = buffer.len_lines();

        let mut heights = Vec::with_capacity(total_lines);
        for line_ix in 0..total_lines {
            let line_start = buffer.line_start(line_ix);
            let kind = parsed
                .blocks
                .iter()
                .find(|block| block.range.start <= line_start && line_start < block.range.end)
                .map(|block| &block.kind);
            heights.push(decor::line_metrics(kind).0);
        }
        let mut tops = Vec::with_capacity(total_lines + 1);
        let mut acc = 0.;
        for height in &heights {
            tops.push(acc);
            acc += height;
        }
        tops.push(acc);

        self.layout = Some(EditorLayout {
            version,
            parsed,
            heights: Rc::new(heights),
            tops: Rc::new(tops),
        });
    }

    /// Индекс активного блока (блок с курсором) в кэше раскладки.
    pub(crate) fn active_block_index(&self) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let tab = self.tabs.get(self.active_tab)?;
        let cursor = tab.editor.selection().head;
        layout
            .parsed
            .blocks
            .iter()
            .position(|block| block.range.start <= cursor && cursor <= block.range.end)
    }

    /// Верхняя панель: имя приложения и корень хранилища.
    fn render_header(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(self.palette.border)
            .bg(self.palette.panel)
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(15.))
                    .child("dunite"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(self.palette.text_muted)
                    .child(format!("{}", self.vault.root().display())),
            )
    }

    /// Центральная колонка: панель вкладок + панель редактора.
    fn render_editor_column(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(self.render_tab_bar(cx))
            .child(self.render_editor_pane(cx))
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("workspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CloseTab, _window, cx| this.close_active_tab(cx)))
            .on_action(cx.listener(|this, _: &NextTab, _window, cx| this.activate_next_tab(cx)))
            .on_action(cx.listener(|this, _: &PrevTab, _window, cx| this.activate_prev_tab(cx)))
            .on_action(cx.listener(|this, _: &Quit, window, _cx| {
                // все несохранённые вкладки пишем на диск (SPEC §10.3)
                this.save_all();
                window.remove_window();
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(self.palette.background)
            .text_color(self.palette.text)
            .child(self.render_header())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_file_tree(cx))
                    .child(self.render_editor_column(cx)),
            )
            .child(self.render_status_bar())
    }
}
