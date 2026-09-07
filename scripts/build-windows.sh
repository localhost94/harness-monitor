#!/usr/bin/env bash
# Builds the Windows ARM app plus the Linux agent it drives, into one folder.
#
# The Windows UI cannot read WSL state usefully on its own (Linux pids live in
# another namespace, and opencode's WAL database is not safe to open over 9p),
# so it runs this same program inside WSL and reads snapshots from its stdout.
# Both binaries must ship together.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
TARGET="${TARGET:-aarch64-pc-windows-msvc}"
OUT="${OUT:-dist-windows}"

echo "==> frontend"
bun install --frozen-lockfile 2>/dev/null || bun install
bun run build

# --features custom-protocol is mandatory: without it the webview loads devUrl
# instead of the embedded assets, and the shipped app shows
# "localhost failed to connect".
echo "==> windows binary ($TARGET)"
(cd src-tauri && cargo xwin build --release --features custom-protocol --target "$TARGET")

echo "==> linux agent (native)"
(cd src-tauri && cargo build --release --features custom-protocol)

mkdir -p "$OUT"
cp "src-tauri/target/$TARGET/release/harness-monitor.exe" "$OUT/"
cp "src-tauri/target/release/harness-monitor" "$OUT/harness-monitor-agent"
chmod +x "$OUT/harness-monitor-agent"

cat <<NOTE

built into $OUT/
  harness-monitor.exe     run this on Windows
  harness-monitor-agent   Linux binary it launches through wsl.exe

Keep them side by side, or point at the agent with HM_AGENT_PATH.
Distro comes from HM_WSL_DISTRO, else the first entry of 'wsl.exe -l -q'.
NOTE
