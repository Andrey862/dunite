# AGENTS.md

Инструкция для агентов и контрибьюторов. Контракт проекта — [SPEC.md](SPEC.md)
(русский): бюджеты — §13, тесты — §15, план — §16. Любое изменение решения —
запись в журнал §18 (дата + причина). Код и идентификаторы — English,
документация, комментарии и строки UI — русский.

## Команды

```sh
cargo fmt                                   # формат (обязателен перед коммитом)
cargo test --workspace                      # полный прогон
cargo test -p dunite-editor                 # быстрый цикл по крейту
cargo test -p dunite-ui --test e2e          # e2e (8 сценариев)
cargo bench                                 # criterion: ядро (editor/index/markdown)
scripts/bench_runtime.sh 10000              # бюджеты §13 по времени (см. docs/BENCH.md)
scripts/build-release.sh                    # релиз → target/release/dunite
./target/release/dunite testdata/vault-small # запуск (нужен GPU/Vulkan)
DUNITE_OPEN=Заметка.md ./target/release/dunite testdata/vault-small
```

### Можно без спроса

- фичи и фиксы в рамках SPEC.md, тесты, рефакторинг внутри крейта;
- обновление docs/BENCH.md по фактическим замерам;
- запись в §18 по факту изменённого решения.

### Спросить пользователя перед

- изменением текста SPEC.md (это контракт, не черновик);
- обновлением зависимостей — особенно gpui (пин намеренный);
- добавлением новых тяжёлых зависимостей;
- работой в `vendor/`, `git push`, переписыванием истории;
- задачами вне текущей вехи (§16).

### Нельзя

- удалять `[patch.crates-io] xattr` — сборка падает (см. vendor/xattr/VENDOR-PATCH.md);
- трогать `ignored/` (плейграунд пользователя, в .gitignore) и коммитить
  сгенерированные хранилища;
- допускать O(N) на кадр в рендере — виртуализация обязательна (§13);
- паниковать в UI-путях: ошибка → `status_message` + `tracing::error!`;
- смешивать языки (UI/доки — русский, идентификаторы — English).

## Структура (где что искать)

| Путь | Содержимое |
|---|---|
| `SPEC.md` | контракт, журнал решений §18 |
| `crates/dunite-app/src/main.rs` | точка входа: окно, keymap, `DUNITE_OPEN` |
| `crates/dunite-ui/src/workspace.rs` | центр: табы, git-кэши, watcher, timeline |
| `crates/dunite-ui/src/{editor_pane,editor_element,decor}.rs` | редактор, live preview, concealment |
| `crates/dunite-ui/src/{file_tree,tree_menu}.rs` | дерево (виртуализация), меню, DnD |
| `crates/dunite-ui/src/palette.rs` | quick switcher, поиск, переименование |
| `crates/dunite-ui/src/{sidebar,keys,actions,theme}.rs` | outline/backlinks/теги, клавиши, тема |
| `crates/dunite-editor/src/` | модель текста: буфер (ropey), undo, курсоры |
| `crates/dunite-markdown/src/lib.rs` | парсер markdown + YAML frontmatter |
| `crates/dunite-vault/src/` | файловое ядро, NoteId (`id.rs`), watcher (`watch.rs`) |
| `crates/dunite-index/src/` | LinkGraph (`graph.rs`), in-memory поиск (`search.rs`) |
| `crates/dunite-git/src/lib.rs` | снапшоты/лог/diff/restore (git2), push/pull (git CLI) |
| `scripts/`, `docs/BENCH.md`, `testdata/vault-small` | инструменты, замеры, тестовое хранилище |

## Стек (зафиксирован, SPEC §5)

- Rust 2024 (`rust-version = 1.92`), workspace `version = 0.1.0`
- **gpui `=0.2.2`** — пин намеренный (нестабильный API Zed); обновление — только
  с подтверждения пользователя
- ropey 1.6, unicode-segmentation 1, pulldown-cmark 0.13
- serde 1 + serde_yml 0.0.12, toml 0.8, dirs 6, anyhow 1, thiserror 2
- git2 0.20 `default-features = false` (без OpenSSL), notify 8
- nucleo-matcher 0.3 (fuzzy в палитре), image 0.25 (вложения), tracing 0.1
- dev: tempfile 3, proptest 1, criterion 0.5
- вендор: `vendor/xattr` — патч зависимости gpui (ENOAATTR), см. VENDOR-PATCH.md

## Стиль кода

Модуль — с `//!` и ссылкой на SPEC; публичное API — с `///` (русский):

```rust
//! Слежение за изменениями файлов хранилища (SPEC §11).
//!
//! Обёртка над `notify` с дебаунсом: события отдаются пачками после
//! 300 мс тишины, чтобы не переиндексировать на каждое касание файла.

/// Начинает наблюдение за `root` (рекурсивно).
pub fn new(root: &Path) -> notify::Result<Self> { /* … */ }
```

Ошибки в UI-путях — без паники:

```rust
Err(err) => {
    self.status_message = format!("git: ошибка снапшота: {err}");
    tracing::error!("снапшот не создан: {err}");
}
```

Коммиты: `<веха|тема>: <описание> (SPEC §N)` — напр. `M6.6: e2e-набор (SPEC §15)`,
`perf: капы в diff-превью коммита`. Магические числа — именованными константами.

GPUI-готчи (проверены практикой):

- `ElementId` обязан быть уникальным — иначе клики «съедаются»;
- события всплывают через перекрывающиеся hitbox'ы — `stop_propagation()` / `.occlude()`;
- `Div::on_click` требует `.id()`; скролл — `ScrollHandle` + `track_scroll` + spacer'ы
  (пример — `file_tree.rs`);
- кэши инвалидировать по `(epoch, version)`, не по ссылкам (пример — layout-кэш редактора).

## Тестирование

- юниты — в крейтах рядом с кодом; property-тесты — `crates/dunite-editor/tests/properties.rs`;
- **e2e** — `crates/dunite-ui/tests/e2e.rs` (`#[gpui::test]`, headless):
  - реальный ввод: `simulate_input` / `simulate_keystrokes` (IME и keymap —
    бинды регистрировать так же, как в `main.rs`);
  - таймеры детерминированные: `cx.executor().advance_clock(..)` + `run_until_parked()`,
    без `sleep` (исключение — watcher-сценарий: inotify в реальном времени,
    polling-цикл с дедлайном);
  - координатные клики мыши не использовать (хрупки к раскладке) — логика кликов
    покрывается юнитами;
- перед сдачей: `cargo fmt && cargo test --workspace` — всё зелёное, флаки нет
  (прогнать e2e 2–3 раза);
- перф-регрессии: `scripts/bench_runtime.sh 10000` против бюджетов §13,
  цифры фиксировать в `docs/BENCH.md`.
