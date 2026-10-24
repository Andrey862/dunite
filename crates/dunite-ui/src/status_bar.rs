//! Статус-бар (SPEC §9.5).
//!
//! M1: корень хранилища, счётчики, позиция курсора, статусное сообщение.
//! Дальше: git-статус, состояние индекса.

use gpui::{div, prelude::*, px};

use crate::workspace::Workspace;

impl Workspace {
    pub(crate) fn render_status_bar(&self) -> impl IntoElement {
        let (note_label, cursor_label) = match self.tabs.get(self.active_tab) {
            Some(tab) => (
                format!(
                    "{}{}",
                    tab.note_id.as_str(),
                    if tab.editor.is_dirty() { " •" } else { "" }
                ),
                format!(
                    "Ln {}, Col {} · {} строк",
                    tab.editor.cursor_line() + 1,
                    tab.editor.cursor_col() + 1,
                    tab.editor.buffer().len_lines()
                ),
            ),
            None => ("—".to_string(), String::new()),
        };

        div()
            .flex()
            .items_center()
            .gap_4()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(self.palette.border)
            .bg(self.palette.status_bar)
            .text_size(px(11.))
            .text_color(self.palette.text_muted)
            .child(format!(
                "хранилище: {} · заметок: {} · тема: {}",
                self.vault.root().display(),
                self.vault.note_count(),
                self.settings.theme.name()
            ))
            .child(self.git_status_text())
            .child(note_label)
            .child(cursor_label)
            .child(self.status_message.clone())
    }
}
