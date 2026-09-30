#!/usr/bin/env bash
# Screenshots the UI at its real window width, for design review without a screen.
#
# Two traps this works around:
#   1. inotify does not fire on /mnt/c (drvfs), so `vite dev` serves stale
#      modules forever - always build, never rely on HMR here;
#   2. Chrome enforces a ~500px minimum window width, so a 420px screenshot is
#      a crop of a 512px layout. `?w=` constrains #root instead.
#
# Extra args become query params, which is how the shape variants get
# previewed: shape=line, view=finished, only=idle, many=12. Each arg is one
# param, joined with & - a space would be a literal space in the URL and Chrome
# would drop everything after it.
#
# Chrome screenshots the whole window, so a 440px widget in the 520px minimum
# window comes out with a dead margin down the right. The last step crops back
# to the requested size plus a hairline, which is what the README pictures are.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

WIDTH="${1:-420}"
HEIGHT="${2:-700}"
OUT="${3:-preview.png}"
shift 3 2>/dev/null || true
EXTRA=""
for param in "$@"; do
  EXTRA="${EXTRA:+$EXTRA&}$param"
done
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
  "http://localhost:$PORT/?w=$WIDTH&h=$HEIGHT&$EXTRA" 2>&1 | tail -1

# The kill runs above via the trap, but vite can still hold the port for a
# moment, which would make the next run fail on --strictPort.
kill "$SERVER" 2>/dev/null || true
wait "$SERVER" 2>/dev/null || true

python3 - "$OUT" "$WIDTH" "$HEIGHT" <<'PY'
import sys
from PIL import Image

path, width, height = sys.argv[1], int(sys.argv[2]) + 2, int(sys.argv[3]) + 2
image = Image.open(path)
if image.width > width:
    image = image.crop((0, 0, width, min(height, image.height)))
image.save(path)
PY
