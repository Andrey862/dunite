//! Панель текста: read-only виртуализированный список строк (M0).
//!
//! M1 превратит её в редактор (курсоры, выделение, ввод), M2 — в Live Preview.

use gpui::{AnyElement, Context, div, prelude::*, px, uniform_list};

use crate::workspace::Workspace;

/// Высота строки в пикселях (uniform_list требует одинаковой высоты).
const LINE_HEIGHT_PX: f32 = 20.;

impl Workspace {
    pub(crate) fn render_text_area(&self, cx: &Context<Self>) -> AnyElement {
        if self.tabs.is_empty() {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(self.palette.text_muted)
                        .child("Откройте заметку в дереве файлов"),
                )
                .into_any_element();
        }

        let line_count = self.tabs[self.active_tab].buffer.len_lines();

        div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(
                uniform_list(
                    "text-pane",
                    line_count,
                    cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                        let palette = this.palette;
                        let buffer = &this.tabs[this.active_tab].buffer;
                        range
                            .map(|ix| {
                                div()
                                    .id(("line", ix))
                                    .flex()
                                    .h(px(LINE_HEIGHT_PX))
                                    .items_center()
                                    .child(
                                        div()
                                            .w(px(56.))
                                            .flex_shrink_0()
                                            .px_2()
                                            .text_size(px(11.))
                                            .text_color(palette.text_muted)
                                            .child(format!("{}", ix + 1)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .px_2()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .font_family("monospace")
                                            .text_size(px(13.))
                                            .child(buffer.line(ix)),
                                    )
                            })
                            .collect()
                    }),
                )
                .size_full(),
            )
            .into_any_element()
    }
}
