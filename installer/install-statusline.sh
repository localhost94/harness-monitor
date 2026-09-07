#!/usr/bin/env bash
# Installs (or removes) the HarnessMonitor statusline shim.
#
# statusLine.command is a single global slot that is usually already occupied,
# so this wraps the existing command instead of replacing it, and is safe to
# re-run: a second install is a no-op, not a shim wrapping a shim.
set -euo pipefail

SETTINGS="$HOME/.claude/settings.json"
SHIM_SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/harness-monitor-statusline.sh"
SHIM_DEST="$HOME/.claude/harness-monitor-statusline.sh"
ORIG_DEST="$HOME/.claude/statusline-command.orig.sh"

usage() { echo "usage: $0 [--install|--uninstall|--status]"; exit 1; }

read_command() {
  python3 - "$SETTINGS" <<'PY'
import json, sys
try:
    with open(sys.argv[1]) as fh:
        print(json.load(fh).get("statusLine", {}).get("command", ""))
except Exception:
    print("")
PY
}

write_command() {
  python3 - "$SETTINGS" "$1" <<'PY'
import json, shutil, sys
path, command = sys.argv[1], sys.argv[2]
with open(path) as fh:
    data = json.load(fh)
shutil.copyfile(path, path + ".harness-monitor.bak")
data.setdefault("statusLine", {"type": "command"})["command"] = command
with open(path, "w") as fh:
    json.dump(data, fh, indent=2)
    fh.write("\n")
PY
}

current=$(read_command)

case "${1:---install}" in
  --status)
    echo "statusLine.command: ${current:-<unset>}"
    [ -f "$ORIG_DEST" ] && echo "wrapped original:   $ORIG_DEST"
    echo "quota state:        ${XDG_STATE_HOME:-$HOME/.local/state}/harness-monitor/quota.json"
    ;;

  --install)
    if [[ "$current" == *harness-monitor-statusline* ]]; then
      echo "already installed; nothing to do"
      exit 0
    fi
    cp "$SHIM_SRC" "$SHIM_DEST"
    chmod +x "$SHIM_DEST"
    if [ -n "$current" ]; then
      # Preserve the user's statusline verbatim, whatever shape it has.
      printf '#!/usr/bin/env bash\nexec %s\n' "$current" >"$ORIG_DEST"
      chmod +x "$ORIG_DEST"
      echo "wrapped existing statusline -> $ORIG_DEST"
    else
      printf '#!/usr/bin/env bash\nexit 0\n' >"$ORIG_DEST"
      chmod +x "$ORIG_DEST"
      echo "no existing statusline; shim will emit nothing"
    fi
    write_command "bash $SHIM_DEST"
    echo "installed. settings.json backed up to $SETTINGS.harness-monitor.bak"
    ;;

  --uninstall)
    if [[ "$current" != *harness-monitor-statusline* ]]; then
      echo "shim not installed; nothing to do"
      exit 0
    fi
    if [ -f "$ORIG_DEST" ]; then
      restored=$(grep -m1 '^exec ' "$ORIG_DEST" | sed 's/^exec //')
      if [ -n "$restored" ]; then
        write_command "$restored"
        echo "restored: $restored"
      fi
    fi
    rm -f "$SHIM_DEST"
    echo "uninstalled (kept $ORIG_DEST for reference)"
    ;;

  *) usage ;;
esac
