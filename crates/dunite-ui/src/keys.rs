//! Привязка клавиш (SPEC §9.4). Контекст `editor` — внутри панели редактора.

use gpui::KeyBinding;

use crate::actions::*;

macro_rules! editor_binding {
    ($keys:expr, $action:expr) => {
        KeyBinding::new($keys, $action, Some("editor"))
    };
}

/// Клавиатурные привязки панели редактора.
pub fn key_bindings() -> Vec<KeyBinding> {
    vec![
        // курсор
        editor_binding!("left", CursorLeft),
        editor_binding!("right", CursorRight),
        editor_binding!("up", CursorUp),
        editor_binding!("down", CursorDown),
        editor_binding!("ctrl-left", CursorWordLeft),
        editor_binding!("ctrl-right", CursorWordRight),
        editor_binding!("home", CursorLineStart),
        editor_binding!("end", CursorLineEnd),
        editor_binding!("pageup", CursorPageUp),
        editor_binding!("pagedown", CursorPageDown),
        editor_binding!("ctrl-home", CursorDocStart),
        editor_binding!("ctrl-end", CursorDocEnd),
        // выделение
        editor_binding!("shift-left", SelectLeft),
        editor_binding!("shift-right", SelectRight),
        editor_binding!("shift-up", SelectUp),
        editor_binding!("shift-down", SelectDown),
        editor_binding!("ctrl-shift-left", SelectWordLeft),
        editor_binding!("ctrl-shift-right", SelectWordRight),
        editor_binding!("shift-home", SelectLineStart),
        editor_binding!("shift-end", SelectLineEnd),
        editor_binding!("shift-pageup", SelectPageUp),
        editor_binding!("shift-pagedown", SelectPageDown),
        editor_binding!("ctrl-shift-home", SelectDocStart),
        editor_binding!("ctrl-shift-end", SelectDocEnd),
        editor_binding!("ctrl-a", SelectAll),
        editor_binding!("escape", CancelSelection),
        // правки
        editor_binding!("backspace", DeleteBackward),
        editor_binding!("delete", DeleteForward),
        editor_binding!("enter", InsertNewline),
        editor_binding!("tab", InsertTab),
        editor_binding!("ctrl-z", Undo),
        editor_binding!("ctrl-shift-z", Redo),
        editor_binding!("ctrl-y", Redo),
        // буфер обмена
        editor_binding!("ctrl-c", Copy),
        editor_binding!("ctrl-x", Cut),
        editor_binding!("ctrl-v", Paste),
    ]
}

/// Привязки вне контекста редактора (управление вкладками, выход).
pub fn global_key_bindings() -> Vec<KeyBinding> {
    use crate::actions::{
        CloseTab, NavBack, NavForward, NextTab, OpenSearch, OpenSwitcher, PrevTab, Quit, RenameNote,
    };
    vec![
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("ctrl-q", Quit, None),
        // палитра и навигация (SPEC §9.4, §8.3)
        KeyBinding::new("ctrl-p", OpenSwitcher, None),
        KeyBinding::new("ctrl-shift-f", OpenSearch, None),
        KeyBinding::new("f2", RenameNote, None),
        KeyBinding::new("alt-left", NavBack, None),
        KeyBinding::new("alt-right", NavForward, None),
    ]
}

/// Привязки открытой палитры (контекст `palette`).
pub fn palette_key_bindings() -> Vec<KeyBinding> {
    use crate::actions::{
        PaletteBackspace, PaletteCancel, PaletteConfirm, PaletteNext, PalettePrev,
    };
    vec![
        KeyBinding::new("enter", PaletteConfirm, Some("palette")),
        KeyBinding::new("escape", PaletteCancel, Some("palette")),
        KeyBinding::new("down", PaletteNext, Some("palette")),
        KeyBinding::new("up", PalettePrev, Some("palette")),
        KeyBinding::new("backspace", PaletteBackspace, Some("palette")),
    ]
}
