//! Корневое представление рабочей области (SPEC §9).
//!
//! Состояние M1: дерево, вкладки с редакторами, автосейв, статус-бар.
//! Дальше — сплиты, сайдбары (outline/backlinks/теги), палитры команд.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use dunite_editor::Editor;
use dunite_index::LinkGraph;
use dunite_markdown::ParsedNote;
use dunite_settings::Settings;
use dunite_vault::{NoteId, Vault};
use gpui::{Context, FocusHandle, FontWeight, Render, RenderImage, Window, div, prelude::*, px};

use crate::actions::{CloseTab, NextTab, PrevTab, Quit};
use crate::decor;
use crate::editor_element::PaintState;
use crate::editor_pane::ClickTracker;
use crate::images;
use crate::theme::Palette;

/// Состояние загрузки изображения (SPEC §7.5).
pub enum ImageState {
    Loading,
    Loaded {
        render: std::sync::Arc<RenderImage>,
        width: u32,
        height: u32,
    },
    Failed,
}

/// Кэш раскладки редактора (SPEC §8.2): парсинг markdown и позиции строк.
///
/// Пересчитывается при смене версии буфера. Инкрементальный репарсинг
/// отдельных блоков — будущая оптимизация; полный парсинг заметки быстр.
pub struct EditorLayout {
    /// Заметка, для которой посчитана раскладка (кэш переживает переключение
    /// вкладок — версии буферов начинаются с 0 и могут совпадать).
    pub note: String,
    pub version: u64,
    /// Ширина панели, для которой посчитаны размеры картинок.
    pub width: f32,
    pub parsed: ParsedNote,
    /// Верх каждой строки (пиксели контента) + итоговая высота.
    pub tops: Rc<Vec<f32>>,
    /// Высоты строк.
    pub heights: Rc<Vec<f32>>,
    /// Строки со standalone-картинками: (номер строки, цель embed).
    pub images: Vec<(usize, String)>,
}

/// Открытая вкладка.
pub struct Tab {
    pub note_id: NoteId,
    pub editor: Editor,
}

/// Рабочая область dunite.
pub struct Workspace {
    pub(crate) vault: Vault,
    /// Граф ссылок/тегов (SPEC §7.2): резолвинг, backlinks, теги.
    pub(crate) graph: LinkGraph,
    pub(crate) settings: Settings,
    pub(crate) palette: Palette,
    /// Развернутые папки дерева (`rel_path`).
    pub(crate) expanded: HashSet<String>,
    /// Развернутые теги в панели тегов.
    pub(crate) expanded_tags: HashSet<String>,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active_tab: usize,
    pub(crate) status_message: String,
    /// Скролл панели редактора (пиксели контента).
    pub(crate) scroll_px: f32,
    /// Геометрия последнего рендера (hit-testing мыши).
    pub(crate) paint_state: Option<PaintState>,
    /// Кэш раскладки (парсинг + вертикальные позиции строк).
    pub(crate) layout: Option<EditorLayout>,
    /// Кэш изображений по цели embed.
    pub(crate) images: HashMap<String, ImageState>,
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
        // полный разбор хранилища при открытии; для больших vault'ов
        // позже уедет в фон (сейчас это миллисекунды)
        let graph = LinkGraph::build(&vault);
        Self {
            vault,
            graph,
            settings,
            palette,
            expanded: HashSet::new(),
            expanded_tags: HashSet::new(),
            tabs: Vec::new(),
            active_tab: 0,
            status_message: String::new(),
            scroll_px: 0.,
            paint_state: None,
            layout: None,
            images: HashMap::new(),
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

    /// Пересчитывает кэш раскладки, если буфер или ширина изменились.
    pub(crate) fn ensure_layout(&mut self, width: f32) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            self.layout = None;
            return;
        };
        let version = tab.editor.version();
        let note = tab.note_id.as_str().to_string();
        if self.layout.as_ref().is_some_and(|layout| {
            layout.note == note && layout.version == version && (layout.width - width).abs() < 2.
        }) {
            return;
        }
        let text = tab.editor.text();
        let parsed = dunite_markdown::parse(&text);
        let buffer = tab.editor.buffer();
        let total_lines = buffer.len_lines();

