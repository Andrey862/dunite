//! Буфер текста (SPEC §8.1).
//!
//! Ropey-буфер с char-смещениями, версией и отслеживанием «грязного»
//! состояния для автосейва (SPEC §10.3).

use ropey::Rope;
use std::ops::Range;

/// Текстовый буфер заметки.
///
/// Все смещения — в символах (`char`), это нативные индексы ropey.
#[derive(Debug, Clone)]
pub struct Buffer {
    rope: Rope,
    version: u64,
    saved_version: u64,
}

impl Buffer {
    pub fn from_text(text: impl AsRef<str>) -> Self {
        Self {
            rope: Rope::from_str(text.as_ref()),
            version: 0,
            saved_version: 0,
        }
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Число символов.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Число строк.
    ///
    /// Семантика ropey: текст, оканчивающийся на `\n`, содержит пустую
    /// последнюю строку (`"a\n"` → 2 строки). Это ожидаемо для редактора.
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Версия буфера; инкрементируется при каждой правке.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Есть ли несохранённые изменения (для индикатора вкладки и автосейва).
    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version
    }

    /// Пометить текущее состояние сохранённым.
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }

    /// Char-смещение начала строки.
    pub fn line_start(&self, line_ix: usize) -> usize {
        self.rope.line_to_char(line_ix.min(self.len_lines()))
    }

    /// Индекс строки для char-смещения.
    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.char_to_line(offset.min(self.len_chars()))
    }

    /// Полный диапазон строки — с завершающим переводом строки, если он есть.
    pub fn line_full_range(&self, line_ix: usize) -> Range<usize> {
        let start = self.line_start(line_ix);
        let end = self.line_start(line_ix + 1);
        start..end
    }

    /// Диапазон содержимого строки — без завершающего перевода строки.
    pub fn line_content_range(&self, line_ix: usize) -> Range<usize> {
        let full = self.line_full_range(line_ix);
        let text = self.slice(full.clone());
        let mut end = full.end;
        let mut tail = text.as_str();
        while tail.ends_with('\n') || tail.ends_with('\r') {
            tail = &tail[..tail.len() - 1];
            end -= 1;
        }
        full.start..end
    }

    /// Текст строки без завершающего перевода строки.
    pub fn line(&self, line_ix: usize) -> String {
        self.slice(self.line_content_range(line_ix))
    }

    /// Фрагмент текста по char-диапазону.
    pub fn slice(&self, range: Range<usize>) -> String {
        let start = range.start.min(self.len_chars());
        let end = range.end.clamp(start, self.len_chars());
        self.rope.slice(start..end).to_string()
    }

    /// Заменяет диапазон символов на `new_text`.
    ///
    /// `range` — в координатах ДО правки. Возвращает удалённый фрагмент.
    pub fn edit(&mut self, range: Range<usize>, new_text: &str) -> String {
        let start = range.start.min(self.len_chars());
        let end = range.end.clamp(start, self.len_chars());
        let deleted = self.slice(start..end);
        if end > start {
            self.rope.remove(start..end);
        }
        if !new_text.is_empty() {
            self.rope.insert(start, new_text);
        }
        self.version += 1;
        deleted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_ranges() {
        let buffer = Buffer::from_text("один\nдва\nтри\n");
        // ropey: после финального \n есть пустая последняя строка
        assert_eq!(buffer.len_lines(), 4);
        assert_eq!(buffer.line(0), "один");
        assert_eq!(buffer.line(2), "три");
        assert_eq!(buffer.line_content_range(0), 0..4);
        assert_eq!(buffer.line_full_range(0), 0..5);
        assert_eq!(buffer.line_full_range(3), 13..13);
    }

    #[test]
    fn crlf_stripped() {
        let buffer = Buffer::from_text("a\r\nb\r\n");
        assert_eq!(buffer.line(0), "a");
        assert_eq!(buffer.line(1), "b");
        assert_eq!(buffer.line_content_range(0), 0..1);
    }

    #[test]
    fn edit_insert_replace_delete() {
        let mut buffer = Buffer::from_text("hello world");
        buffer.edit(5..5, ",");
        assert_eq!(buffer.text(), "hello, world");
        buffer.edit(0..5, "goodbye");
        assert_eq!(buffer.text(), "goodbye, world");
        buffer.edit(7..14, "");
        assert_eq!(buffer.text(), "goodbye");
        assert!(buffer.is_dirty());
    }

    #[test]
    fn edit_returns_deleted() {
        let mut buffer = Buffer::from_text("abcdef");
        assert_eq!(buffer.edit(1..4, "X"), "bcd");
        assert_eq!(buffer.text(), "aXef");
    }

    #[test]
    fn edit_out_of_bounds_clamped() {
        let mut buffer = Buffer::from_text("ab");
        buffer.edit(5..10, "x");
        assert_eq!(buffer.text(), "abx");
    }

    #[test]
    fn version_and_saved() {
        let mut buffer = Buffer::from_text("a");
        assert!(!buffer.is_dirty());
        buffer.edit(1..1, "b");
        assert!(buffer.is_dirty());
        buffer.mark_saved();
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn empty_buffer() {
        let buffer = Buffer::from_text("");
        assert!(buffer.is_empty());
        assert_eq!(buffer.line(0), "");
    }
}
