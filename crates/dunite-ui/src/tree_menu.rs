//! Контекстное меню дерева файлов (SPEC §9.1).
//!
//! ПКМ по файлу/папке/пустой области — меню операций; имена и назначения
//! вводятся через палитру. Удаление безопасно: перед ним делается
//! git-снапшот (SPEC §10).

use gpui::{Context, div, prelude::*, px};

use crate::palette::PaletteIntent;
use crate::workspace::Workspace;

/// Цель контекстного меню.
#[derive(Debug, Clone)]
pub struct TreeTarget {
    pub path: String,
    pub is_folder: bool,
}

/// Открытое контекстное меню.
pub struct TreeMenuState {
    pub x: f32,
    pub y: f32,
    /// `None` — пустая область дерева (корень хранилища).
    pub target: Option<TreeTarget>,
}

/// Буфер обмена дерева (копировать/вырезать).
#[derive(Clone)]
pub struct TreeClip {
    pub path: String,
    pub cut: bool,
}

/// Пункты меню.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeAction {
    NewNote,
    NewFolder,
    Rename,
    Copy,
    Cut,
    Paste,
    MoveTo,
    Delete,
}

impl Workspace {
    /// Открывает контекстное меню в точке клика.
    pub(crate) fn open_tree_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        target: Option<TreeTarget>,
        cx: &mut Context<Self>,
    ) {
        self.tree_menu = Some(TreeMenuState {
            x: position.x.to_f64() as f32,
            y: position.y.to_f64() as f32,
            target,
        });
        cx.notify();
    }

    pub(crate) fn close_tree_menu(&mut self, cx: &mut Context<Self>) {
        if self.tree_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Оверлей контекстного меню (если открыто).
    pub(crate) fn render_tree_menu(&self, cx: &Context<Self>) -> Option<gpui::AnyElement> {
        let menu = self.tree_menu.as_ref()?;
        let is_folder = menu.target.as_ref().map(|t| t.is_folder).unwrap_or(true);
        let has_target = menu.target.is_some();

        let mut items: Vec<(TreeAction, &'static str)> = Vec::new();
        if !has_target {
            // корень хранилища
            items.push((TreeAction::NewNote, "Новая заметка"));
            items.push((TreeAction::NewFolder, "Новая папка"));
        } else if is_folder {
            items.push((TreeAction::NewNote, "Новая заметка"));
            items.push((TreeAction::NewFolder, "Новая папка"));
            items.push((TreeAction::Rename, "Переименовать…"));
            items.push((TreeAction::MoveTo, "Переместить в…"));
        } else {
            items.push((TreeAction::Rename, "Переименовать…"));
            items.push((TreeAction::MoveTo, "Переместить в…"));
        }
        if has_target {
            items.push((TreeAction::Copy, "Копировать"));
            items.push((TreeAction::Cut, "Вырезать"));
        }
        if self.tree_clipboard.is_some() {
            items.push((TreeAction::Paste, "Вставить"));
        }
        if has_target {
            items.push((TreeAction::Delete, "Удалить"));
        }

        let panel = div()
            .id("tree-menu")
            .flex()
            .flex_col()
            .min_w(px(180.))
            .py_1()
            .border_1()
            .border_color(self.palette.border)
            .rounded_md()
            .bg(self.palette.background)
            .shadow_lg()
            .children(items.into_iter().map(|(action, label)| {
                div()
                    .id(gpui::SharedString::from(format!("tree-menu-{label}")))
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .hover(|style| style.bg(self.palette.hover))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.tree_menu_action(action, window, cx);
                    }))
                    .child(div().text_size(px(12.)).child(label))
            }));

        Some(
            div()
                .id("tree-menu-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.close_tree_menu(cx);
                }))
                .child(
                    div()
                        .absolute()
                        .top(px(menu.y))
                        .left(px(menu.x))
                        .child(panel),
                )
                .into_any_element(),
        )
    }

    /// Выполняет пункт меню.
    fn tree_menu_action(
        &mut self,
        action: TreeAction,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.tree_menu.as_ref().and_then(|menu| menu.target.clone());
        self.close_tree_menu(cx);

        let (path, is_folder) = match &target {
            Some(target) => (target.path.clone(), target.is_folder),
            None => (String::new(), true),
        };
        // для файлов операции создания/вставки идут в родительскую папку
        let dir = if is_folder {
            path.clone()
        } else {
            parent_dir(&path)
        };

        match action {
            TreeAction::NewNote => self.open_palette(PaletteIntent::CreateNote { dir }, window, cx),
            TreeAction::NewFolder => {
                self.open_palette(PaletteIntent::CreateFolder { dir }, window, cx)
            }
            TreeAction::Rename => {
                if !path.is_empty() {
                    self.open_palette(PaletteIntent::RenameEntry { path }, window, cx);
                }
            }
            TreeAction::MoveTo => {
                if !path.is_empty() {
                    self.open_palette(PaletteIntent::MoveEntry { path }, window, cx);
                }
            }
            TreeAction::Copy => {
                self.tree_clipboard = Some(TreeClip {
                    path: path.clone(),
                    cut: false,
                });
                self.status_message = format!("скопировано: {path}");
            }
            TreeAction::Cut => {
                self.tree_clipboard = Some(TreeClip {
                    path: path.clone(),
                    cut: true,
                });
                self.status_message = format!("вырезано: {path}");
            }
            TreeAction::Paste => {
                let clip = self.tree_clipboard.clone();
                if let Some(clip) = clip {
                    self.tree_paste(&clip, &dir, cx);
                }
            }
            TreeAction::Delete => {
                if !path.is_empty() {
                    self.tree_delete(&path, cx);
                }
            }
        }
        cx.notify();
    }
}

/// Родительский каталог относительного пути (`""` для корня).
pub(crate) fn parent_dir(rel_path: &str) -> String {
    match rel_path.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// Разрешён ли drop: не в себя, не в собственную подпапку и не в ту же
/// папку (перетаскивание туда, где и так лежим, бессмысленно).
pub(crate) fn drop_allowed(drag: &TreeTarget, dest_dir: &str) -> bool {
    if drag.path == dest_dir {
        return false;
    }
    let own_subtree = format!("{}/", drag.path);
    if dest_dir.starts_with(&own_subtree) {
        return false;
    }
    parent_dir(&drag.path) != dest_dir
}