        let mut heights = Vec::with_capacity(total_lines);
        let mut images = Vec::new();
        for line_ix in 0..total_lines {
            let line_start = buffer.line_start(line_ix);
            let kind = parsed
                .blocks
                .iter()
                .find(|block| block.range.start <= line_start && line_start < block.range.end)
                .map(|block| &block.kind);
            let mut height = decor::line_metrics(kind).0;

            // строки со standalone-картинками (SPEC §8.2)
            let content = buffer.line_content_range(line_ix);
            let line_text = buffer.line(line_ix);
            if let Some(target) = decor::standalone_image_target(&parsed, &content, &line_text) {
                let target_text = buffer.slice(target);
                height = match self.images.get(&target_text) {
                    Some(ImageState::Loaded {
                        width: w,
                        height: h,
                        ..
                    }) => {
                        let (_, display_h) = images::fit_size(
                            *w,
                            *h,
                            width
                                - crate::editor_element::GUTTER_WIDTH_PX
                                - 2. * crate::editor_element::TEXT_PADDING_PX,
                            images::MAX_IMAGE_HEIGHT,
                        );
                        height + display_h + 8.
                    }
                    _ => height + 8.,
                };
                images.push((line_ix, target_text));
            }
            heights.push(height);
        }
        let mut tops = Vec::with_capacity(total_lines + 1);
        let mut acc = 0.;
        for height in &heights {
            tops.push(acc);
            acc += height;
        }
        tops.push(acc);

        self.layout = Some(EditorLayout {
            note,
            version,
            width,
            parsed,
            heights: Rc::new(heights),
            tops: Rc::new(tops),
            images,
        });
    }

    /// Отображаемый размер картинки строки (ширина, высота в px).
    pub(crate) fn image_display_size(&self, target: &str, avail_width: f32) -> Option<(f32, f32)> {
        match self.images.get(target)? {
            ImageState::Loaded { width, height, .. } => Some(images::fit_size(
                *width,
                *height,
                avail_width,
                images::MAX_IMAGE_HEIGHT,
            )),
            _ => None,
        }
    }

    /// Резолвит цель embed в путь файла (SPEC §7.5).
    pub(crate) fn resolve_image_path(&self, target: &str) -> Option<std::path::PathBuf> {
        let direct = self.vault.absolute(target);
        if direct.is_file() {
            return Some(direct);
        }
        let name = std::path::Path::new(target)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())?;
        self.vault
            .find_file_by_name(&name)
            .map(|rel| self.vault.absolute(&rel))
    }

    /// Запускает фоновую загрузку изображения (однократно на цель).
    pub(crate) fn load_missing_image(&mut self, target: String, cx: &mut Context<Self>) {
        if self.images.contains_key(&target) {
            return;
        }
        let Some(path) = self.resolve_image_path(&target) else {
            self.images.insert(target, ImageState::Failed);
            return;
        };
        self.images.insert(target.clone(), ImageState::Loading);
        cx.spawn(async move |this, cx| {
            let decoded = cx
                .background_executor()
                .spawn(async move {
                    let bytes = std::fs::read(path).ok()?;
                    images::decode_image(&bytes)
                })
                .await;
            this.update(cx, |workspace, cx| {
                let state = match decoded {
                    Some(decoded) => ImageState::Loaded {
                        render: decoded.render,
                        width: decoded.width,
                        height: decoded.height,
                    },
                    None => ImageState::Failed,
                };
                workspace.images.insert(target, state);
                // высоты строк изменились
                workspace.layout = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Индекс активного блока (блок со строкой курсора) в кэше раскладки.
    ///
    /// Курсор в конце строки блока относится к следующей строке и не
    /// «включает» его маркеры (SPEC §8.2).
    pub(crate) fn active_block_index(&self) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let tab = self.tabs.get(self.active_tab)?;
        let buffer = tab.editor.buffer();
        let cursor = tab.editor.selection().head;
        let line_start = buffer.line_start(buffer.line_of(cursor));
        layout.parsed.block_index_at(line_start)
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
                    .child(self.render_editor_column(cx))
                    .child(self.render_right_sidebar(cx)),
            )
            .child(self.render_status_bar())
    }
}
