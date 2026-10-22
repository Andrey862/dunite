//! Модель хранилища (SPEC §7).
//!
//! Хранилище — директория с markdown-файлами; состояние выводится из файлов.
//! M0: сканирование дерева, чтение заметок. Дальше — frontmatter, ссылки,
//! теги, вложения, watcher (M2/M3).

mod id;
pub mod watch;

pub use id::NoteId;

use std::io;
use std::path::{Path, PathBuf};

/// Тип записи в дереве хранилища.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    /// `.md` / `.markdown`
    Note,
    /// Поддерживаемые изображения (SPEC §7.5)
    Image,
    /// Прочие файлы (видимы в дереве, не индексируются)
    Other,
}

impl EntryKind {
    pub fn is_folder(self) -> bool {
        self == EntryKind::Folder
    }

    pub fn is_note(self) -> bool {
        self == EntryKind::Note
    }
}

/// Запись дерева хранилища.
#[derive(Debug, Clone)]
pub struct VaultEntry {
    /// Имя файла/папки.
    pub name: String,
    /// Относительный путь с `/`-разделителями (он же `NoteId` для заметок).
    pub rel_path: String,
    pub kind: EntryKind,
    /// Дочерние записи (для папок), отсортированы: папки, затем файлы.
    pub children: Vec<VaultEntry>,
}

impl VaultEntry {
    pub fn is_folder(&self) -> bool {
        self.kind.is_folder()
    }

    pub fn note_id(&self) -> NoteId {
        NoteId::new(self.rel_path.clone())
    }
}

/// Открытое хранилище.
#[derive(Debug)]
pub struct Vault {
    root: PathBuf,
    tree: Vec<VaultEntry>,
    note_count: usize,
    total_size: u64,
}

impl Vault {
    /// Открывает хранилище в `root` и сканирует дерево.
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} не является директорией", root.display()),
            ));
        }
        let mut vault = Self {
            root,
            tree: Vec::new(),
            note_count: 0,
            total_size: 0,
        };
        vault.rescan()?;
        Ok(vault)
    }

    /// Пересканирует дерево с диска.
    pub fn rescan(&mut self) -> io::Result<()> {
        let mut stats = ScanStats::default();
        self.tree = scan_dir(&self.root, "", &mut stats)?;
        self.note_count = stats.note_count;
        self.total_size = stats.total_size;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn tree(&self) -> &[VaultEntry] {
        &self.tree
    }

    pub fn note_count(&self) -> usize {
        self.note_count
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Абсолютный путь по относительному (для операций с ФС).
    pub fn absolute(&self, rel_path: &str) -> PathBuf {
        self.root.join(rel_path)
    }

    /// Читает заметку как UTF-8 текст.
    ///
    /// Битая кодировка не роняет приложение: невалидные байты заменяются
    /// U+FFFD (SPEC §14 «не падать»).
    pub fn read_note(&self, rel_path: &str) -> io::Result<String> {
        let bytes = std::fs::read(self.absolute(rel_path))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Все заметки хранилища в порядке дерева.
    pub fn iter_notes(&self) -> impl Iterator<Item = &VaultEntry> {
        fn walk<'a>(entries: &'a [VaultEntry], out: &mut Vec<&'a VaultEntry>) {
            for entry in entries {
                if entry.is_folder() {
                    walk(&entry.children, out);
                } else if entry.kind.is_note() {
                    out.push(entry);
                }
            }
        }
        let mut out = Vec::with_capacity(self.note_count);
        walk(&self.tree, &mut out);
        out.into_iter()
    }

    /// Ищет заметку по имени файла (без пути и расширения).
    ///
    /// Упрощённый резолвинг ссылок для M2; полный (пути, алиасы, короткие
    /// имена) — M3 (SPEC §7.2). Регистронезависимо.
    pub fn find_note_by_name(&self, name: &str) -> Option<NoteId> {
        let name_lower = name.to_lowercase();
        self.iter_notes()
            .find(|entry| {
                NoteId::new(entry.rel_path.clone())
                    .file_stem()
                    .to_lowercase()
                    == name_lower
            })
            .map(|entry| entry.note_id())
    }

    /// Ищет файл (в т.ч. вложение) по имени — упрощённый резолвинг embeds.
    pub fn find_file_by_name(&self, name: &str) -> Option<String> {
        fn walk<'a>(entries: &'a [VaultEntry], name: &str) -> Option<&'a VaultEntry> {
            for entry in entries {
                if entry.name == name {
                    return Some(entry);
                }
                if entry.is_folder() {
                    if let Some(found) = walk(&entry.children, name) {
                        return Some(found);
                    }
                }
            }
            None
        }
        walk(&self.tree, name).map(|entry| entry.rel_path.clone())
    }
}

