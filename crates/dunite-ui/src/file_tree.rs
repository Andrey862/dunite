//! Дерево файлов (SPEC §9.1).
//!
//! M0: отображение, сворачивание папок, открытие заметок по клику.
//! Дальше: создание/переименование/удаление, drag & drop, контекстное меню.

use dunite_vault::{EntryKind, VaultEntry};
use gpui::{AnyElement, Context, div, prelude::*, px};

use crate::workspace::Workspace;

impl Workspace {
    /// Левая панель: заголовок + дерево хранилища.
    pub(crate) fn render_file_tree(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(260.))
            .border_r_1()
            .border_color(self.palette.border)
            .bg(self.palette.panel)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(self.palette.border)
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_size(px(12.))
                            .child("Файлы"),
                    ),
            )
            .child(
                div()
                    .id("file-tree")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(self.render_entries(self.vault.tree(), 0, cx)),
            )
    }

    /// Рекурсивный рендер записей дерева.
    pub(crate) fn render_entries(
        &self,
        entries: &[VaultEntry],
        depth: usize,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        entries
            .iter()
            .map(|entry| {
                let is_folder = entry.is_folder();
                let kind = entry.kind;
                let expanded = is_folder && self.expanded.contains(&entry.rel_path);
                let rel_path = entry.rel_path.clone();

                let glyph = match entry.kind {
                    EntryKind::Folder if expanded => "▾",
                    EntryKind::Folder => "▸",
                    EntryKind::Note => "•",
                    EntryKind::Image => "▣",
                    EntryKind::Other => "□",
                };

                // ВНИМАНИЕ: id должен быть уникален во всём дереве, а не только
                // среди соседей. gpui хранит состояние клика (pending_mouse_down)
                // по глобальному пути id; коллизия id строк разных уровней вложенности
                // «съедает» клики (один обработчик выбрасывает событие другого).
                // rel_path уникален — используем его.
                let row_id: gpui::SharedString = rel_path.clone().into();
                let row = div()
                    .id(row_id)
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .pl(px(8. + 14. * depth as f32))
                    .pr_2()
                    .py_0p5()
                    .cursor_pointer()
                    .hover(|style| style.bg(self.palette.hover))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        if kind.is_folder() {
                            this.toggle_folder(&rel_path, cx);
                        } else if kind.is_note() {
                            this.open_note(&rel_path, cx);
                        } else {
                            // вложения в редакторе не открываются (SPEC §7.5);
                            // изображения смотрят в заметках через ![[...]]
                            this.status_message =
                                format!("{rel_path}: этот файл нельзя открыть в редакторе");
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .w(px(12.))
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(self.palette.text_muted)
                            .child(glyph),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .child(entry.name.clone()),
                    );

                let mut node = div().flex().flex_col().child(row);
                if expanded {
                    node = node.children(self.render_entries(&entry.children, depth + 1, cx));
                }
                node.into_any_element()
            })
            .collect()
    }
}
