//! Дерево файлов (SPEC §9.1).
//!
//! M0: отображение, сворачивание папок, открытие заметок по клику.
//! Дальше: создание/переименование/удаление, drag & drop, контекстное меню.

use dunite_vault::{EntryKind, VaultEntry};
use gpui::{AnyElement, Context, Render, Window, div, prelude::*, px, rgb};

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
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _window, cx| {
                            this.open_tree_menu(event.position, None, cx);
                        }),
                    )
                    // drop в пустую область дерева = перемещение в корень
                    .can_drop(|dragged: &dyn std::any::Any, _window, _cx| {
                        dragged
                            .downcast_ref::<crate::tree_menu::TreeTarget>()
                            .is_some_and(|drag| crate::tree_menu::drop_allowed(drag, ""))
                    })
                    .drag_over::<crate::tree_menu::TreeTarget>({
                        let drag_bg = self.palette.hover;
                        move |style, _drag, _window, _cx| style.bg(drag_bg)
                    })
                    .on_drop(cx.listener(
                        |this, drag: &crate::tree_menu::TreeTarget, _window, cx| {
                            this.tree_move_entry(&drag.path, "", cx);
                        },
                    ))
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
                let menu_path = rel_path.clone();
                let drag_target = crate::tree_menu::TreeTarget {
                    path: rel_path.clone(),
                    is_folder: kind.is_folder(),
                };
                let drop_dest = if kind.is_folder() {
                    rel_path.clone()
                } else {
                    crate::tree_menu::parent_dir(&rel_path)
                };
                let drag_bg = self.palette.selection;
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
                    // drag & drop: перетаскивание с обновлением ссылок (§9.1)
                    .on_drag(
                        drag_target,
                        |target: &crate::tree_menu::TreeTarget, _, _, cx| {
                            cx.new(|_| DragGhost {
                                name: target
                                    .path
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(&target.path)
                                    .to_string(),
                                is_folder: target.is_folder,
                            })
                        },
                    )
                    .can_drop({
                        let dest = drop_dest.clone();
                        move |dragged: &dyn std::any::Any, _window, _cx| {
                            dragged
                                .downcast_ref::<crate::tree_menu::TreeTarget>()
                                .is_some_and(|drag| crate::tree_menu::drop_allowed(drag, &dest))
                        }
                    })
                    .drag_over::<crate::tree_menu::TreeTarget>(move |style, _drag, _window, _cx| {
                        style.bg(drag_bg)
                    })
                    .on_drop(cx.listener(
                        move |this, drag: &crate::tree_menu::TreeTarget, _window, cx| {
                            this.tree_move_entry(&drag.path, &drop_dest, cx);
                        },
                    ))
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
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                            // клик по строке не должен всплывать к контейнеру:
                            // иначе его обработчик перезапишет меню корневым
                            cx.stop_propagation();
                            this.open_tree_menu(
                                event.position,
                                Some(crate::tree_menu::TreeTarget {
                                    path: menu_path.clone(),
                                    is_folder: kind.is_folder(),
                                }),
                                cx,
                            );
                        }),
                    )
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

/// Призрак перетаскивания — маленькая плашка с именем файла/папки.
struct DragGhost {
    name: String,
    is_folder: bool,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x8a8a8a))
            .bg(rgb(0x2f2f30))
            .text_size(px(12.))
            .text_color(rgb(0xf5f5f5))
            .shadow_md()
            .child(format!(
                "{} {}",
                if self.is_folder { "▸" } else { "•" },
                self.name
            ))
    }
}
