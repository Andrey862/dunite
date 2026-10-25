//! E2E-сценарии (SPEC §15): модель + UI-проводка + диск, headless.
//!
//! Уровень покрытия: реальный ввод (`simulate_input` / `simulate_keystrokes`
//! через IME и keymap), реальные файлы на диске (tempdir), реальный git;
//! таймеры (автосейв 500 мс) двигаются детерминированно через
//! `advance_clock` — без sleep и флаки. Координатные клики мыши здесь
//! сознательно не используются: они хрупки, а логика кликов покрыта
//! юнит-тестами; через e2e идут клавиатурные и текстовые пути.

use std::fs;
use std::time::Duration;

use dunite_git::GitRepo;
use dunite_settings::Settings;
use dunite_ui::Workspace;
use dunite_vault::Vault;
use gpui::{TestAppContext, VisualTestContext, WindowHandle};

/// Фикстура: хранилище с заметками, ссылающимися друг на друга.
fn fixture() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Главная.md"), "см. [[Цель]]\n").unwrap();
    fs::write(dir.path().join("Цель.md"), "тело заметки\n").unwrap();
    fs::create_dir(dir.path().join("папка")).unwrap();
    fs::write(dir.path().join("папка/Вложенная.md"), "вложенная заметка\n").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, vault)
}

