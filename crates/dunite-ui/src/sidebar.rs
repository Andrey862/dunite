//! Правый сайдбар (SPEC §9.3): структура заметки, обратные ссылки, теги.

use dunite_index::NoteMeta;
use gpui::{Context, div, prelude::*, px};

use crate::workspace::Workspace;

impl Workspace {
    /// Правая панель: структура, backlinks, теги, история (SPEC §9.3).
    /// Ширина — перетаскиваемой границей.
    pub(crate) fn render_right_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .child(
                div()
                    .id("panel-drag")
                    .w(px(4.))
                    .h_full()
                    .flex_shrink_0()
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .hover(|style| style.bg(self.palette.accent))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _window, cx| {
                            this.panel_drag_start(event, cx);
                        }),
                    )
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _event, _window, cx| this.panel_drag_end(cx)),
                    )
                    .on_mouse_up_out(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _event, _window, cx| this.panel_drag_end(cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(self.right_panel_width))
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(self.palette.border)
                    .bg(self.palette.panel)
                    .child(self.render_outline(cx))
                    .child(self.render_backlinks(cx))
                    .child(self.render_tags(cx))
                    .child(self.render_git_history(cx)),
            )
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

    // ── история git (SPEC §10.2) ────────────────────────────────────────

    /// Полоса remote: статус синхронизации и кнопки pull/push (SPEC §10.1).
    fn render_remote_strip(&self, cx: &Context<Self>) -> impl IntoElement {
        let status = self.git.as_ref().and_then(|git| git.remote_status());
        let mut actions = div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.git_button("git-pull", "pull", GitAction::Pull, cx))
            .child(self.git_button("git-push", "push", GitAction::Push, cx));
        if status.is_none() {
            actions = actions.child(self.git_button(
                "git-add-remote",
                "+ remote",
                GitAction::AddRemote,
                cx,
            ));
        }

        div()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(self.palette.border)
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(self.palette.text_muted)
                    .truncate()
                    .child(status.unwrap_or_else(|| "remote не настроен".to_string())),
            )
            .child(actions)
    }

    fn git_button(
        &self,
        id: &'static str,
        label: &'static str,
        action: GitAction,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|style| style.bg(self.palette.hover))
            .on_click(cx.listener(move |this, _event, window, cx| match action {
                GitAction::Pull => this.git_pull(cx),
                GitAction::Push => this.git_push(cx),
                GitAction::AddRemote => this.open_remote_setup(window, cx),
            }))
            .child(div().text_size(px(10.)).child(label))
    }

    /// История git: коммиты, diff-превью, восстановление заметки.
    fn render_git_history(&self, cx: &Context<Self>) -> impl IntoElement {
        let expanded = self.git_expanded.clone();

        let scope_chip = |label: &'static str, active: bool, scope_note: bool| {
            div()
                .id(gpui::SharedString::from(format!("git-scope-{label}")))
                .px_1p5()
                .rounded_md()
                .cursor_pointer()
                .bg(if active {
                    self.palette.hover
                } else {
                    self.palette.panel
                })
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.git_scope_note = scope_note;
                    this.git_expanded = None;
                    this.git_diff.clear();
                    this.refresh_git_caches();
                    cx.notify();
                }))
                .child(div().text_size(px(10.)).child(label))
        };

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_b_1()
                    .border_color(self.palette.border)
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(self.palette.text_muted)
                            .child("История (git)"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(scope_chip("Заметка", self.git_scope_note, true))
                            .child(scope_chip("Всё", !self.git_scope_note, false))
                            .child(
                                div()
                                    .id("git-snapshot-now")
                                    .px_1p5()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(self.palette.hover))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.save_and_snapshot(cx);
                                    }))
                                    .child(div().text_size(px(10.)).child("снапшот")),
                            ),
                    ),
            )
            .child(self.render_remote_strip(cx))
            .child(
                div()
                    .id("git-history")
                    .flex()
                    .flex_col()
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .children(self.git_log().iter().map(|commit| {
                        let is_open = expanded.as_deref() == Some(commit.id.as_str());
                        let commit_id = commit.id.clone();
                        let mut node = div().flex().flex_col().child(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "git-commit:{}",
                                    commit.id
                                )))
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_1()
                                .cursor_pointer()
                                .hover(|style| style.bg(self.palette.hover))
                                .on_click(cx.listener(move |this, _event, _window, cx| {
                                    this.toggle_git_diff(&commit_id, cx);
                                }))
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(self.palette.accent)
                                        .child(commit.short_id.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(self.palette.text_muted)
                                        .child(commit.time.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .truncate()
                                        .child(commit.summary.clone()),
                                ),
                        );
                        if is_open {
                            let commit_id_for_files = commit.id.clone();
                            let selected_file = self.git_diff_file.clone();
                            let restore_label = format!(
                                "восстановить {}",
                                selected_file
                                    .as_deref()
                                    .map(|file| file.rsplit('/').next().unwrap_or(file).to_string())
                                    .unwrap_or_else(|| "файл".to_string())
                            );
                            node = node.child(
                                div()
                                    .px_3()
                                    .pb_2()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    // файлы коммита: клик — выбрать для превью
                                    .children(self.git_diff_files.iter().map(|file| {
                                        let is_selected =
                                            selected_file.as_deref() == Some(file.as_str());
                                        let file_path = file.clone();
                                        let commit_for_click = commit_id_for_files.clone();
                                        let label =
                                            file.rsplit('/').next().unwrap_or(file).to_string();
                                        div()
                                            .id(gpui::SharedString::from(format!(
                                                "git-file:{file}"
                                            )))
                                            .px_1p5()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .bg(if is_selected {
                                                self.palette.hover
                                            } else {
                                                self.palette.panel
                                            })
                                            .on_click(cx.listener(
                                                move |this, _event, _window, cx| {
                                                    this.select_git_diff_file(
                                                        &commit_for_click,
                                                        &file_path,
                                                        cx,
                                                    );
                                                },
                                            ))
                                            .child(div().text_size(px(10.)).truncate().child(label))
                                    }))
                                    .children(self.git_diff.iter().map(|line| {
                                        let (color, prefix) = match line.origin {
                                            '+' => (self.palette.accent, "+"),
                                            '-' => (self.palette.text_muted, "-"),
                                            _ => (self.palette.text_muted, " "),
                                        };
                                        div()
                                            .flex()
                                            .gap_1()
                                            .text_size(px(10.))
                                            .font_family("monospace")
                                            .text_color(color)
                                            .child(prefix)
                                            .child(line.text.clone())
                                    }))
                                    .child(
                                        div()
                                            .id("git-restore")
                                            .mt_1()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(self.palette.border)
                                            .cursor_pointer()
                                            .hover(|style| style.bg(self.palette.hover))
                                            .on_click(cx.listener(|this, _event, _window, cx| {
                                                if let (Some(commit), Some(file)) = (
                                                    this.git_expanded.clone(),
                                                    this.git_diff_file.clone(),
                                                ) {
                                                    this.restore_from_commit(&commit, &file, cx);
                                                }
                                            }))
                                            .child(div().text_size(px(10.)).child(restore_label)),
                                    ),
                            );
                        }
                        node
                    })),
            )
    }
}

/// Действия кнопок remote-полосы.
#[derive(Clone, Copy)]
enum GitAction {
    Pull,
    Push,
    AddRemote,
}
