//! Буфер текста (SPEC §8.1).

use ropey::Rope;

/// Текстовый буфер заметки.
///
/// M0 — read-only снапшот: rope даёт дешёвый доступ к строкам для
/// виртуализированного рендера. Редактирование появится в M1.
#[derive(Debug, Clone)]
pub struct Buffer {
    rope: Rope,
}

impl Buffer {
    pub fn from_text(text: impl AsRef<str>) -> Self {
        Self {
            rope: Rope::from_str(text.as_ref()),
        }
    }

    /// Число строк.
    ///
    /// Семантика ropey: текст, оканчивающийся на `\n`, содержит пустую
    /// последнюю строку (`"a\n"` → 2 строки). Это ожидаемо для редактора.
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// Строка по индексу без завершающих `\r`/`\n`.
    pub fn line(&self, index: usize) -> String {
        if index >= self.rope.len_lines() {
            return String::new();
        }
        let mut line = self.rope.line(index).to_string();
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        line
    }

    /// Весь текст (для тестов и будущей сериализации).
    pub fn text(&self) -> String {
        self.rope.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_lengths() {
        let buffer = Buffer::from_text("один\nдва\nтри\n");
        // ropey: после финального \n есть пустая последняя строка
        assert_eq!(buffer.len_lines(), 4);
        assert_eq!(buffer.line(0), "один");
        assert_eq!(buffer.line(2), "три");
        assert_eq!(buffer.line(3), "");
        assert_eq!(buffer.text(), "один\nдва\nтри\n");
    }

    #[test]
    fn crlf_stripped() {
        let buffer = Buffer::from_text("a\r\nb\r\n");
        assert_eq!(buffer.line(0), "a");
        assert_eq!(buffer.line(1), "b");
    }

    #[test]
    fn empty_buffer() {
        let buffer = Buffer::from_text("");
        assert!(buffer.is_empty());
        assert_eq!(buffer.line(0), "");
    }
}
