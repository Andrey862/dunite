#!/usr/bin/env bash
# Релизная сборка dunite (SPEC §16).
#
# Результат: target/release/dunite
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== сборка релиза (LTO thin, strip) =="
cargo build --release

BIN="$ROOT/target/release/dunite"
"$BIN" --version
echo
echo "бинарник: $BIN"
du -h "$BIN" | cut -f1 | xargs echo "размер:   "
echo
echo "запуск:   $BIN [путь-к-хранилищу]"
