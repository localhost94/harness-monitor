#!/usr/bin/env bash
# Screenshots the UI at its real window width, for design review without a screen.
#
# Two traps this works around:
#   1. inotify does not fire on /mnt/c (drvfs), so `vite dev` serves stale
#      modules forever - always build, never rely on HMR here;
#   2. Chrome enforces a ~500px minimum window width, so a 420px screenshot is
#      a crop of a 512px layout. `?w=` constrains #root instead.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

WIDTH="${1:-420}"
HEIGHT="${2:-700}"
OUT="${3:-preview.png}"
PORT=1422
CHROME="/mnt/c/Program Files/Google/Chrome/Application/chrome.exe"

bun run build >/dev/null
bunx vite preview --port "$PORT" --strictPort >/dev/null 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null || true' EXIT

for _ in $(seq 1 30); do
  curl -sf -o /dev/null "http://localhost:$PORT/" && break
  sleep 0.5
done

WIN=$((WIDTH < 520 ? 520 : WIDTH + 40))
"$CHROME" --headless=new --disable-gpu --hide-scrollbars \
  --window-size="$WIN,$HEIGHT" --virtual-time-budget=3000 \
  --screenshot="$(wslpath -w "$PWD/$OUT")" \
  "http://localhost:$PORT/?w=$WIDTH" 2>&1 | tail -1
