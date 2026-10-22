//! Слежение за изменениями файлов хранилища (SPEC §11).
//!
//! Обёртка над `notify` с дебаунсом: события собираются и отдаются пачками
//! после 300 мс тишины, чтобы не переиндексировать на каждое касание файла.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Внешнее изменение файла хранилища.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum VaultEvent {
    /// Файл создан или изменён (путь — относительный, с `/`).
    Upsert(String),
    /// Файл удалён или переименован (старый путь).
    Remove(String),
}

/// Наблюдатель за хранилищем.
pub struct VaultWatcher {
    // наблюдатель живёт столько же, сколько обёртка
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<notify::Event>>,
    root: PathBuf,
    debounce: Duration,
    pending: Vec<VaultEvent>,
    last_event: Option<Instant>,
}

impl VaultWatcher {
    /// Начинает наблюдение за `root` (рекурсивно).
    pub fn new(root: &Path) -> notify::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            rx,
            root: root.to_path_buf(),
            debounce: Duration::from_millis(300),
            pending: Vec::new(),
            last_event: None,
        })
    }

    /// Неблокирующее чтение: забирает события из канала и отдаёт пачкой,
    /// когда после последнего события прошло не меньше дебаунса.
    ///
    /// Контракт: события, поступившие при вызове, возвращаются одним из
    /// СЛЕДУЮЩИХ вызовов (после тишины). Периодический опрос раз в ~200 мс
    /// даёт задержку 200–400 мс — укладывается в бюджет SPEC §11.
    pub fn drain(&mut self) -> Vec<VaultEvent> {
        loop {
            match self.rx.try_recv() {
                Ok(Ok(event)) => {
                    self.last_event = Some(Instant::now());
                    self.collect(&event);
                }
                Ok(Err(_)) | Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        let quiet = self
            .last_event
            .is_some_and(|last| last.elapsed() >= self.debounce);
        if quiet {
            self.last_event = None;
            let mut batch = std::mem::take(&mut self.pending);
            batch.sort();
            batch.dedup();
            batch
        } else {
            Vec::new()
        }
    }

    /// Разбирает событие notify в события хранилища.
    fn collect(&mut self, event: &notify::Event) {
        // Access/открытие файла игнорируем: наши же чтения (разбор,
        // автосейв) иначе порождали бы события по кругу
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        for path in &event.paths {
            let Some(rel) = self.relative_path(path) else {
                continue;
            };
            // существование пути надёжнее класса события: rename приходит
            // парой From/To, а Create/Modify различаются у платформ по-разному
            let vault_event = if path.is_file() {
                VaultEvent::Upsert(rel)
            } else {
                VaultEvent::Remove(rel)
            };
            self.pending.push(vault_event);
        }
    }

    /// Относительный путь с `/`-разделителями; None — вне хранилища или
    /// служебный/скрытый файл.
    fn relative_path(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        if parts.is_empty() || parts.iter().any(|part| part.starts_with('.')) {
            return None;
        }
        let joined = parts.join("/");
        // индексируем markdown; вложения интересуют только для кэша картинок
        joined.to_lowercase().ends_with(".md").then_some(joined)
    }
}

/// Уникальность событий внутри пачки — для sort/dedup.
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn watches_create_modify_remove() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = VaultWatcher::new(dir.path()).unwrap();

        // контракт polling'а: забрать события, дождаться тишины, получить пачку
        let mut poll = |watcher: &mut VaultWatcher| {
            watcher.drain();
            std::thread::sleep(Duration::from_millis(400));
            watcher.drain()
        };

        fs::write(dir.path().join("Заметка.md"), "привет\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let events = poll(&mut watcher);
        assert!(
            events.contains(&VaultEvent::Upsert("Заметка.md".to_string())),
            "{events:?}"
        );

        fs::write(dir.path().join("Заметка.md"), "изменено\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let events = poll(&mut watcher);
        assert!(
            events.contains(&VaultEvent::Upsert("Заметка.md".to_string())),
            "{events:?}"
        );

        fs::remove_file(dir.path().join("Заметка.md")).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let events = poll(&mut watcher);
        assert!(
            events.contains(&VaultEvent::Remove("Заметка.md".to_string())),
            "{events:?}"
        );
    }

    #[test]
    fn ignores_hidden_and_non_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = VaultWatcher::new(dir.path()).unwrap();

        fs::create_dir(dir.path().join(".dunite")).unwrap();
        fs::write(dir.path().join(".dunite/index.bin"), "x").unwrap();
        fs::write(dir.path().join("картинка.png"), "x").unwrap();
        fs::write(dir.path().join(".скрытая.md"), "x").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(watcher.drain().is_empty());
    }

    #[test]
    fn reads_do_not_retrigger_events() {
        // регрессия: чтение файла порождало событие Access, apply читал файл
        // снова — события повторялись бесконечно
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Заметка.md"), "текст\n").unwrap();
        let mut watcher = VaultWatcher::new(dir.path()).unwrap();

        fs::write(dir.path().join("Заметка.md"), "новый текст\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = watcher.drain();
        std::thread::sleep(Duration::from_millis(400));
        let first = watcher.drain();
        assert!(!first.is_empty(), "{first:?}");

        // читаем файл, как это делает граф при переиндексации
        for _ in 0..5 {
            let _ = fs::read_to_string(dir.path().join("Заметка.md")).unwrap();
        }
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            watcher.drain().is_empty(),
            "чтения не должны порождать события"
        );
    }

    #[test]
    fn debounce_batches_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = VaultWatcher::new(dir.path()).unwrap();

        // серия событий без паузы — до дебаунса пусто
        for ix in 0..5 {
            fs::write(dir.path().join(format!("note{ix}.md")), "x").unwrap();
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(watcher.drain().is_empty());

        std::thread::sleep(Duration::from_millis(400));
        let events = watcher.drain();
        assert_eq!(events.len(), 5, "{events:?}");
        assert!(watcher.drain().is_empty());
    }
}
