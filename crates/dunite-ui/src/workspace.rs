//! Корневое представление рабочей области (SPEC §9).
//!
//! Состояние M1: дерево, вкладки с редакторами, автосейв, статус-бар.
//! Дальше — сплиты, сайдбары (outline/backlinks/теги), палитры команд.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use dunite_editor::Editor;
use dunite_git::{CommitInfo, GitRepo};
use dunite_index::LinkGraph;
use dunite_markdown::ParsedNote;
use dunite_settings::Settings;
use dunite_vault::watch::{VaultEvent, VaultWatcher};
use dunite_vault::{NoteId, Vault};
use gpui::{
    Context, FocusHandle, FontWeight, MouseDownEvent, MouseMoveEvent, Render, RenderImage, Window,
    div, prelude::*, px,
};

use crate::actions::{
    CloseTab, InitGit, NavBack, NavForward, NextTab, OpenSearch, OpenSwitcher, PrevTab, Quit,
    ReloadNote, RenameNote, SaveNote,
};
use crate::decor;
use crate::editor_element::PaintState;
use crate::editor_pane::ClickTracker;
use crate::images;
use crate::palette::{PaletteMode, PaletteResults, PaletteState};
use crate::theme::Palette;
use crate::tree_menu::{TreeClip, TreeMenuState};

/// Состояние загрузки изображения (SPEC §7.5).
pub enum ImageState {
    Loading,
    Loaded {
        render: std::sync::Arc<RenderImage>,
        width: u32,
        height: u32,
    },
    Failed,
}

/// Кэш раскладки редактора (SPEC §8.2): парсинг markdown и позиции строк.
///
/// Пересчитывается при смене версии буфера. Инкрементальный репарсинг
/// отдельных блоков — будущая оптимизация; полный парсинг заметки быстр.
pub struct EditorLayout {
    /// Ключ кэша: уникальный идентификатор экземпляра буфера.
    /// Кэш не переживает ни правки (version), ни подмену редактора (epoch) —
    /// раньше протухание ломало рендер после внешних изменений.
    pub epoch: u64,
    pub version: u64,
    /// Ширина панели, для которой посчитаны размеры картинок.
    pub width: f32,
    pub parsed: ParsedNote,
    /// Верх каждой строки (пиксели контента) + итоговая высота.
    pub tops: Rc<Vec<f32>>,
    /// Высоты строк.
    pub heights: Rc<Vec<f32>>,
    /// Строки со standalone-картинками: (номер строки, цель embed).
    pub images: Vec<(usize, String)>,
}

/// Открытая вкладка.
pub struct Tab {
    pub note_id: NoteId,
    pub editor: Editor,
    /// Файл изменён снаружи, а в буфере есть несохранённые правки:
    /// автосейв приостановлен до явного выбора (SPEC §14).
    pub conflict: bool,
    /// Файл удалён снаружи: буфер сохранён, вкладка помечена (SPEC §14).
    pub deleted: bool,
}

/// Рабочая область dunite.
pub struct Workspace {
    pub(crate) vault: Vault,
    /// Граф ссылок/тегов (SPEC §7.2): резолвинг, backlinks, теги.
    pub(crate) graph: LinkGraph,
    pub(crate) settings: Settings,
    pub(crate) palette: Palette,
    /// Развернутые папки дерева (`rel_path`).
    pub(crate) expanded: HashSet<String>,
    /// Развернутые теги в панели тегов.
    pub(crate) expanded_tags: HashSet<String>,
    /// Открытая палитра (quick switcher / переименование).
    pub(crate) palette_state: Option<PaletteState>,
    /// Фокус палитры — отдельный, чтобы её контекст клавиш не спорил
    /// с контекстом редактора.
    pub(crate) palette_focus: FocusHandle,
    /// История навигации назад/вперёд (SPEC §8.3): (заметка, смещение).
    pub(crate) nav_back: Vec<(NoteId, usize)>,
    pub(crate) nav_forward: Vec<(NoteId, usize)>,
    /// Наблюдатель за внешними изменениями файлов (SPEC §11).
    pub(crate) watcher: Option<VaultWatcher>,
    /// Контекстное меню дерева файлов (SPEC §9.1).
    pub(crate) tree_menu: Option<TreeMenuState>,
    /// Буфер обмена дерева (копировать/вырезать).
    pub(crate) tree_clipboard: Option<TreeClip>,
    /// Git-репозиторий хранилища (SPEC §10).
    pub(crate) git: Option<GitRepo>,
    /// Последнее изменение — для автоснапшота.
    pub(crate) last_edit: Option<std::time::Instant>,
    /// Время последнего снапшота — для статус-бара.
    pub(crate) last_snapshot: Option<String>,
    /// Кэши git-состояния: пересчёт слишком дорог на больших хранилищах
    /// (status сканирует все файлы) — тяжёлые обновляются раз в 30 с
    /// планировщиком и по событиям.
    pub(crate) git_status_cache: String,
    pub(crate) git_log_cache: Vec<CommitInfo>,
    /// Кэш тегов: `graph.tags()` строит полный BTreeMap по метаданным —
    /// на каждый кадр это фризило рендер больших хранилищ.
    pub(crate) tags_cache: std::collections::BTreeMap<String, usize>,
    /// Область timeline: заметка или всё хранилище.
    pub(crate) git_scope_note: bool,
    /// Раскрытый коммит в timeline (diff-превью).
    pub(crate) git_expanded: Option<String>,
    /// Diff-строки раскрытого коммита.
    pub(crate) git_diff: Vec<dunite_git::DiffLine>,
    /// Файлы раскрытого коммита и выбранный для превью/восстановления.
    pub(crate) git_diff_files: Vec<String>,
    pub(crate) git_diff_file: Option<String>,
    /// Ширина правой панели (перетаскиваемая граница, SPEC §9).
    pub(crate) right_panel_width: f32,
    pub(crate) panel_dragging: bool,
    pub(crate) panel_drag_last_x: f32,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active_tab: usize,
    pub(crate) status_message: String,
    /// Скролл панели редактора (пиксели контента).
    pub(crate) scroll_px: f32,
    /// Геометрия последнего рендера (hit-testing мыши).
    pub(crate) paint_state: Option<PaintState>,
    /// Кэш раскладки (парсинг + вертикальные позиции строк).
    pub(crate) layout: Option<EditorLayout>,
    /// Кэш изображений по цели embed.
    pub(crate) images: HashMap<String, ImageState>,
    /// Идёт выделение мышью (drag).
    pub(crate) selecting: bool,
    /// Распознавание двойного/тройного клика.
    pub(crate) click: ClickTracker,
    pub(crate) focus_handle: FocusHandle,
    /// Поколение правок: автосейв пишет только последнее.
    pub(crate) save_generation: u64,
}

