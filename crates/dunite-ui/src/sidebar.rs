//! Правый сайдбар (SPEC §9.3): структура заметки, обратные ссылки, теги.

use dunite_index::NoteMeta;
use gpui::{Context, div, prelude::*, px};

use crate::workspace::Workspace;

impl Workspace {
    pub(crate) fn render_right_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(280.))
            .border_l_1()
            .border_color(self.palette.border)
            .bg(self.palette.panel)
            .child(self.render_outline(cx))
            .child(self.render_backlinks(cx))
            .child(self.render_tags(cx))
    }

    /// Заголовок секции.
    fn section_header(&self, title: &str) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .border_b_1()
            .border_t_1()
            .border_color(self.palette.border)
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(self.palette.text_muted)
                    .child(title.to_string()),
            )
    }

    /// Метаданные активной заметки.
    pub(crate) fn active_meta(&self) -> Option<&NoteMeta> {
        let tab = self.tabs.get(self.active_tab)?;
        self.graph.meta(&tab.note_id)
    }

    /// Структура: заголовки заметки, клик — переход.
    fn render_outline(&self, cx: &Context<Self>) -> impl IntoElement {
        let headings = self
            .active_meta()
            .map(|meta| meta.headings.clone())
            .unwrap_or_default();

        div()
            .flex()
            .flex_col()
            .child(self.section_header("Структура"))
            .child(
                div()
                    .id("outline")
                    .flex()
                    .flex_col()
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .children(headings.into_iter().map(|heading| {
                        let offset = heading.offset;
                        div()
                            .id(("outline-item", offset))
                            .pl(px(8. + 12. * heading.level.saturating_sub(1) as f32))
                            .pr_2()
                            .py_0p5()
                            .cursor_pointer()
                            .hover(|style| style.bg(self.palette.hover))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.goto_offset(offset, cx);
                            }))
                            .child(div().text_size(px(12.)).truncate().child(heading.text))
                    })),
            )
    }

    /// Обратные ссылки + неразрешённые ссылки заметки.
    fn render_backlinks(&self, cx: &Context<Self>) -> impl IntoElement {
        let active_id = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.note_id.clone());
        let backlinks = active_id
            .as_ref()
            .map(|id| self.graph.backlinks(id).to_vec())
            .unwrap_or_default();
        let unresolved: Vec<String> = self
            .active_meta()
            .map(|meta| {
                meta.links
                    .iter()
                    .filter(|link| {
                        // внешние URL и вложения — не ссылки на заметки
                        // (SPEC §7.2, §7.5)
                        let external = link.target_text.contains("://")
                            || link.target_text.starts_with("mailto:");
                        !external
                            && link.kind != dunite_index::LinkKind::Embed
                            && active_id.as_ref().is_some_and(|from| {
                                self.graph.resolve(from, &link.target_text).is_none()
                            })
                    })
                    .map(|link| link.target_text.clone())
                    .collect()
            })
            .unwrap_or_default();

        let mut content = div()
            .id("backlinks")
            .flex()
            .flex_col()
            .max_h(px(260.))
            .overflow_y_scroll()
            .children(backlinks.into_iter().map(|ref_| {
                let source = ref_.source.as_str().to_string();
                let source_label = ref_.source.file_stem().to_string();
                let context = ref_.context.clone();
                let line = ref_.line;
                div()
                    .id(gpui::SharedString::from(format!("backlink:{source}")))
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .hover(|style| style.bg(self.palette.hover))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.open_note_at_line(&source, line, cx);
                    }))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(source_label),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(self.palette.text_muted)
                            .truncate()
                            .child(context),
                    )
            }));

        if !unresolved.is_empty() {
            content = content.child(
                div()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(self.palette.border)
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(self.palette.text_muted)
                            .child("Неразрешённые ссылки"),
                    ),
            );
            content = content.children(unresolved.into_iter().map(|target| {
                div().px_3().py_0p5().child(
                    div()
                        .text_size(px(11.))
                        .text_color(self.palette.text_muted)
                        .truncate()
                        .child(target),
                )
            }));
        }

        div()
            .flex()
            .flex_col()
            .child(self.section_header("Обратные ссылки"))
            .child(content)
    }

    /// Теги хранилища: клик — заметки с тегом.
    fn render_tags(&self, cx: &Context<Self>) -> impl IntoElement {
        let tags = self.graph.tags();

        div()
            .flex()
            .flex_col()
            .child(self.section_header("Теги"))
            .child(
                div()
                    .id("tags")
                    .flex()
                    .flex_col()
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .children(tags.into_iter().map(|(tag, count)| {
                        let expanded = self.expanded_tags.contains(&tag);
                        let mut node = div().flex().flex_col().child(
                            div()
                                .id(gpui::SharedString::from(format!("tag:{tag}")))
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_3()
                                .py_0p5()
                                .cursor_pointer()
                                .hover(|style| style.bg(self.palette.hover))
                                .on_click(cx.listener({
                                    let tag = tag.clone();
                                    move |this, _event, _window, cx| {
                                        this.toggle_tag(&tag, cx);
                                    }
                                }))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(self.palette.accent)
                                        .truncate()
                                        .child(format!("#{tag}")),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(self.palette.text_muted)
                                        .child(format!("{count}")),
                                ),
                        );
                        if expanded {
                            let notes = self.graph.notes_with_tag(&tag);
                            node = node.children(notes.into_iter().map(|note_id| {
                                let rel_path = note_id.as_str().to_string();
                                let label = note_id.file_stem().to_string();
                                div()
                                    .id(gpui::SharedString::from(format!("tag-note:{rel_path}")))
                                    .pl(px(20.))
                                    .pr_2()
                                    .py_0p5()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(self.palette.hover))
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.open_note(&rel_path, cx);
                                    }))
                                    .child(div().text_size(px(11.)).truncate().child(label))
                            }));
                        }
                        node
                    })),
            )
    }

    // ── действия сайдбара ────────────────────────────────────────────────

    /// Переход к смещению в активной заметке.
    pub(crate) fn goto_offset(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor, _| editor.set_cursor(offset, false));
        self.scroll_cursor_into_view();
        cx.notify();
    }

    /// Открыть заметку и перейти к строке (для backlinks).
    pub(crate) fn open_note_at_line(
        &mut self,
        rel_path: &str,
        line: usize,
        cx: &mut Context<Self>,
    ) {
        self.open_note(rel_path, cx);
        let offset = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.editor.buffer().line_start(line))
            .unwrap_or(0);
        self.goto_offset(offset, cx);
    }

    /// Раскрыть/свернуть список заметок тега.
    pub(crate) fn toggle_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        if !self.expanded_tags.remove(tag) {
            self.expanded_tags.insert(tag.to_string());
        }
        cx.notify();
    }
}
