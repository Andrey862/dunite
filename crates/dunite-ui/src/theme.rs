//! Палитра интерфейса: две встроенные темы — light и dark (SPEC §12).

use dunite_settings::Theme;
use gpui::{Hsla, rgb};

/// Цвета интерфейса активной темы.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Hsla,
    pub panel: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub hover: Hsla,
    pub tab_bar: Hsla,
    pub tab_active: Hsla,
    pub status_bar: Hsla,
    /// Фон выделения текста.
    pub selection: Hsla,
    /// Цвет курсора.
    pub cursor: Hsla,
    /// Фон колонки нумерации строк.
    pub gutter: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Palette {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                background: c(0xfafafa),
                panel: c(0xf1f1f1),
                border: c(0xd0d0d0),
                text: c(0x1f1f1f),
                text_muted: c(0x8a8a8a),
                accent: c(0x2b6cb0),
                hover: c(0xe3e3e3),
                tab_bar: c(0xe9e9e9),
                tab_active: c(0xfafafa),
                status_bar: c(0xe9e9e9),
                selection: c(0xbdd7ff),
                cursor: c(0x1f1f1f),
                gutter: c(0xf3f3f3),
            },
            Theme::Dark => Self {
                background: c(0x1e1e1e),
                panel: c(0x252526),
                border: c(0x3c3c3c),
                text: c(0xdcdcdc),
                text_muted: c(0x8a8a8a),
                accent: c(0x4f9ee8),
                hover: c(0x2f2f30),
                tab_bar: c(0x252526),
                tab_active: c(0x1e1e1e),
                status_bar: c(0x252526),
                selection: c(0x264f78),
                cursor: c(0xdcdcdc),
                gutter: c(0x232323),
            },
        }
    }
}
