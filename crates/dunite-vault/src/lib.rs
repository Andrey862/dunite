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

    // ── файловые операции (SPEC §9.1) ────────────────────────────────────

    /// Создаёт пустую заметку в `rel_dir`; расширение `.md` добавляется.
    pub fn create_note(&self, rel_dir: &str, name: &str) -> io::Result<NoteId> {
        let name = ensure_extension(name.trim(), ".md");
        let dir = self.root.join(rel_dir);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&name);
        if path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{name} уже существует"),
            ));
        }
        std::fs::write(&path, "")?;
        Ok(NoteId::new(join_rel(rel_dir, &name)))
    }

    /// Создаёт папку в `rel_dir`.
    pub fn create_folder(&self, rel_dir: &str, name: &str) -> io::Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "пустое имя"));
        }
        let path = self.root.join(rel_dir).join(name);
        std::fs::create_dir_all(&path)?;
        Ok(join_rel(rel_dir, name))
    }

    /// Удаляет файл или папку (папку — рекурсивно).
    ///
    /// Безопасно: перед удалением UI делает git-снапшот (SPEC §10).
    pub fn delete_entry(&self, rel_path: &str) -> io::Result<()> {
        let path = self.root.join(rel_path);
        if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    }

    /// Копирует файл или папку (рекурсивно) в `dest_dir`.
    ///
    /// При конфликте имён добавляет «копия»/«копия N»; возвращает новый путь.
    pub fn copy_entry(&self, src_rel: &str, dest_dir: &str) -> io::Result<String> {
        let src = self.root.join(src_rel);
        let dest_dir_path = self.root.join(dest_dir);
        std::fs::create_dir_all(&dest_dir_path)?;
        let original_name = file_name(src_rel);
        let name = unique_name(&dest_dir_path, &original_name);
        copy_recursive(&src, &dest_dir_path.join(&name))?;
        Ok(join_rel(dest_dir, &name))
    }

    /// Перемежает файл или папку в `dest_dir`; возвращает новый путь.
    ///
    /// При конфликте имён — «копия»/«копия N» (как при копировании).
    pub fn move_entry(&self, src_rel: &str, dest_dir: &str) -> io::Result<String> {
        let src = self.root.join(src_rel);
        let dest_dir_path = self.root.join(dest_dir);
        std::fs::create_dir_all(&dest_dir_path)?;
        let original_name = file_name(src_rel);
        let name = unique_name(&dest_dir_path, &original_name);
        let dest = dest_dir_path.join(&name);
        if std::fs::rename(&src, &dest).is_err() {
            // другие ФС: копируем и удаляем источник
            copy_recursive(&src, &dest)?;
            if src.is_dir() {
                std::fs::remove_dir_all(&src)?;
            } else {
                std::fs::remove_file(&src)?;
            }
        }
        Ok(join_rel(dest_dir, &name))
    }

    /// Свободное имя файла в `dest_dir` (с «копия» при конфликте) —
    /// для планирования перемещений.
    pub fn free_name(&self, dest_dir: &str, name: &str) -> String {
        unique_name(&self.root.join(dest_dir), name)
    }

    /// Переименовывает файл или папку в пределах той же папки (точное имя).
    pub fn rename_entry(&self, rel_path: &str, new_name: &str) -> io::Result<String> {
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name.contains('/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "некорректное имя",
            ));
        }
        let parent = rel_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let dest = self.root.join(parent).join(new_name);
        if dest.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{new_name} уже существует"),
            ));
        }
        std::fs::rename(self.root.join(rel_path), &dest)?;
        Ok(join_rel(parent, new_name))
    }

    /// Заметки внутри `rel_prefix` (рекурсивно) — для переименования папок.
    pub fn notes_under(&self, rel_prefix: &str) -> Vec<NoteId> {
        fn walk<'a>(entries: &'a [VaultEntry], prefix: &str, out: &mut Vec<NoteId>) {
            for entry in entries {
                if entry.is_folder() {
                    walk(&entry.children, prefix, out);
                } else if entry.kind.is_note() && entry.rel_path.starts_with(prefix) {
                    out.push(entry.note_id());
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.tree, rel_prefix, &mut out);
        out
    }

    /// Все папки хранилища (включая корень `""`) — для выбора назначения.
    pub fn folders(&self) -> Vec<String> {
        fn walk<'a>(entries: &'a [VaultEntry], out: &mut Vec<String>) {
            for entry in entries {
                if entry.is_folder() {
                    out.push(entry.rel_path.clone());
                    walk(&entry.children, out);
                }
            }
        }
        let mut out = vec![String::new()];
        walk(&self.tree, &mut out);
        out
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

/// Склеивает относительный путь (`""` + `имя` = `имя`).
fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Последний компонент относительного пути.
fn file_name(rel_path: &str) -> String {
    rel_path.rsplit('/').next().unwrap_or(rel_path).to_string()
}

/// Добавляет расширение, если его нет.
fn ensure_extension(name: &str, ext: &str) -> String {
    if name.to_lowercase().ends_with(ext) {
        name.to_string()
    } else {
        format!("{name}{ext}")
    }
}

/// Свободное имя в `dir`: `имя` → `имя копия` → `имя копия 2` …
fn unique_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let mut counter = 1;
    loop {
        let candidate = if counter == 1 {
            format!("{stem} копия{ext}")
        } else {
            format!("{stem} копия {counter}{ext}")
        };
        if !dir.join(&candidate).exists() {
            return candidate;
        }
        counter += 1;
    }
}

