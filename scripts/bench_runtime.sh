#!/usr/bin/env bash
# Замеры рантайма против бюджетов SPEC §13: старт, открытие хранилища
# из 10k заметок, память.
#
# Использование: scripts/bench_runtime.sh [кол-во заметок] (по умолчанию 10000)
set -euo pipefail

NOTES="${1:-10000}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VAULT="/tmp/dunite-bench-vault"
LOG="/tmp/dunite-bench.log"
APP="$ROOT/target/release/dunite"

cd "$ROOT"
echo "== сборка релиза =="
cargo build --release --quiet

echo "== генерация хранилища ($NOTES заметок) =="
rm -rf "$VAULT"
cargo run --release --quiet --example gen_vault -- "$VAULT" "$NOTES"

echo "== запуск =="
rm -f "$LOG"
START=$(date +%s%N)
DUNITE_OPEN="заметка-00000.md" "$APP" "$VAULT" > "$LOG" 2>&1 &
PID=$!

# маркер: хранилище разобрано (скан файлов)
for _ in $(seq 1 400); do
  grep -q "хранилище открыто" "$LOG" && break
  sleep 0.05
done
SCANNED=$(date +%s%N)
grep -q "хранилище открыто" "$LOG" || { echo "не дождались открытия хранилища"; kill "$PID" 2>/dev/null; exit 1; }

# маркер: граф ссылок и поисковый индекс построены
for _ in $(seq 1 400); do
  grep -q "граф и индекс построены" "$LOG" && break
  sleep 0.05
done
GRAPHED=$(date +%s%N)

# маркер: окно и планировщики запущены ≈ интерактивность
for _ in $(seq 1 400); do
  grep -q "watcher запущен" "$LOG" && break
  sleep 0.05
done
READY=$(date +%s%N)

sleep 5
RSS_KB=$(ps -o rss= -p "$PID" | tr -d ' ')
kill "$PID" 2>/dev/null || true

SCAN_MS=$(( (SCANNED - START) / 1000000 ))
GRAPH_MS=$(( (GRAPHED - SCANNED) / 1000000 ))
READY_MS=$(( (READY - START) / 1000000 ))
RSS_MB=$(( RSS_KB / 1024 ))

echo
echo "скан файлов ($NOTES заметок):          ${SCAN_MS} мс"
echo "граф ссылок + индекс поиска:           ${GRAPH_MS} мс"
echo "итого до готовности окна:              ${READY_MS} мс   [бюджет §13: открытие 10k < 2000 мс]"
echo "память (RSS), открытое хранилище:      ${RSS_MB} МБ  [бюджет §13: см. docs/BENCH.md]"
echo
echo "бюджет «холодный старт < 300 мс» меряется на малом хранилище:"
echo "запустите scripts/bench_runtime.sh 50 и смотрите «итого до готовности окна»."
echo "лог: $LOG"
