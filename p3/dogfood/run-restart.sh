#!/usr/bin/env bash
# Manager-restart drill (P3 dogfood) — run inside the privileged L1 container.
#
# A harness keeps one MCP session open across a manager crash/restart. This drills the
# first post-restart call through the published client SDK, against real compositor
# sessions. Self-checking: per-assertion [OK]/[FAIL] + final FAILS=; exit code = FAILS.
set -u

BIN="${BIN:-/out/hardening/daiorama-df2}"
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-/tmp/hc}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-/tmp/hd}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/hr}"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
export DAIORAMA_BIN="$BIN" DAIORAMA_SCOPE=agent

echo "== setup"
id
echo "binary: $(ls -la "$BIN" 2>&1)"
"$BIN" version
"$BIN" config setup; echo "config_setup_rc=$?"
SCOPES="$XDG_CONFIG_HOME/daiorama/default/scopes"
mkdir -p "$SCOPES"
printf '(sessions: (max: Some(2), visibility: own),\n grant: (categories: ["input"]))\n' > "$SCOPES/agent.ron"
echo "scope file: $(cat "$SCOPES/agent.ron")"
export PYTHONPATH="${SDK_PATH:-/sdk/lib/python3.14/site-packages}"
python3 - <<'PY'
import importlib.metadata as m
print("sdk mcp==", m.version("mcp"))
PY

echo
echo "== manager-restart drill"
python3 /src/p3/dogfood/manager_restart.py
RC=$?
echo "drill_rc=$RC"

echo
echo "== post-run leak check"
found=0
for p in /proc/[0-9]*; do
  comm=$(cat "$p/comm" 2>/dev/null) || continue
  case "$comm" in
    mutter|Hyprland) found=1; echo "proc ${p#/proc/}: $comm";;
  esac
done
[ "$found" = 0 ] && echo "(no compositors)"
echo "compositors_left=$found"
echo "runtime_dirs=$(ls -d /tmp/dr-* 2>/dev/null | wc -l)"

echo
echo "FAILS=$RC"
exit "$RC"