impl Workspace {
    pub fn new(
        vault: Vault,
        settings: Settings,
        focus_handle: FocusHandle,
        palette_focus: FocusHandle,
    ) -> Self {
        let palette = Palette::for_theme(settings.theme);
        // полный разбор хранилища при открытии; для больших vault'ов
        // позже уедет в фон (сейчас это миллисекунды)
        let graph = LinkGraph::build(&vault);
        tracing::info!("граф и индекс построены: {} заметок", vault.note_count());
        let git = GitRepo::open(vault.root()).ok().flatten();
        Self {
            vault,
            graph,
            settings,
            palette,
            expanded: HashSet::new(),
            expanded_tags: HashSet::new(),
            tabs: Vec::new(),
            active_tab: 0,
            status_message: String::new(),
            scroll_px: 0.,
            paint_state: None,
            layout: None,
            images: HashMap::new(),
            selecting: false,
            click: ClickTracker::default(),
            focus_handle,
            palette_state: None,
            palette_focus,
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            watcher: None,
            tree_menu: None,
            tree_clipboard: None,
            git,
            last_edit: None,
            last_snapshot: None,
            git_status_cache: String::new(),
            git_log_cache: Vec::new(),
            tags_cache: Default::default(),
            git_scope_note: true,
            git_expanded: None,
            git_diff: Vec::new(),
            git_diff_files: Vec::new(),
            git_diff_file: None,
            right_panel_width: 280.,
            panel_dragging: false,
            panel_drag_last_x: 0.,
            save_generation: 0,
        }
    }

