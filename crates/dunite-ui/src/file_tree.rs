//! Дерево файлов (SPEC §9.1).
//!
//! Виртуализация: рендерятся только строки видимой области. Без неё
//! папка с тысячами файлов создавала тысячи элементов на каждый кадр —
//! набор текста и скролл держали ядро на 100%.

use std::collections::HashSet;

use dunite_vault::{EntryKind, VaultEntry};
use gpui::{Context, Render, Window, div, prelude::*, px, rgb};

use crate::workspace::Workspace;

/// Высота строки дерева (фиксирована — нужна для виртуализации).
const ROW_HEIGHT: f32 = 22.;
/// Сколько строк рендерится вокруг видимой области.
const OVERSCAN: usize = 24;

/// Строка дерева: плоский список видимых (не скрытых) строк.
pub(crate) struct TreeRow {
    pub rel_path: String,
    pub name: String,
    pub kind: EntryKind,
    pub depth: usize,
    pub expanded: bool,
}

/// Разворачивает дерево в плоский список строк (только раскрытые ветки).
pub(crate) fn flatten_tree(entries: &[VaultEntry], expanded: &HashSet<String>) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    walk(entries, 0, expanded, &mut rows);
    rows
}

fn walk(entries: &[VaultEntry], depth: usize, expanded: &HashSet<String>, out: &mut Vec<TreeRow>) {
    for entry in entries {
        let is_folder = entry.is_folder();
        let is_expanded = is_folder && expanded.contains(&entry.rel_path);
        out.push(TreeRow {
            rel_path: entry.rel_path.clone(),
            name: entry.name.clone(),
            kind: entry.kind,
            depth,
            expanded: is_expanded,
        });
        if is_expanded {
            walk(&entry.children, depth + 1, expanded, out);
        }
    }
}

impl Workspace {
    /// Левая панель: заголовок + виртуализированное дерево хранилища.
    pub(crate) fn render_file_tree(&self, cx: &Context<Self>) -> impl IntoElement {
        let rows = flatten_tree(self.vault.tree(), &self.expanded);
        let total = rows.len();
        let scroll_y = self.tree_scroll.offset().y.to_f64() as f32;
        let first = ((scroll_y / ROW_HEIGHT) as usize).saturating_sub(OVERSCAN);
        let last = (first + 2 * OVERSCAN + 40).min(total);
        let spacer_after = total.saturating_sub(last);

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
                    .track_scroll(&self.tree_scroll)
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
                    // пространство до/после видимых строк сохраняет скроллбар
                    .child(div().h(px(first as f32 * ROW_HEIGHT)))
                    .children(
                        rows[first..last]
                            .iter()
                            .map(|row| self.render_tree_row(row, cx)),
                    )
                    .child(div().h(px(spacer_after as f32 * ROW_HEIGHT))),
            )
    }

    /// Одна строка дерева: клик/ПКМ/drag & drop.
    fn render_tree_row(&self, row: &TreeRow, cx: &Context<Self>) -> gpui::AnyElement {
        let is_folder = row.kind.is_folder();
        let glyph = match row.kind {
            EntryKind::Folder if row.expanded => "▾",
            EntryKind::Folder => "▸",
            EntryKind::Note => "•",
            EntryKind::Image => "▣",
            EntryKind::Other => "□",
        };

        let rel_path = row.rel_path.clone();
        let menu_path = row.rel_path.clone();
        let kind = row.kind;
        let drag_target = crate::tree_menu::TreeTarget {
            path: row.rel_path.clone(),
            is_folder,
        };
        let drop_dest = if is_folder {
            row.rel_path.clone()
        } else {
            crate::tree_menu::parent_dir(&row.rel_path)
        };
        let drop_dest_hover = drop_dest.clone();
        let drag_bg = self.palette.selection;

        div()
            .id(gpui::SharedString::from(format!(
                "tree-row:{}",
                row.rel_path
            )))
            .flex()
            .items_center()
            .gap_1p5()
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .pl(px(8. + 14. * row.depth as f32))
            .pr_2()
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
            .can_drop(move |dragged: &dyn std::any::Any, _window, _cx| {
                dragged
                    .downcast_ref::<crate::tree_menu::TreeTarget>()
                    .is_some_and(|drag| crate::tree_menu::drop_allowed(drag, &drop_dest_hover))
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
                if is_folder {
                    this.toggle_folder(&rel_path, cx);
                } else if kind.is_note() {
                    this.open_note(&rel_path, cx);
                } else {
                    // вложения в редакторе не открываются (SPEC §7.5)
                    this.status_message =
                        format!("{rel_path}: этот файл нельзя открыть в редакторе");
                    cx.notify();
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                    // клик по строке не должен всплывать к контейнеру
                    cx.stop_propagation();
                    this.open_tree_menu(
                        event.position,
                        Some(crate::tree_menu::TreeTarget {
                            path: menu_path.clone(),
                            is_folder,
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
                    .child(row.name.clone()),
            )
            .into_any_element()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, rel: &str, kind: EntryKind, children: Vec<VaultEntry>) -> VaultEntry {
        VaultEntry {
            name: name.to_string(),
            rel_path: rel.to_string(),
            kind,
            children,
        }
    }

    #[test]
    fn flatten_shows_only_expanded_branches() {
        let tree = vec![
            entry(
                "папка",
                "папка",
                EntryKind::Folder,
                vec![entry("файл.md", "папка/файл.md", EntryKind::Note, vec![])],
            ),
            entry("заметка.md", "заметка.md", EntryKind::Note, vec![]),
        ];

        // свёрнутое дерево: только верхний уровень
        let rows = flatten_tree(&tree, &HashSet::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rel_path, "папка");
        assert!(!rows[0].expanded);

        // раскрытая папка добавляет детей
        let mut expanded = HashSet::new();
        expanded.insert("папка".to_string());
        let rows = flatten_tree(&tree, &expanded);
        assert_eq!(rows.len(), 3);
        assert!(rows[0].expanded);
        assert_eq!(rows[1].rel_path, "папка/файл.md");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].rel_path, "заметка.md");
    }
}