/// Рекурсивное копирование файла или папки.
fn copy_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dest)?;
        Ok(())
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

    // ── файловые операции (SPEC §9.1) ──────────────────────────────────

    #[test]
    fn create_note_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();

        let folder = vault.create_folder("", "Проекты").unwrap();
        assert_eq!(folder, "Проекты");

        let id = vault.create_note("Проекты", "Идея").unwrap();
        assert_eq!(id.as_str(), "Проекты/Идея.md"); // расширение добавилось
        assert!(dir.path().join("Проекты/Идея.md").is_file());

        // дубликат не создаётся
        assert!(vault.create_note("Проекты", "Идея").is_err());
    }

    #[test]
    fn delete_file_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.create_folder("", "Ветка").unwrap();
        vault.create_note("Ветка", "Файл").unwrap();
        vault.create_note("", "Верхний").unwrap();

        vault.delete_entry("Верхний.md").unwrap();
        assert!(!dir.path().join("Верхний.md").exists());
        vault.delete_entry("Ветка").unwrap();
        assert!(!dir.path().join("Ветка").exists());
    }

    #[test]
    fn copy_entry_with_unique_names() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.create_folder("", "Архив").unwrap();
        vault.create_note("", "Дубль").unwrap();

        let first = vault.copy_entry("Дубль.md", "Архив").unwrap();
        assert_eq!(first, "Архив/Дубль.md");
        let second = vault.copy_entry("Дубль.md", "Архив").unwrap();
        assert_eq!(second, "Архив/Дубль копия.md");
        let third = vault.copy_entry("Дубль.md", "Архив").unwrap();
        assert_eq!(third, "Архив/Дубль копия 2.md");
    }

    #[test]
    fn copy_folder_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.create_folder("", "Исходник").unwrap();
        vault.create_folder("Исходник", "Подпапка").unwrap();
        vault.create_note("Исходник/Подпапка", "Глубокая").unwrap();

        let copy = vault.copy_entry("Исходник", "").unwrap();
        assert_eq!(copy, "Исходник копия");
        assert!(
            dir.path()
                .join("Исходник копия/Подпапка/Глубокая.md")
                .is_file()
        );
    }

    #[test]
    fn move_entry_between_folders() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.create_folder("", "Откуда").unwrap();
        vault.create_folder("", "Куда").unwrap();
        vault.create_note("Откуда", "Переезд").unwrap();

        let moved = vault.move_entry("Откуда/Переезд.md", "Куда").unwrap();
        assert_eq!(moved, "Куда/Переезд.md");
        assert!(dir.path().join("Куда/Переезд.md").is_file());
        assert!(!dir.path().join("Откуда/Переезд.md").exists());
    }

    #[test]
    fn notes_under_and_folders() {
        let (_dir, vault) = fixture_vault();
        let under = vault.notes_under("projects");
        assert_eq!(under.len(), 2); // Alpha.md и Бета.md (картинка — не заметка)

        let mut folders = vault.folders();
        folders.sort();
        assert_eq!(folders, vec!["", "projects", "projects/inner"]);
        let _ = _dir;
    }
}
