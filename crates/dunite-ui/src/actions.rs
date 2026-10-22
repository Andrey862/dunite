//! Действия и горячие клавиши (SPEC §9.4).

use gpui::actions;

actions!(dunite, [CloseTab, NextTab, PrevTab, Quit]);

actions!(
    dunite,
    [
        // движения курсора
        CursorLeft,
        CursorRight,
        CursorUp,
        CursorDown,
        CursorWordLeft,
        CursorWordRight,
        CursorLineStart,
        CursorLineEnd,
        CursorPageUp,
        CursorPageDown,
        CursorDocStart,
        CursorDocEnd,
        // движения с расширением выделения
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectLineStart,
        SelectLineEnd,
        SelectPageUp,
        SelectPageDown,
        SelectDocStart,
        SelectDocEnd,
        SelectAll,
        CancelSelection,
        // правки
        DeleteBackward,
        DeleteForward,
        InsertNewline,
        InsertTab,
        Undo,
        Redo,
        // буфер обмена
        Copy,
        Cut,
        Paste,
    ]
);

actions!(
    dunite,
    [
        // палитра: quick switcher и переименование (SPEC §9.4)
        OpenSwitcher,
        RenameNote,
        // навигация back/forward (SPEC §8.3)
        NavBack,
        NavForward,
    ]
);

actions!(
    dunite,
    [
        // действия открытой палитры (контекст `palette`)
        PaletteConfirm,
        PaletteCancel,
        PaletteNext,
        PalettePrev,
        PaletteBackspace,
    ]
);
