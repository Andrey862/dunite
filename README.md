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
cargo run --release [путь-к-хранилищу]
```

Тестовое хранилище для прогонки: `testdata/vault-small`.

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