#[derive(Default)]
struct ScanStats {
    note_count: usize,
    total_size: u64,
}

fn scan_dir(dir: &Path, rel_prefix: &str, stats: &mut ScanStats) -> io::Result<Vec<VaultEntry>> {
    let mut folders = Vec::new();
    let mut files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_hidden(&name) {
            continue;
        }
        let rel_path = if rel_prefix.is_empty() {
            name.clone()
        } else {
            format!("{rel_prefix}/{name}")
        };
        let path = entry.path();
        // is_dir()/is_file() следуют симлинкам; битые симлинки просто пропускаем
        if path.is_dir() {
            let children = scan_dir(&path, &rel_path, stats)?;
            folders.push(VaultEntry {
                name,
                rel_path,
                kind: EntryKind::Folder,
                children,
            });
        } else if path.is_file() {
            let kind = classify(&name);
            if let Ok(meta) = entry.metadata() {
                stats.total_size += meta.len();
            }
            if kind.is_note() {
                stats.note_count += 1;
            }
            files.push(VaultEntry {
                name,
                rel_path,
                kind,
                children: Vec::new(),
            });
        }
    }

    let by_name = |a: &VaultEntry, b: &VaultEntry| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    };
    folders.sort_by(by_name);
    files.sort_by(by_name);
    folders.extend(files);
    Ok(folders)
}

/// Имена, скрытые от дерева/индексации (SPEC §7.6): служебные каталоги
/// и dot-файлы.
fn is_hidden(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db") || name.starts_with('.')
}

fn classify(name: &str) -> EntryKind {
    let lower = name.to_ascii_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "md" | "markdown" => EntryKind::Note,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => EntryKind::Image,
        _ => EntryKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("projects/inner")).unwrap();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("beta.md"), "b\n").unwrap();
        fs::write(root.join("Alpha.md"), "привет\n").unwrap();
        fs::write(root.join("projects/gamma.md"), "g\n").unwrap();
        fs::write(root.join("projects/inner/delta.md"), "d\n").unwrap();
        fs::write(root.join("projects/pic.png"), [0u8; 8]).unwrap();
        fs::write(root.join("readme.txt"), "t\n").unwrap();
        fs::write(root.join(".obsidian/app.json"), "{}").unwrap();
        fs::write(root.join(".git/config"), "").unwrap();
        let vault = Vault::open(root).unwrap();
        (dir, vault)
    }

    #[test]
    fn scan_counts_and_order() {
        let (_dir, vault) = fixture_vault();
        assert_eq!(vault.note_count(), 4);

        // папки первыми, внутри — по имени без учёта регистра
        let names: Vec<&str> = vault.tree().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["projects", "Alpha.md", "beta.md", "readme.txt"]);

        let projects = &vault.tree()[0];
        let child_names: Vec<&str> = projects.children.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(child_names, vec!["inner", "gamma.md", "pic.png"]);
    }

    #[test]
    fn hidden_dirs_excluded() {
        let (_dir, vault) = fixture_vault();
        assert!(
            vault
                .tree()
                .iter()
                .all(|e| e.name != ".git" && e.name != ".obsidian")
        );
    }

    #[test]
    fn read_note_and_iter() {
        let (_dir, vault) = fixture_vault();
        assert_eq!(vault.read_note("Alpha.md").unwrap(), "привет\n");
        assert_eq!(vault.read_note("projects/inner/delta.md").unwrap(), "d\n");

        let notes: Vec<&str> = vault.iter_notes().map(|e| e.rel_path.as_str()).collect();
        assert_eq!(
            notes,
            vec![
                "projects/inner/delta.md",
                "projects/gamma.md",
                "Alpha.md",
                "beta.md"
            ]
        );
    }

    #[test]
    fn open_rejects_nonexistent() {
        assert!(Vault::open("/definitely/not/here").is_err());
    }
}
