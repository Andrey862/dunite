//! Панель вкладок (SPEC §9.2).
//!
//! M0: переключение и закрытие кликом. Дальше: reorder, pin, закрытие колёсиком.

use gpui::{Context, div, prelude::*, px};

use crate::workspace::Workspace;

impl Workspace {
    pub(crate) fn render_tab_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .h(px(34.))
            .border_b_1()
            .border_color(self.palette.border)
            .bg(self.palette.tab_bar)
            .children(self.tabs.iter().enumerate().map(|(ix, tab)| {
                let is_active = ix == self.active_tab;
                let title = tab.note_id.file_stem().to_string();
                let palette = self.palette;

                div()
                    .id(("tab", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_r_1()
                    .border_color(palette.border)
                    .cursor_pointer()
                    .bg(if is_active {
                        palette.tab_active
                    } else {
                        palette.tab_bar
                    })
                    .on_click(
                        cx.listener(move |this, _event, _window, cx| this.activate_tab(ix, cx)),
                    )
                    .child(div().text_size(px(13.)).child(title))
                    .child(
                        div()
                            .id(("tab-close", ix))
                            .px_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(palette.text_muted)
                            .hover(|style| style.bg(palette.hover))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.close_tab_at(ix, cx)
                            }))
                            .child("×"),
                    )
            }))
    }
}
