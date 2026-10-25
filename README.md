# dunite

Высокопроизводительный клон Obsidian на Rust + [gpui](https://gpui.rs)
(UI-фреймворк Zed IDE). Без Electron, без веб-технологий.

Спецификация и roadmap: [SPEC.md](SPEC.md).

## Возможности (план)

- Live Preview markdown-редактор (исходник обратим, рендер — WYSIWYG)
- Дерево файлов, табы, сплиты
- Теги, обратные ссылки, изображения
- Полнотекстовый поиск по хранилищу
- Git-бэкапы: автоснапшоты, история, восстановление

Текущий этап: **M0 — каркас** (см. §16 SPEC.md).

## Сборка и запуск

```sh
scripts/build-release.sh              # релизная сборка (LTO thin, strip)
./target/release/dunite [хранилище]   # запуск; без аргумента — текущая папка
```

Флаги: `dunite --version`, `dunite --help`.

**Требования:** Linux (Wayland/X11), GPU с поддержкой Vulkan, `git`
(для бэкапов и синхронизации). Шрифты — системные.

Тестовое хранилище: `testdata/vault-small` (собранные edge-cases —
`ignored/testdata/vault-small/edge-cases`, если развернуть плейграунд).

Проверка качества: `cargo test` (юнит + e2e), `cargo bench` (ядро),
`scripts/bench_runtime.sh 10000` (бюджеты §13, см. docs/BENCH.md).

## Известные ограничения

- сплиты центральной панели — задача после MVP (код в истории git);
- SVG и PDF не рендерятся (SVG — вне MVP, PDF — отдельная задача);
- экспорт в HTML/PDF — вне MVP;
- поиск без стемминга (подстрочные совпадения, как в Obsidian);
- индексация хранилища — на старте (~0.9 с на 10k), не в фоне.

## Структура

Cargo workspace, границы слоёв — SPEC.md §6:

- `crates/dunite-app` — точка входа, wiring подсистем
- `crates/dunite-ui` — компоненты gpui
- `crates/dunite-editor` — текстовый движок (ropey)
- `crates/dunite-markdown` — парсинг markdown
- `crates/dunite-vault` — модель хранилища
- `crates/dunite-index` — граф ссылок и поиск (M3/M4)
- `crates/dunite-git` — git-бэкапы (M5)
- `crates/dunite-settings` — настройки и темы
