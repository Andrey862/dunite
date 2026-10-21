//! Свойства модели редактора (SPEC §15).

use dunite_editor::Editor;
use proptest::prelude::*;

proptest! {
    /// Любая серия вставок полностью откатывается до исходного текста.
    #[test]
    fn undo_restores_original(
        initial in ".*",
        ops in prop::collection::vec((0usize..64, ".{0,8}"), 0..12),
    ) {
        let mut editor = Editor::from_text(&initial);
        for (pos, text) in ops {
            editor.set_cursor(pos, false);
            editor.insert_text(&text);
        }
        while editor.can_undo() {
            editor.undo();
        }
        prop_assert_eq!(editor.text(), initial);
    }

    /// Откат и повтор всех правок возвращает итоговый текст.
    #[test]
    fn undo_redo_is_lossless(
        initial in ".*",
        ops in prop::collection::vec((0usize..64, ".{0,8}"), 0..12),
    ) {
        let mut editor = Editor::from_text(&initial);
        for (pos, text) in ops {
            editor.set_cursor(pos, false);
            editor.insert_text(&text);
        }
        let expected = editor.text();
        while editor.can_undo() {
            editor.undo();
        }
        while editor.can_redo() {
            editor.redo();
        }
        prop_assert_eq!(editor.text(), expected);
    }
}
