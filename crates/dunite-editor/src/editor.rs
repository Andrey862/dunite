//! Редактор (SPEC §8): буфер + выделение + история правок.
//!
//! Модель не знает о gpui: все операции — над char-смещениями.
//! UI вызывает методы [`Editor`] и рендерит состояние.

use std::ops::Range;
use std::time::Instant;

use unicode_segmentation::UnicodeSegmentation;

use crate::auto_continue::{EnterPlan, plan_enter};
use crate::buffer::Buffer;
use crate::history::{Edit, EditKind, History, Transaction};
use crate::movement::{self, Movement};
use crate::selection::Selection;

/// Состояние редактора заметки.
pub struct Editor {
    buffer: Buffer,
    selection: Selection,
    history: History,
    preferred_col: Option<usize>,
}

impl Editor {
    pub fn from_text(text: impl AsRef<str>) -> Self {
        Self {
            buffer: Buffer::from_text(text),
            selection: Selection::collapsed(0),
            history: History::new(),
            preferred_col: None,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn version(&self) -> u64 {
        self.buffer.version()
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    pub fn mark_saved(&mut self) {
        self.buffer.mark_saved();
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub fn selected_text(&self) -> String {
        let range = self.selection.range();
        if range.is_empty() {
            String::new()
        } else {
            self.buffer.slice(range)
        }
    }

    /// Строка курсора (0-based).
    pub fn cursor_line(&self) -> usize {
        self.buffer.line_of(self.selection.head)
    }

    /// Колонка курсора (в символах, от начала содержимого строки).
    pub fn cursor_col(&self) -> usize {
        let content = self.buffer.line_content_range(self.cursor_line());
        self.selection.head.clamp(content.start, content.end) - content.start
    }

    // ── правки ────────────────────────────────────────────────────────────

    /// Применяет правку и записывает её в историю.
    fn apply(&mut self, range: Range<usize>, new_text: &str, kind: EditKind) {
        let selection_before = self.selection;
        let deleted = self.buffer.edit(range.clone(), new_text);
        let edit = Edit {
            range,
            inserted: new_text.to_string(),
            deleted,
        };
        let cursor = edit.range.start + edit.inserted_chars();
        self.selection = Selection::collapsed(cursor);
        self.preferred_col = None;
        self.history.push(Transaction {
            edits: vec![edit],
            selection_before,
            selection_after: self.selection,
            kind,
            timestamp: Instant::now(),
        });
    }

    /// Ввод текста (заменяет выделение); группируется в undo по времени.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let range = self.selection.range();
        self.apply(range, text, EditKind::Typing);
    }

    /// Вставка из буфера обмена (не группируется с набором).
    pub fn insert_pasted_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let range = self.selection.range();
        self.apply(range, text, EditKind::Other);
    }

    /// Enter: перевод строки с авто-продолжением списков/цитат.
    pub fn insert_newline(&mut self) {
        let insert_at = self.selection.range().start;
        let line = self.buffer.line_of(insert_at);
        let content_start = self.buffer.line_content_range(line).start;
        let before = self.buffer.slice(content_start..insert_at);

        match plan_enter(&before) {
            EnterPlan::Newline => {
                let range = self.selection.range();
                self.apply(range, "\n", EditKind::Typing);
            }
            EnterPlan::Continue(prefix) => {
                let range = self.selection.range();
                let text = format!("\n{prefix}");
                self.apply(range, &text, EditKind::Typing);
            }
            EnterPlan::ExitItem { delete_chars } => {
                if self.selection.is_empty() {
                    let marker_end = content_start + delete_chars;
                    self.apply(content_start..marker_end, "", EditKind::Typing);
                } else {
                    let range = self.selection.range();
                    self.apply(range, "\n", EditKind::Typing);
                }
            }
        }
    }

    /// Tab: два пробела (отступ списка — M2).
    pub fn insert_tab(&mut self) {
        let range = self.selection.range();
        self.apply(range, "  ", EditKind::Typing);
    }

    /// Backspace: выделение или символ (в начале строки склеивает строки).
    pub fn delete_backward(&mut self) {
        if !self.selection.is_empty() {
            let range = self.selection.range();
            self.apply(range, "", EditKind::Delete);
            return;
        }
        let head = self.selection.head;
        if head == 0 {
            return;
        }
        let line = self.buffer.line_of(head);
        let content = self.buffer.line_content_range(line);
        if head > content.start {
            let col = head - content.start;
            let line_text = self.buffer.line(line);
            let prev = prev_grapheme_col(&line_text, col);
            self.apply(content.start + prev..head, "", EditKind::Delete);
        } else {
            // в начале строки: удаляем перевод строки (с учётом \r\n)
            let prev_content = self.buffer.line_content_range(line - 1);
            self.apply(prev_content.end..head, "", EditKind::Delete);
        }
    }

    /// Delete: выделение или символ справа (в конце строки склеивает строки).
    pub fn delete_forward(&mut self) {
        if !self.selection.is_empty() {
            let range = self.selection.range();
            self.apply(range, "", EditKind::Delete);
            return;
        }
        let head = self.selection.head;
        let line = self.buffer.line_of(head);
        let content = self.buffer.line_content_range(line);
        if head < content.end {
            let col = head - content.start;
            let line_text = self.buffer.line(line);
            let next = next_grapheme_col(&line_text, col);
            self.apply(head..content.start + next, "", EditKind::Delete);
        } else if line + 1 < self.buffer.len_lines() {
            let next_content = self.buffer.line_content_range(line + 1);
            self.apply(head..next_content.start, "", EditKind::Delete);
        }
    }

    // ── курсор и выделение ────────────────────────────────────────────────

    pub fn move_cursor(&mut self, movement: Movement, extend: bool) {
        let (target, preferred_col) = movement::target(
            &self.buffer,
            self.selection.head,
            movement,
            self.preferred_col,
        );
        self.selection.move_to(target, extend);
        self.preferred_col = preferred_col;
        self.history.break_group();
    }

    pub fn set_cursor(&mut self, offset: usize, extend: bool) {
        let offset = offset.min(self.buffer.len_chars());
        self.selection.move_to(offset, extend);
        self.preferred_col = None;
        self.history.break_group();
    }

    /// Двойной клик: выделить «слово» под смещением.
    pub fn select_word_at(&mut self, offset: usize) {
        let line = self.buffer.line_of(offset);
        let content = self.buffer.line_content_range(line);
        let col = offset.clamp(content.start, content.end) - content.start;
        let line_text = self.buffer.line(line);
        let (start, end) = movement::word_range_at(&line_text, col);
        self.selection.anchor = content.start + start;
        self.selection.head = content.start + end;
        self.history.break_group();
    }

    /// Тройной клик: выделить строку целиком (с переводом строки).
    pub fn select_line_at(&mut self, offset: usize) {
        let line = self.buffer.line_of(offset);
        let full = self.buffer.line_full_range(line);
        self.selection.anchor = full.start;
        self.selection.head = full.end;
        self.history.break_group();
    }

    pub fn select_all(&mut self) {
        self.selection.anchor = 0;
        self.selection.head = self.buffer.len_chars();
        self.history.break_group();
    }

    /// Escape: свернуть выделение к курсору.
    pub fn collapse_to_head(&mut self) {
        self.selection.collapse_to_head();
        self.history.break_group();
    }

    // ── undo / redo ───────────────────────────────────────────────────────

    pub fn undo(&mut self) -> bool {
        let Some(txn) = self.history.pop_undo() else {
            return false;
        };
        for edit in txn.edits.iter().rev() {
            let inverse = edit.range.start..edit.range.start + edit.inserted_chars();
            self.buffer.edit(inverse, &edit.deleted);
        }
        self.selection = txn.selection_before;
        self.preferred_col = None;
        self.history.push_redo(txn);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(txn) = self.history.pop_redo() else {
            return false;
        };
        for edit in &txn.edits {
            self.buffer.edit(edit.range.clone(), &edit.inserted);
        }
        self.selection = txn.selection_after;
        self.preferred_col = None;
        self.history.push_undo(txn);
        true
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
}

fn prev_grapheme_col(line_text: &str, col: usize) -> usize {
    let prefix: String = line_text.chars().take(col).collect();
    prefix
        .graphemes(true)
        .last()
        .map(|g| col - g.chars().count())
        .unwrap_or(0)
}

fn next_grapheme_col(line_text: &str, col: usize) -> usize {
    let suffix: String = line_text.chars().skip(col).collect();
    suffix
        .graphemes(true)
        .next()
        .map(|g| col + g.chars().count())
        .unwrap_or(col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_cursor() {
        let mut editor = Editor::from_text("");
        editor.insert_text("привет");
        assert_eq!(editor.text(), "привет");
        assert_eq!(editor.cursor_line(), 0);
        assert_eq!(editor.cursor_col(), 6);
    }

    #[test]
    fn insert_replaces_selection() {
        let mut editor = Editor::from_text("hello world");
        editor.set_cursor(0, false);
        editor.set_cursor(5, true);
        editor.insert_text("hi");
        assert_eq!(editor.text(), "hi world");
        assert_eq!(editor.cursor_col(), 2);
    }

    #[test]
    fn delete_backward_joins_lines() {
        let mut editor = Editor::from_text("ab\ncd");
        editor.set_cursor(3, false);
        editor.delete_backward();
        assert_eq!(editor.text(), "abcd");
        assert_eq!(editor.cursor_col(), 2);
    }

    #[test]
    fn delete_forward_joins_lines() {
        let mut editor = Editor::from_text("ab\ncd");
        editor.set_cursor(2, false);
        editor.delete_forward();
        assert_eq!(editor.text(), "abcd");
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut editor = Editor::from_text("base");
        editor.set_cursor(4, false);
        editor.insert_pasted_text(" XYZ");
        editor.insert_text("abc");
        let edited = editor.text();
        assert!(editor.undo());
        assert_eq!(editor.text(), "base XYZ");
        assert!(editor.undo());
        assert_eq!(editor.text(), "base");
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(editor.text(), "base XYZ");
        assert!(editor.redo());
        assert_eq!(editor.text(), edited);
    }

    #[test]
    fn undo_restores_selection() {
        let mut editor = Editor::from_text("abc");
        editor.set_cursor(1, true);
        editor.select_all();
        editor.insert_text("x");
        assert!(editor.undo());
        assert_eq!(editor.selection().range(), 0..3);
    }

    #[test]
    fn newline_continues_bullet() {
        let mut editor = Editor::from_text("- пункт");
        editor.set_cursor(7, false);
        editor.insert_newline();
        assert_eq!(editor.text(), "- пункт\n- ");
        assert_eq!(editor.cursor_col(), 2);
    }

    #[test]
    fn newline_on_empty_item_exits_list() {
        let mut editor = Editor::from_text("- пункт\n- ");
        editor.set_cursor(10, false);
        editor.insert_newline();
        assert_eq!(editor.text(), "- пункт\n");
        assert_eq!(editor.cursor_line(), 1);
        assert_eq!(editor.cursor_col(), 0);
    }

    #[test]
    fn newline_continues_checkbox_unchecked() {
        let mut editor = Editor::from_text("- [x] сделано");
        editor.set_cursor(13, false);
        editor.insert_newline();
        assert_eq!(editor.text(), "- [x] сделано\n- [ ] ");
    }

    #[test]
    fn newline_continues_ordered_list() {
        let mut editor = Editor::from_text("1. один");
        editor.set_cursor(7, false);
        editor.insert_newline();
        assert_eq!(editor.text(), "1. один\n2. ");
    }

    #[test]
    fn double_click_selects_word() {
        let mut editor = Editor::from_text("hello world");
        editor.select_word_at(7);
        assert_eq!(editor.selected_text(), "world");
    }

    #[test]
    fn triple_click_selects_line() {
        let mut editor = Editor::from_text("ab\ncd\n");
        editor.select_line_at(3);
        assert_eq!(editor.selected_text(), "cd\n");
    }

    #[test]
    fn movement_with_selection() {
        let mut editor = Editor::from_text("hello world");
        editor.set_cursor(0, false);
        editor.move_cursor(Movement::WordRight, true);
        assert_eq!(editor.selected_text(), "hello ");
        editor.move_cursor(Movement::WordRight, true);
        assert_eq!(editor.selected_text(), "hello world");
    }
}