fn open_workspace(
    cx: &mut TestAppContext,
    vault: Vault,
) -> (WindowHandle<Workspace>, VisualTestContext) {
    // те же бинды, что в main.rs — иначе simulate_keystrokes не найдёт действия
    cx.update(|cx| {
        cx.bind_keys(dunite_ui::global_key_bindings());
        cx.bind_keys(dunite_ui::key_bindings());
        cx.bind_keys(dunite_ui::palette_key_bindings());
    });
    let window = cx.add_window(|window, cx| {
        let focus_handle = cx.focus_handle();
        let palette_focus = cx.focus_handle();
        focus_handle.focus(window);
        Workspace::new(vault, Settings::default(), focus_handle, palette_focus)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    (window, vcx)
}

/// Активная заметка (для ассертов).
fn active_note(window: &WindowHandle<Workspace>, cx: &TestAppContext) -> Option<String> {
    window
        .read_with(cx, |workspace, _| {
            workspace
                .tabs
                .get(workspace.active_tab)
                .map(|tab| tab.note_id.as_str().to_string())
        })
        .unwrap()
}

fn open_note(window: &WindowHandle<Workspace>, cx: &mut TestAppContext, rel: &str) {
    window
        .update(cx, |workspace, _window, cx| workspace.open_note(rel, cx))
        .unwrap();
}

/// 1. Базовый сценарий: ввод через IME попадает на диск после автосейва.
#[gpui::test]
async fn smoke_edit_save(cx: &mut TestAppContext) {
    let (dir, vault) = fixture();
    let (window, mut vcx) = open_workspace(cx, vault);

    open_note(&window, cx, "Цель.md");
    vcx.run_until_parked();
    vcx.simulate_input("привет из теста");
    vcx.run_until_parked();

    // файл ещё не сохранён: дебаунс автосейва 500 мс
    let before = fs::read_to_string(dir.path().join("Цель.md")).unwrap();
    assert!(!before.contains("привет из теста"), "{before}");

    cx.executor().advance_clock(Duration::from_millis(600));
    vcx.run_until_parked();
    let after = fs::read_to_string(dir.path().join("Цель.md")).unwrap();
    assert!(after.contains("привет из теста"), "{after}");
}

/// 2. Серии набора короче дебаунса сливаются в один автосейв.
#[gpui::test]
async fn autosave_coalesces_typing_bursts(cx: &mut TestAppContext) {
    let (dir, vault) = fixture();
    let (window, mut vcx) = open_workspace(cx, vault);

    open_note(&window, cx, "Цель.md");
    vcx.run_until_parked();
    vcx.simulate_input("раз");
    cx.executor().advance_clock(Duration::from_millis(200));
    vcx.run_until_parked();
    vcx.simulate_input(" два");
    vcx.run_until_parked();

    cx.executor().advance_clock(Duration::from_millis(600));
    vcx.run_until_parked();
    let text = fs::read_to_string(dir.path().join("Цель.md")).unwrap();
    assert!(text.contains("раз два"), "{text}");
}

/// 3. Quick switcher: Ctrl+P, ввод, Enter открывает заметку.
#[gpui::test]
async fn quick_switcher_opens_note(cx: &mut TestAppContext) {
    let (_dir, vault) = fixture();
    let (window, mut vcx) = open_workspace(cx, vault);

    vcx.run_until_parked();
    vcx.simulate_keystrokes("ctrl-p");
    vcx.run_until_parked();
    vcx.simulate_input("вложен");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();

    assert_eq!(
        active_note(&window, cx).as_deref(),
        Some("папка/Вложенная.md")
    );
}

/// 4. Переименование переписывает ссылающиеся ссылки (SPEC §7.2).
#[gpui::test]
async fn rename_updates_links(cx: &mut TestAppContext) {
    let (dir, vault) = fixture();
    let (window, mut vcx) = open_workspace(cx, vault);

    window
        .update(cx, |workspace, _window, cx| {
            workspace.tree_rename_entry("Цель.md", "Новая.md", cx);
        })
        .unwrap();
    vcx.run_until_parked();

    let text = fs::read_to_string(dir.path().join("Главная.md")).unwrap();
    assert!(text.contains("[[Новая]]"), "{text}");
    assert!(!text.contains("[[Цель]]"), "{text}");
    assert!(dir.path().join("Новая.md").is_file());
    assert!(!dir.path().join("Цель.md").exists());
    assert_eq!(active_note(&window, cx), None); // вкладок не было
}

/// 5. Поиск: Ctrl+Shift+F, запрос, Enter открывает найденную заметку.
#[gpui::test]
async fn search_finds_and_opens(cx: &mut TestAppContext) {
    let (_dir, vault) = fixture();
    let (window, mut vcx) = open_workspace(cx, vault);

    vcx.run_until_parked();
    vcx.simulate_keystrokes("ctrl-shift-f");
    vcx.run_until_parked();
    vcx.simulate_input("тело заметки");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();

    assert_eq!(active_note(&window, cx).as_deref(), Some("Цель.md"));
}

/// 6. Watcher: внешнее изменение файла перечитывает вкладку.
#[gpui::test]
async fn watcher_reloads_tab(cx: &mut TestAppContext) {
    let (dir, vault) = fixture();
    let (window, mut vcx) = open_workspace(cx, vault);
    window
        .update(cx, |workspace, _window, cx| workspace.spawn_watcher(cx))
        .unwrap();
    open_note(&window, cx, "Цель.md");
    vcx.run_until_parked();

    fs::write(dir.path().join("Цель.md"), "внешний текст\n").unwrap();

    // inotify приходит в реальном времени (дебаунс watcher'а 300 мс — тоже
    // реальное время), а поллинг вкладки — детерминированный таймер 200 мс:
    // спим по-настоящему и двигаем часы, пока вкладка не перечитается
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut reloaded = false;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        cx.executor().advance_clock(Duration::from_millis(300));
        vcx.run_until_parked();
        let text = window
            .update(cx, |workspace, _window, _cx| {
                workspace
                    .tabs
                    .first()
                    .map(|tab| tab.editor.text())
                    .unwrap_or_default()
            })
            .unwrap();
        if text.contains("внешний текст") {
            reloaded = true;
            break;
        }
    }
    assert!(reloaded, "вкладка не перечиталась после внешнего изменения");
}

/// 7. Регрессия: закрытие первой активной вкладки не должно ронять
/// приложение (underflow usize вычисления активной вкладки).
#[gpui::test]
async fn close_first_tab_no_panic(cx: &mut TestAppContext) {
    let (_dir, vault) = fixture();
    let (window, mut vcx) = open_workspace(cx, vault);

    open_note(&window, cx, "Главная.md");
    open_note(&window, cx, "Цель.md");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("ctrl-w"); // закрывает активную (Цель)
    vcx.run_until_parked();
    assert_eq!(active_note(&window, cx).as_deref(), Some("Главная.md"));

    // теперь закрываем первую вкладку — раньше здесь была паника
    vcx.simulate_keystrokes("ctrl-w");
    vcx.run_until_parked();
    assert_eq!(active_note(&window, cx), None);
}

/// 8. Git: удаление фиксируется коммитом, восстановление возвращает файл.
#[gpui::test]
async fn git_delete_restore(cx: &mut TestAppContext) {
    let (dir, vault) = fixture();
    GitRepo::init(dir.path()).unwrap(); // базовый снапшот создаётся сам
    let (window, mut vcx) = open_workspace(cx, vault);

    window
        .update(cx, |workspace, _window, cx| {
            workspace.tree_delete("Цель.md", cx);
        })
        .unwrap();
    vcx.run_until_parked();
    assert!(!dir.path().join("Цель.md").exists());

    // коммит удаления хранит файл (через предков) — восстанавливаем из него
    let git = GitRepo::open(dir.path()).unwrap().unwrap();
    let commits = git.log(50).unwrap();
    let removal = commits
        .iter()
        .find(|commit| commit.summary.starts_with("удаление:"))
        .expect("коммит удаления должен существовать");
    window
        .update(cx, |workspace, _window, cx| {
            workspace.restore_from_commit(&removal.id, "Цель.md", cx);
        })
        .unwrap();
    vcx.run_until_parked();

    assert!(dir.path().join("Цель.md").is_file());
    assert_eq!(
        fs::read_to_string(dir.path().join("Цель.md")).unwrap(),
        "тело заметки\n"
    );
}
