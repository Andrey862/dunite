# Vendor patch: xattr 0.2.3

Копия крейта `xattr 0.2.3` (license: MIT/Apache-2.0) с одним патчем.

## Зачем

Цепочка зависимостей gpui: `gpui → gpui_http_client → zed-async-tar → xattr 0.2.3`.

`libc 0.2.190` удалил константу `libc::ENOATTR` на Linux/Android (на этих
платформах это алиас `ENODATA`; константа была deprecated с 2019 года —
rust-lang/libc#1356). `xattr 0.2.3` обращается к `libc::ENOATTR` и поэтому
больше не компилируется. Апстрима нет: `xattr 0.2.3` — последний релиз
линейки 0.2, исправление есть только в `xattr 1.x`, который `zed-async-tar`
не принимает (`version = "0.2"`). Обсуждение той же проблемы:
Far-Beyond-Pulsar/WGPUI#275.

## Что изменено

Единственный файл: `src/sys/mod.rs` — определение `ENOATTR` в макросе
`platforms!`:

- Linux/Android: `ENOATTR = ENODATA` (семантически идентично исходному);
- macOS/BSD: без изменений, `libc::ENOATTR` там существует.

## Обновление

Когда gpui перейдёт на `xattr 1.x` (или `zed-async-tar` выпустит фикс) —
удалить `vendor/xattr` и запись `[patch.crates-io]` в корневом `Cargo.toml`.
