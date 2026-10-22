//! Метаданные заметки (SPEC §7.3).

use std::ops::Range;

use dunite_vault::NoteId;

/// Вид ссылки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `[[Заметка]]`
    Wiki,
    /// `![[embed]]` — вложение или встраивание
    Embed,
    /// `[текст](target)`
    Markdown,
}

/// Заголовок заметки (для outline и навигации).
#[derive(Debug, Clone)]
pub struct NoteHeading {
    pub level: u8,
    pub text: String,
    /// Char-смещение начала строки заголовка.
    pub offset: usize,
}

/// Исходящая ссылка заметки.
#[derive(Debug, Clone)]
pub struct ResolvedLink {
    /// Текст цели: `[[target|display]]` → `target`.
    pub target_text: String,
    /// Char-диапазон текста цели — для перезаписи при переименовании.
    pub target_range: Range<usize>,
    pub display: Option<String>,
    /// Char-диапазон ссылки в исходнике.
    pub range: Range<usize>,
    /// Номер строки (0-based) — позиция для навигации.
    pub line: usize,
    /// Текст строки со ссылкой — контекст для панели backlinks.
    pub context: String,
    pub kind: LinkKind,
}

/// Метаданные заметки.
#[derive(Debug, Clone)]
pub struct NoteMeta {
    pub id: NoteId,
    /// `title` из frontmatter или имя файла (SPEC §7.3).
    pub title: String,
    pub aliases: Vec<String>,
    /// Теги: frontmatter + inline `#теги`, в нижнем регистре.
    pub tags: Vec<String>,
    pub headings: Vec<NoteHeading>,
    pub links: Vec<ResolvedLink>,
}
