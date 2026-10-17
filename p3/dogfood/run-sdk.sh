#!/usr/bin/env bash
# Harness-compatibility drill harness (P3 dogfood) — run inside the privileged L1 container
# under dbus-run-session.
#
# The point: a harness does not handcraft JSON-RPC, it points its MCP client SDK at the pinned
# command (`daiorama mcp`) and lets the SDK negotiate. sdk_client.py drives that exact path
# with the published `mcp` client against a real manager and real compositor sessions.
#
# Self-checking: per-assertion [OK]/[FAIL] + a final FAILS=; exit code = FAILS.
set -u

BIN="${BIN:-/out/hardening/daiorama-p3}"
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-/tmp/hc}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-/tmp/hd}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/hr}"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
SOCK="$XDG_RUNTIME_DIR/daiorama/default/manager.sock"
export DAIORAMA_BIN="$BIN" DAIORAMA_SCOPE=agent

FAILS=0
hr() { echo; echo "=================================================================="; echo "## $*"; echo "=================================================================="; }
check_eq() {
  local label="$1" actual="$2" expected="$3"
  if [ "$actual" = "$expected" ]; then
    echo "[OK]   $label ($actual)"
  else
    echo "[FAIL] $label: got '$actual' want '$expected'"
    FAILS=$((FAILS + 1))
  fi
}
check() {
  local label="$1" needle="$2" file="$3"
  if grep -qF -- "$needle" "$file" 2>/dev/null; then
    echo "[OK]   $label"
  else
    echo "[FAIL] $label :: expected '$needle' in $file"
    FAILS=$((FAILS + 1))
  fi
}

hr "[0] setup (fresh container)"
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

hr "[1] daemon start (cold)"
"$BIN" daemon start; echo "start_rc=$?"
sleep 1
"$BIN" --remote "$SOCK" status > /tmp/sdk.status 2>&1; echo "status_rc=$?"
cat /tmp/sdk.status

hr "[2] raw handshake probes (what the SDK sends first)"
python3 /src/p3/dogfood/sdk_client.py --raw 2>&1
echo "raw_rc=$?"

hr "[3] official SDK client drill (mode=auto, the harness default)"
python3 /src/p3/dogfood/sdk_client.py 2>&1
SDK_RC=$?
echo "sdk_rc=$SDK_RC"
check_eq "SDK drill exited clean" "$SDK_RC" "0"

hr "[4] leak check after the SDK closed its bridge"
echo "-- mutter/Hyprland processes:"
found=0
for p in /proc/[0-9]*; do
  comm=$(cat "$p/comm" 2>/dev/null) || continue
  case "$comm" in
    mutter|Hyprland) found=1; echo "proc ${p#/proc/}: $comm";;
  esac
done
[ "$found" = 0 ] && echo "(none)"
check_eq "no compositor processes left" "$found" "0"
echo "-- session scopes:"
if [ -d /sys/fs/cgroup/daiorama-sessions ]; then
  n=$(find /sys/fs/cgroup/daiorama-sessions -mindepth 1 -maxdepth 1 -type d | wc -l)
  for d in /sys/fs/cgroup/daiorama-sessions/*/; do
    [ -d "$d" ] || continue
    echo "scope $(basename "$d"): procs=[$(tr '\n' ' ' <"$d/cgroup.procs" 2>/dev/null)]"
  done
  check_eq "no live session scopes left" "$n" "0"
else
  echo "(no session subtree)"
fi
echo "-- runtime dirs: $(ls -d /tmp/dr-* 2>/dev/null | wc -l) dr-* dirs"

hr "[5] daemon end + post-state"
"$BIN" daemon end --force; echo "end_rc=$?"
sleep 1
echo "-- daemon.log tail (15):"
tail -n 15 "$XDG_DATA_HOME/daiorama/default/daemon.log" 2>&1

echo
echo "FAILS=$FAILS"
exit "$FAILS"