    /// Запускает фоновые автоснапшоты git (SPEC §10.1) и fetch счётчиков.
    pub fn spawn_git_scheduler(&mut self, cx: &mut Context<Self>) {
        self.refresh_git_caches();
        self.refresh_tags_cache();
        cx.spawn(async move |this, cx| {
            let mut last_fetch = std::time::Instant::now();
            let mut tick = 0u32;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                tick += 1;
                this.update(cx, |workspace, cx| {
                    workspace.maybe_snapshot(cx);
                    // теги дёшевы; тяжёлые git-кэши — раз в 30 с (6 тиков):
                    // git status сканирует все файлы хранилища и фризил
                    // главный поток на больших хранилищах
                    workspace.refresh_tags_cache();
                    if tick % 6 == 0 {
                        workspace.refresh_git_caches();
                    }
                    cx.notify();
                })
                .ok();
                // fetch раз в минуту — свежие ↑↓ для статус-бара (сетевой,
                // поэтому редко и в фоне)
                if last_fetch.elapsed().as_secs() >= 60 {
                    last_fetch = std::time::Instant::now();
                    this.update(cx, |workspace, cx| workspace.git_fetch_in_background(cx))
                        .ok();
                }
            }
        })
        .detach();
    }

    /// Автоснапшот: когда после последнего изменения прошла задержка
    /// и есть что коммитить.
    pub(crate) fn maybe_snapshot(&mut self, cx: &mut Context<Self>) {
        let debounce = self.settings.snapshot_debounce_secs;
        if debounce == 0 || self.git.is_none() {
            return;
        }
        let quiet = self
            .last_edit
            .is_some_and(|last| last.elapsed().as_secs() >= debounce);
        if !quiet {
            return;
        }
        self.snapshot_now("автоснапшот", cx);
    }

    /// Снапшот сейчас; `reason` — для статус-сообщения.
    pub(crate) fn snapshot_now(&mut self, reason: &str, cx: &mut Context<Self>) {
        let Some(git) = self.git.as_ref() else {
            self.status_message = "git-репозиторий не найден (Ctrl+G — создать)".to_string();
            cx.notify();
            return;
        };
        match git.snapshot() {
            Ok(Some(_)) => {
                self.last_snapshot = Some(dunite_git::now_local());
                self.status_message = format!("git: снапшот создан ({reason})");
                self.git_expanded = None;
                self.git_diff.clear();
            }
            Ok(None) => {
                self.status_message = "git: нет изменений для снапшота".to_string();
            }
            Err(err) => {
                self.status_message = format!("git: ошибка снапшота: {err}");
            }
        }
        self.refresh_git_caches();
        cx.notify();
    }

    /// Ctrl+G: git init для хранилища без репозитория (SPEC §10.1).
    pub(crate) fn git_init(&mut self, cx: &mut Context<Self>) {
        if self.git.is_some() {
            self.status_message = "git: репозиторий уже есть".to_string();
            cx.notify();
            return;
        }
        match GitRepo::init(self.vault.root()) {
            Ok(repo) => {
                self.git = Some(repo);
                self.last_snapshot = Some(dunite_git::now_local());
                self.status_message = "git: хранилище под контролем версий".to_string();
            }
            Err(err) => {
                self.status_message = format!("git init не удался: {err}");
            }
        }
        self.refresh_git_caches();
        cx.notify();
    }

    /// Push в remote (сетевая операция — в фоне, SPEC §10.1).
    pub(crate) fn git_push(&mut self, cx: &mut Context<Self>) {
        let Some(git) = self.git.as_ref() else {
            self.status_message = "git: репозиторий не найден (Ctrl+G)".to_string();
            cx.notify();
            return;
        };
        let args = git.push_args();
        let root = git.root().to_path_buf();
        self.status_message = "git: push…".to_string();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { dunite_git::run_cli(&root, &args) })
                .await;
            this.update(cx, |workspace, cx| {
                workspace.status_message = match result {
                    Ok(output) if output.is_empty() => "git: push завершён".to_string(),
                    Ok(output) => format!("git: {output}"),
                    Err(err) => format!("git: push: {err}"),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Pull из remote: только fast-forward, без мержей (SPEC §14).
    pub(crate) fn git_pull(&mut self, cx: &mut Context<Self>) {
        let Some(git) = self.git.as_ref() else {
            self.status_message = "git: репозиторий не найден (Ctrl+G)".to_string();
            cx.notify();
            return;
        };
        let args = git.pull_args();
        let root = git.root().to_path_buf();
        self.status_message = "git: pull…".to_string();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { dunite_git::run_cli(&root, &args) })
                .await;
            this.update(cx, |workspace, cx| {
                match result {
                    Ok(_output) => {
                        workspace.reload_all_tabs();
                        workspace.status_message = "git: pull завершён (fast-forward)".to_string();
                    }
                    Err(err) => {
                        workspace.status_message =
                            format!("git: pull: {err} — истории разошлись? разрешите вручную");
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Фоновый fetch: обновляет счётчики ↑↓ (раз в минуту из планировщика).
    pub(crate) fn git_fetch_in_background(&mut self, cx: &mut Context<Self>) {
        let Some(git) = self.git.as_ref() else {
            return;
        };
        let args = git.fetch_args();
        let root = git.root().to_path_buf();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let _ = dunite_git::run_cli(&root, &args);
                })
                .await;
            this.update(cx, |_workspace, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// Палитра добавления remote (кнопка «+ remote»).
    pub(crate) fn open_remote_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_state = Some(PaletteState::new(
            PaletteMode::AddRemote,
            String::new(),
            PaletteResults::Notes(Vec::new()),
        ));
        self.palette_focus.focus(window);
        cx.notify();
    }

    /// Добавляет remote (имя — `origin`, первый по списку).
    pub(crate) fn add_remote(&mut self, url: &str, cx: &mut Context<Self>) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.status_message = "git: пустой URL remote".to_string();
            cx.notify();
            return;
        }
        match self.git.as_ref() {
            Some(git) => match git.add_remote("origin", &url) {
                Ok(()) => {
                    self.status_message =
                        "git: remote origin добавлен — push запомнит upstream".to_string();
                    self.git_fetch_in_background(cx);
                }
                Err(err) => {
                    self.status_message = format!("git: {err}");
                }
            },
            None => {
                self.status_message = "git: репозиторий не найден (Ctrl+G)".to_string();
            }
        }
        cx.notify();
    }

    // ── операции дерева файлов (SPEC §9.1) ──────────────────────────────

    /// Обновление после файловых операций: дерево, вкладки, рендер.
    fn tree_refresh(&mut self, cx: &mut Context<Self>) {
        let _ = self.vault.rescan();
        self.reload_all_tabs();
        self.layout = None;
        cx.notify();
    }

    /// Создаёт заметку и открывает её.
    pub(crate) fn tree_create_note(&mut self, dir: &str, name: &str, cx: &mut Context<Self>) {
        match self.vault.create_note(dir, name) {
            Ok(id) => {
                self.graph.update_note(&self.vault, &id);
                self.tree_refresh(cx);
                self.open_note(id.as_str(), cx);
                self.status_message = format!("создана заметка {}", id.as_str());
            }
            Err(err) => {
                self.status_message = format!("не удалось создать заметку: {err}");
            }
        }
        cx.notify();
    }

    pub(crate) fn tree_create_folder(&mut self, dir: &str, name: &str, cx: &mut Context<Self>) {
        match self.vault.create_folder(dir, name) {
            Ok(path) => {
                self.tree_refresh(cx);
                self.status_message = format!("создана папка {path}");
            }
            Err(err) => {
                self.status_message = format!("не удалось создать папку: {err}");
            }
        }
        cx.notify();
    }

    /// Переименование файла/папки/вложения (SPEC §7.2: ссылки обновляются).
    pub(crate) fn tree_rename_entry(&mut self, path: &str, new_name: &str, cx: &mut Context<Self>) {
        let parent = crate::tree_menu::parent_dir(path);
        let to = if parent.is_empty() {
            new_name.trim().to_string()
        } else {
            format!("{}/{}", parent, new_name.trim())
        };
        let is_folder = self.vault.absolute(path).is_dir();
        let is_note = path.to_lowercase().ends_with(".md");

        let result: std::io::Result<usize> = if is_folder {
            self.graph.rename_folder(&self.vault, path, &to)
        } else if is_note {
            self.graph
                .rename_note(&self.vault, &NoteId::new(path), &NoteId::new(&to))
        } else {
            self.vault.rename_entry(path, new_name).map(|_| 0usize)
        };

        match result {
            Ok(updated) => {
                self.retag_tabs_after_move(path, &to, is_folder);
                self.tree_refresh(cx);
                self.status_message = if updated > 0 {
                    format!("переименовано в {to}: обновлено ссылок: {updated}")
                } else {
                    format!("переименовано в {to}")
                };
            }
            Err(err) => {
                self.status_message = format!("переименование не удалось: {err}");
            }
        }
        cx.notify();
    }

    /// Перемещение файла/папки в `dest_dir` (ссылки обновляются).
    pub(crate) fn tree_move_entry(&mut self, path: &str, dest_dir: &str, cx: &mut Context<Self>) {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let to_name = self.vault.free_name(dest_dir, &name);
        let to = if dest_dir.is_empty() {
            to_name
        } else {
            format!("{dest_dir}/{to_name}")
        };
        let is_folder = self.vault.absolute(path).is_dir();
        let is_note = path.to_lowercase().ends_with(".md");

        let moved: std::io::Result<usize> = if is_folder {
            self.graph.rename_folder(&self.vault, path, &to)
        } else if is_note {
            self.graph
                .rename_note(&self.vault, &NoteId::new(path), &NoteId::new(&to))
        } else {
            self.vault.move_entry(path, dest_dir).map(|_| 0usize)
        };

        match moved {
            Ok(updated) => {
                self.retag_tabs_after_move(path, &to, is_folder);
                self.tree_refresh(cx);
                self.status_message = if updated > 0 {
                    format!("перемещено в {to}: обновлено ссылок: {updated}")
                } else {
                    format!("перемещено в {to}")
                };
            }
            Err(err) => {
                self.status_message = format!("перемещение не удалось: {err}");
            }
        }
        cx.notify();
    }

    /// Копирование/перемещение через буфер обмена дерева.
    pub(crate) fn tree_paste(
        &mut self,
        clip: &crate::tree_menu::TreeClip,
        dest_dir: &str,
        cx: &mut Context<Self>,
    ) {
        if clip.cut {
            self.tree_move_entry(&clip.path, dest_dir, cx);
            self.tree_clipboard = None;
            return;
        }
        match self.vault.copy_entry(&clip.path, dest_dir) {
            Ok(new_path) => {
                self.tree_refresh(cx);
                for id in self.vault.notes_under(&new_path) {
                    self.graph.update_note(&self.vault, &id);
                }
                self.status_message = format!("вставлено: {new_path}");
            }
            Err(err) => {
                self.status_message = format!("вставка не удалась: {err}");
            }
        }
        cx.notify();
    }

    /// Удаление файла/папки. Безопасно: сначала git-снапшот (SPEC §10).
    pub(crate) fn tree_delete(&mut self, path: &str, cx: &mut Context<Self>) {
        let is_folder = self.vault.absolute(path).is_dir();
        let is_note = path.to_lowercase().ends_with(".md");
        let removed: Vec<NoteId> = if is_folder {
            self.vault.notes_under(path)
        } else if is_note {
            vec![NoteId::new(path)]
        } else {
            Vec::new()
        };

        // восстановимо из истории: фиксируем несохранённые правки до
        // удаления (сам коммит удаления создаётся ниже)
        if self.git.is_some() {
            self.save_all();
            if let Some(git) = self.git.as_ref() {
                if git.has_changes() {
                    let _ = git.snapshot_with_message(&format!("перед удалением {path}"));
                }
            }
        }

        match self.vault.delete_entry(path) {
            Ok(()) => {
                self.close_tabs_under(path);
                for id in removed {
                    self.graph.remove_note(&id);
                }
                self.tree_refresh(cx);
                // коммит удаления: его diff содержит файл, поэтому
                // «восстановить» работает прямо с него (SPEC §10.2)
                if let Some(git) = self.git.as_ref() {
                    if let Ok(Some(_)) = git.snapshot_with_message(&format!("удаление: {path}"))
                    {
                        self.last_snapshot = Some(dunite_git::now_local());
                    }
                }
                self.status_message =
                    format!("удалено: {path} — восстановить можно из истории (git)");
            }
            Err(err) => {
                self.status_message = format!("удаление не удалось: {err}");
            }
        }
        cx.notify();
    }

    /// Закрывает вкладки удалённого файла/папки.
    fn close_tabs_under(&mut self, path: &str) {
        let prefix = format!("{path}/");
        self.tabs.retain(|tab| {
            let id = tab.note_id.as_str();
            id != path && !id.starts_with(&prefix)
        });
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        }
    }

    /// Обновляет note_id вкладок после переименования/перемещения.
    fn retag_tabs_after_move(&mut self, from: &str, to: &str, is_folder: bool) {
        let from_prefix = format!("{from}/");
        for tab in &mut self.tabs {
            let id = tab.note_id.as_str();
            if is_folder && id.starts_with(&from_prefix) {
                let suffix = &id[from.len()..];
                tab.note_id = NoteId::new(format!("{to}{suffix}"));
            } else if !is_folder && id == from {
                tab.note_id = NoteId::new(to);
            }
        }
    }

    /// Строка git-состояния для статус-бара (из кэша, SPEC §9.5).
    pub(crate) fn git_status_text(&self) -> String {
        self.git_status_cache.clone()
    }

    /// Timeline коммитов (из кэша, SPEC §10.2).
    pub(crate) fn git_log(&self) -> &[CommitInfo] {
        &self.git_log_cache
    }

    /// Пересчёт git-кэшей: тяжело на больших хранилищах (status сканирует
    /// все файлы) — планировщик раз в 30 с и события (снапшот, init).
    pub(crate) fn refresh_git_caches(&mut self) {
        self.git_status_cache = self.compute_git_status_text();
        self.git_log_cache = self.compute_git_log();
    }

    /// Лёгкий кэш тегов — после правок и раз в тик планировщика.
    pub(crate) fn refresh_tags_cache(&mut self) {
        self.tags_cache = self.graph.tags();
    }

    fn compute_git_status_text(&self) -> String {
        let Some(git) = self.git.as_ref() else {
            return "git: нет репозитория (Ctrl+G)".to_string();
        };
        let changes = git.changed_files().map(|files| files.len()).unwrap_or(0);
        let snapshot = self
            .last_snapshot
            .as_deref()
            .map(|time| format!(" · снапшот {time}"))
            .unwrap_or_default();
        let remote = git
            .remote_status()
            .map(|status| format!(" · {status}"))
            .unwrap_or_default();
        if changes == 0 {
            format!("git: чисто{snapshot}{remote}")
        } else {
            format!("git: изменённых файлов: {changes}{snapshot}{remote}")
        }
    }

    fn compute_git_log(&self) -> Vec<CommitInfo> {
        let Some(git) = self.git.as_ref() else {
            return Vec::new();
        };
        if self.git_scope_note {
            match self.tabs.get(self.active_tab) {
                Some(tab) => git
                    .log_for_path(tab.note_id.as_str(), 50)
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        } else {
            git.log(50).unwrap_or_default()
        }
    }

    /// Раскрыть/свернуть diff-превью коммита.
    ///
    /// Файлы берутся из САМОГО коммита: удалённая заметка не имеет
    /// вкладки, восстановление работает по файлу коммита (SPEC §10.2).
    pub(crate) fn toggle_git_diff(&mut self, commit_id: &str, cx: &mut Context<Self>) {
        if self.git_expanded.as_deref() == Some(commit_id) {
            self.git_expanded = None;
            self.git_diff.clear();
            self.git_diff_files.clear();
            self.git_diff_file = None;
            cx.notify();
            return;
        }
        let Some(git) = self.git.as_ref() else {
            return;
        };
        let mut files = git.commit_files(commit_id).unwrap_or_default();
        files.sort();
        // выбранный файл: активная заметка, если она в коммите; иначе первая
        // заметка коммита; иначе первый файл
        let preferred = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.note_id.as_str().to_string())
            .filter(|id| files.iter().any(|file| file == id))
            .or_else(|| files.iter().find(|file| file.ends_with(".md")).cloned())
            .or_else(|| files.first().cloned());
        self.git_diff = preferred
            .as_ref()
            .and_then(|path| git.diff_note(commit_id, path).ok())
            .unwrap_or_default();
        self.git_diff_file = preferred;
        self.git_diff_files = files;
        self.git_expanded = Some(commit_id.to_string());
        cx.notify();
    }

    /// Выбор файла коммита для diff-превью (клик по чипу файла).
    pub(crate) fn select_git_diff_file(
        &mut self,
        commit_id: &str,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git.as_ref() else {
            return;
        };
        self.git_diff_file = Some(path.to_string());
        self.git_diff = git.diff_note(commit_id, path).ok().unwrap_or_default();
        cx.notify();
    }

    /// Восстановить файл из коммита (SPEC §10.2: всегда новый коммит).
    pub(crate) fn restore_from_commit(
        &mut self,
        commit_id: &str,
        rel_path: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git.as_ref() else {
            return;
        };
        match git.restore_note(commit_id, rel_path) {
            Ok(_) => {
                // файл мог быть удалён — вернуть его в дерево и граф
                let _ = self.vault.rescan();
                if rel_path.to_lowercase().ends_with(".md") {
                    self.graph.update_note(&self.vault, &NoteId::new(rel_path));
                }
                self.save_generation += 1;
                self.reload_all_tabs();
                self.snapshot_now("восстановление из истории", cx);
                self.status_message = format!("git: {rel_path} восстановлена из истории");
            }
            Err(err) => {
                self.status_message = format!("git: восстановление не удалось: {err}");
            }
        }
        cx.notify();
    }

    // ── граница правой панели (ресайз) ──────────────────────────────────

    /// Начало перетаскивания границы.
    pub(crate) fn panel_drag_start(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.panel_dragging = true;
        self.panel_drag_last_x = event.position.x.to_f64() as f32;
        cx.notify();
    }

    /// Перетаскивание: тянем влево — панель шире.
    pub(crate) fn panel_drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.panel_dragging {
            return;
        }
        let x = event.position.x.to_f64() as f32;
        let delta = self.panel_drag_last_x - x;
        self.panel_drag_last_x = x;
        self.right_panel_width = (self.right_panel_width + delta).clamp(220., 720.);
        cx.notify();
    }

    pub(crate) fn panel_drag_end(&mut self, cx: &mut Context<Self>) {
        self.panel_dragging = false;
        cx.notify();
    }

    /// Запускает фоновое наблюдение за внешними изменениями файлов.
    pub fn spawn_watcher(&mut self, cx: &mut Context<Self>) {
        match VaultWatcher::new(self.vault.root()) {
            Ok(watcher) => {
                self.watcher = Some(watcher);
                tracing::info!("watcher запущен: {}", self.vault.root().display());
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(200))
                            .await;
                        let batch = this
                            .update(cx, |workspace, _| {
                                workspace
                                    .watcher
                                    .as_mut()
                                    .map(|watcher| watcher.drain())
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();
                        if !batch.is_empty() {
                            tracing::info!("watcher: пачка из {} событий", batch.len());
                            this.update(cx, |workspace, cx| {
                                workspace.apply_vault_events(batch, cx)
                            })
                            .ok();
                        }
                    }
                })
                .detach();
            }
            Err(err) => {
                self.status_message = format!("watcher недоступен: {err}");
                tracing::error!("watcher не запустился: {err}");
            }
        }
    }

    /// Применяет пачку внешних изменений: граф, дерево, открытые вкладки.
    fn apply_vault_events(&mut self, events: Vec<VaultEvent>, cx: &mut Context<Self>) {
        let mut changed = false;
        for event in events {
            tracing::info!("watcher: событие {event:?}");
            match event {
                VaultEvent::Upsert(rel) => {
                    if self.vault.absolute(&rel).is_file() {
                        self.graph.update_note(&self.vault, &NoteId::new(&rel));
                        self.status_message = format!("внешнее изменение: {rel}");
                        changed = true;
                    }
                }
                VaultEvent::Remove(rel) => {
                    self.graph.remove_note(&NoteId::new(&rel));
                    self.status_message = format!("файл удалён снаружи: {rel}");
                    changed = true;
                }
            }
        }
        if changed {
            let _ = self.vault.rescan();
            self.reload_all_tabs();
            cx.notify();
        }
    }

    /// Открывает заметку во вкладке (или фокусирует уже открытую).
    pub fn open_note(&mut self, rel_path: &str, cx: &mut Context<Self>) {
        self.open_note_impl(rel_path, cx, true);
    }

    fn open_note_impl(&mut self, rel_path: &str, cx: &mut Context<Self>, record_history: bool) {
        if record_history {
            let current = self
                .tabs
                .get(self.active_tab)
                .map(|tab| (tab.note_id.clone(), tab.editor.selection().head));
            if let Some((current_id, offset)) = current {
                if current_id.as_str() != rel_path {
                    self.nav_back.push((current_id, offset));
                    self.nav_forward.clear();
                }
            }
        }
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|tab| tab.note_id.as_str() == rel_path)
        {
            self.active_tab = ix;
            self.status_message.clear();
            cx.notify();
            return;
        }
        match self.vault.read_note(rel_path) {
            Ok(text) => {
                self.tabs.push(Tab {
                    note_id: NoteId::new(rel_path),
                    editor: Editor::from_text(text),
                    conflict: false,
                    deleted: false,
                });
                self.active_tab = self.tabs.len() - 1;
                self.scroll_px = 0.;
                self.status_message.clear();
            }
            Err(err) => {
                self.status_message = format!("не удалось открыть {rel_path}: {err}");
            }
        }
        cx.notify();
    }

    /// Назад по истории переходов (Alt+Left).
    pub(crate) fn navigate_back(&mut self, cx: &mut Context<Self>) {
        let Some((id, offset)) = self.nav_back.pop() else {
            return;
        };
        if let Some(tab) = self.tabs.get(self.active_tab) {
            self.nav_forward
                .push((tab.note_id.clone(), tab.editor.selection().head));
        }
        self.open_note_impl(id.as_str(), cx, false);
        self.goto_offset(offset, cx);
    }

    /// Вперёд по истории переходов (Alt+Right).
    pub(crate) fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        let Some((id, offset)) = self.nav_forward.pop() else {
            return;
        };
        if let Some(tab) = self.tabs.get(self.active_tab) {
            self.nav_back
                .push((tab.note_id.clone(), tab.editor.selection().head));
        }
        self.open_note_impl(id.as_str(), cx, false);
        self.goto_offset(offset, cx);
    }

    pub(crate) fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active_tab = ix;
            self.scroll_px = 0.;
            cx.notify();
        }
    }

    pub(crate) fn close_tab_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.save_tab_now(ix);
        self.tabs.remove(ix);
        self.active_tab = next_active_tab(self.active_tab, ix, self.tabs.len());
        cx.notify();
    }

    pub(crate) fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.close_tab_at(self.active_tab, cx);
        }
    }

    pub(crate) fn activate_next_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
            self.scroll_px = 0.;
            cx.notify();
        }
    }

    pub(crate) fn activate_prev_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
            self.scroll_px = 0.;
            cx.notify();
        }
    }

    /// Разворачивает/сворачивает папку дерева.
    pub(crate) fn toggle_folder(&mut self, rel_path: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(rel_path) {
            self.expanded.insert(rel_path.to_string());
        }
        cx.notify();
    }

    /// Пересчитывает кэш раскладки, если буфер или ширина изменились.
    pub(crate) fn ensure_layout(&mut self, width: f32) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            self.layout = None;
            return;
        };
        let version = tab.editor.version();
        let epoch = tab.editor.buffer().epoch();
        if self.layout.as_ref().is_some_and(|layout| {
            layout.epoch == epoch && layout.version == version && (layout.width - width).abs() < 2.
        }) {
            return;
        }
        let text = tab.editor.text();
        // огромная заметка — plain-режим без декораций (SPEC §14)
        let parsed = if text.len() > dunite_index::LARGE_NOTE_BYTES {
            dunite_markdown::ParsedNote::default()
        } else {
            dunite_markdown::parse(&text)
        };
        let buffer = tab.editor.buffer();
        let total_lines = buffer.len_lines();

        let mut heights = Vec::with_capacity(total_lines);
        let mut images = Vec::new();
        for line_ix in 0..total_lines {
            let line_start = buffer.line_start(line_ix);
            let kind = parsed
                .blocks
                .iter()
                .find(|block| block.range.start <= line_start && line_start < block.range.end)
                .map(|block| &block.kind);
            let mut height = decor::line_metrics(kind).0;

            // строки со standalone-картинками (SPEC §8.2)
            let content = buffer.line_content_range(line_ix);
            let line_text = buffer.line(line_ix);
            if let Some(target) = decor::standalone_image_target(&parsed, &content, &line_text) {
                let target_text = buffer.slice(target);
                height = match self.images.get(&target_text) {
                    Some(ImageState::Loaded {
                        width: w,
                        height: h,
                        ..
                    }) => {
                        let (_, display_h) = images::fit_size(
                            *w,
                            *h,
                            width
                                - crate::editor_element::GUTTER_WIDTH_PX
                                - 2. * crate::editor_element::TEXT_PADDING_PX,
                            images::MAX_IMAGE_HEIGHT,
                        );
                        height + display_h + 8.
                    }
                    _ => height + 8.,
                };
                images.push((line_ix, target_text));
            }
            heights.push(height);
        }
        let mut tops = Vec::with_capacity(total_lines + 1);
        let mut acc = 0.;
        for height in &heights {
            tops.push(acc);
            acc += height;
        }
        tops.push(acc);

        self.layout = Some(EditorLayout {
            epoch,
            version,
            width,
            parsed,
            heights: Rc::new(heights),
            tops: Rc::new(tops),
            images,
        });
    }

    /// Отображаемый размер картинки строки (ширина, высота в px).
    pub(crate) fn image_display_size(&self, target: &str, avail_width: f32) -> Option<(f32, f32)> {
        match self.images.get(target)? {
            ImageState::Loaded { width, height, .. } => Some(images::fit_size(
                *width,
                *height,
                avail_width,
                images::MAX_IMAGE_HEIGHT,
            )),
            _ => None,
        }
    }

    /// Резолвит цель embed в путь файла (SPEC §7.5).
    pub(crate) fn resolve_image_path(&self, target: &str) -> Option<std::path::PathBuf> {
        let direct = self.vault.absolute(target);
        if direct.is_file() {
            return Some(direct);
        }
        let name = std::path::Path::new(target)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())?;
        self.vault
            .find_file_by_name(&name)
            .map(|rel| self.vault.absolute(&rel))
    }

    /// Запускает фоновую загрузку изображения (однократно на цель).
    pub(crate) fn load_missing_image(&mut self, target: String, cx: &mut Context<Self>) {
        if self.images.contains_key(&target) {
            return;
        }
        let Some(path) = self.resolve_image_path(&target) else {
            self.images.insert(target, ImageState::Failed);
            return;
        };
        self.images.insert(target.clone(), ImageState::Loading);
        cx.spawn(async move |this, cx| {
            let decoded = cx
                .background_executor()
                .spawn(async move {
                    let bytes = std::fs::read(path).ok()?;
                    images::decode_image(&bytes)
                })
                .await;
            this.update(cx, |workspace, cx| {
                let state = match decoded {
                    Some(decoded) => ImageState::Loaded {
                        render: decoded.render,
                        width: decoded.width,
                        height: decoded.height,
                    },
                    None => ImageState::Failed,
                };
                workspace.images.insert(target, state);
                // высоты строк изменились
                workspace.layout = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Индекс активного блока (блок со строкой курсора) в кэше раскладки.
    ///
    /// Курсор в конце строки блока относится к следующей строке и не
    /// «включает» его маркеры (SPEC §8.2).
    pub(crate) fn active_block_index(&self) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let tab = self.tabs.get(self.active_tab)?;
        let buffer = tab.editor.buffer();
        let cursor = tab.editor.selection().head;
        let line_start = buffer.line_start(buffer.line_of(cursor));
        layout.parsed.block_index_at(line_start)
    }

    /// Верхняя панель: имя приложения и корень хранилища.
    fn render_header(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(self.palette.border)
            .bg(self.palette.panel)
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(15.))
                    .child("dunite"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(self.palette.text_muted)
                    .child(format!("{}", self.vault.root().display())),
            )
    }

    /// Центральная колонка: панель вкладок + панель редактора.
    fn render_editor_column(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(self.render_tab_bar(cx))
            .child(self.render_editor_pane(cx))
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("workspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CloseTab, _window, cx| this.close_active_tab(cx)))
            .on_action(cx.listener(|this, _: &NextTab, _window, cx| this.activate_next_tab(cx)))
            .on_action(cx.listener(|this, _: &PrevTab, _window, cx| this.activate_prev_tab(cx)))
            .on_action(cx.listener(|this, _: &Quit, window, cx| {
                // все несохранённые вкладки пишем на диск и фиксируем
                // последний снапшот (SPEC §10.3)
                this.save_all();
                this.snapshot_now("выход", cx);
                window.remove_window();
            }))
            .on_action(
                cx.listener(|this, _: &OpenSwitcher, window, cx| this.open_switcher(window, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenSearch, window, cx| this.open_search(window, cx)))
            .on_action(cx.listener(|this, _: &RenameNote, window, cx| this.open_rename(window, cx)))
            .on_action(cx.listener(|this, _: &NavBack, _, cx| this.navigate_back(cx)))
            .on_action(cx.listener(|this, _: &NavForward, _, cx| this.navigate_forward(cx)))
            .on_action(cx.listener(|this, _: &SaveNote, _, cx| this.save_and_snapshot(cx)))
            .on_action(cx.listener(|this, _: &ReloadNote, _, cx| this.reload_active_tab(cx)))
            .on_action(cx.listener(|this, _: &InitGit, _, cx| this.git_init(cx)))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.panel_drag_move(event, cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(self.palette.background)
            .text_color(self.palette.text)
            .child(self.render_header())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_file_tree(cx))
                    .child(self.render_editor_column(cx))
                    .child(self.render_right_sidebar(cx)),
            )
            .child(self.render_status_bar())
            .children(self.render_palette(cx))
            .children(self.render_tree_menu(cx))
    }
}

/// Индекс активной вкладки после закрытия вкладки `closed`.
///
/// Закрытие активной вкладки активирует следующую (или последнюю);
/// закрытие левой сдвигает индекс. Раньше здесь был underflow `usize`,
/// который ронял приложение при закрытии первой вкладки.
fn next_active_tab(active: usize, closed: usize, remaining: usize) -> usize {
    if remaining == 0 {
        0
    } else if closed < active {
        active - 1
    } else if closed == active {
        active.min(remaining - 1)
    } else {
        active
    }
}

#[cfg(test)]
mod tests {
    use super::next_active_tab;

    #[test]
    fn closing_first_active_tab_does_not_underflow() {
        // регрессия: (active - 1) при active == 0 паниковал и ронял приложение
        assert_eq!(next_active_tab(0, 0, 2), 0);
        assert_eq!(next_active_tab(0, 0, 0), 0);
    }

    #[test]
    fn closing_activates_neighbor() {
        assert_eq!(next_active_tab(2, 2, 2), 1); // закрыта последняя — активируем левую
        assert_eq!(next_active_tab(1, 1, 2), 1); // активируем следующую
        assert_eq!(next_active_tab(2, 0, 2), 1); // закрыта левая — индекс сдвигается
        assert_eq!(next_active_tab(1, 2, 2), 1); // закрыта правая — без изменений
    }
}
