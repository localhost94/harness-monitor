#!/usr/bin/env bash
# Statusline shim for HarnessMonitor.
#
# Claude Code pipes a JSON blob - including rate_limits - into the statusline
# command on every render. That is the freshest plan-quota reading available
# without a network call, so we tee it to a state file and then hand the input
# to whatever statusline the user already had. The original's behaviour and
# output must be untouched: if this script breaks, the user's statusline breaks.
set -uo pipefail

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/harness-monitor"
STATE_FILE="$STATE_DIR/quota.json"
ORIG="${HM_STATUSLINE_ORIG:-$HOME/.claude/statusline-command.orig.sh}"

input=$(cat)

# Best-effort, never fatal, and written atomically so a reader never sees a
# half-written file.
if command -v jq >/dev/null 2>&1; then
  mkdir -p "$STATE_DIR" 2>/dev/null &&
    printf '%s' "$input" |
    jq -c '{at: now, rate_limits: (.rate_limits // null)}' >"$STATE_FILE.tmp" 2>/dev/null &&
    mv -f "$STATE_FILE.tmp" "$STATE_FILE" 2>/dev/null
fi

if [ -x "$ORIG" ] || [ -f "$ORIG" ]; then
  printf '%s' "$input" | exec bash "$ORIG"
fi
