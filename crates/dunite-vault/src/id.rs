//! Идентификатор заметки (SPEC §7).

use std::fmt;
use std::path::Path;

/// Идентификатор заметки — относительный путь от корня хранилища
/// с `/`-разделителями, например `projects/alpha.md`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoteId(String);

impl NoteId {
    pub fn new(rel_path: impl Into<String>) -> Self {
        Self(rel_path.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Имя файла без расширения — заголовок заметки по умолчанию (SPEC §7.3).
    pub fn file_stem(&self) -> &str {
        Path::new(&self.0)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&self.0)
    }

    /// Родительская папка (`None` для заметок в корне).
    pub fn parent(&self) -> Option<&str> {
        match self.0.rfind('/') {
            Some(ix) => Some(&self.0[..ix]),
            None => None,
        }
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for NoteId {
    fn from(rel_path: &str) -> Self {
        Self::new(rel_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stem_and_parent() {
        let id = NoteId::new("projects/alpha.md");
        assert_eq!(id.file_stem(), "alpha");
        assert_eq!(id.parent(), Some("projects"));

        let id = NoteId::new("beta.md");
        assert_eq!(id.file_stem(), "beta");
        assert_eq!(id.parent(), None);
    }
}
