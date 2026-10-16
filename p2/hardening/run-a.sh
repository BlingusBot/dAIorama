#!/usr/bin/env bash
# Hardening round (a) — full lifecycle churn on the real chain, run inside the privileged
# L1 container: create several real sessions, list/status (renderer marks), close some,
# SIGKILL the manager mid-session, start it again — the startup sweep must reap the orphaned
# scopes and compositors. Raw evidence: this script's stdout.
set -u

BIN="${BIN:-/out/hardening/daiorama}"
export XDG_CONFIG_HOME=/tmp/hc XDG_DATA_HOME=/tmp/hd XDG_RUNTIME_DIR=/tmp/hr
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
SOCK="$XDG_RUNTIME_DIR/daiorama/default/manager.sock"
DLOG="$XDG_DATA_HOME/daiorama/default/daemon.log"
SUBTREE=/sys/fs/cgroup/daiorama-sessions

scan_compositors() {
  local found=0
  for p in /proc/[0-9]*; do
    local comm
    comm=$(cat "$p/comm" 2>/dev/null) || continue
    case "$comm" in
      mutter|Hyprland)
        found=1
        echo "proc ${p#/proc/}: $comm :: $(tr '\0' ' ' <"$p/cmdline" 2>/dev/null | cut -c1-140)"
        ;;
    esac
  done
  [ "$found" = 0 ] && echo "(no mutter/Hyprland processes)"
  return 0
}

show_scopes() {
  if [ ! -d "$SUBTREE" ]; then echo "(no session subtree at $SUBTREE)"; return 0; fi
  local empty=1
  for d in "$SUBTREE"/*/; do
    [ -d "$d" ] || continue
    empty=0
    echo "scope $(basename "$d"): procs=[$(tr '\n' ' ' <"$d/cgroup.procs" 2>/dev/null)]"
  done
  [ "$empty" = 1 ] && echo "(subtree present, no scope dirs)"
  return 0
}

hr() { echo; echo "=================================================================="; echo "## $*"; echo "=================================================================="; }

hr "[0] pre-state (fresh container)"
id
echo "binary: $(ls -la "$BIN" 2>&1)"
"$BIN" version; echo "version_rc=$?"
echo "nsroot subtree_control: $(cat /sys/fs/cgroup/cgroup.subtree_control 2>&1)"
echo "subtree listing: $(ls -d $SUBTREE 2>&1)"
echo "-- compositors:"; scan_compositors
echo "-- scopes:"; show_scopes

hr "[1] daemon start (cold)"
"$BIN" daemon start; echo "start_rc=$?"
sleep 1
MGR_PID=$("$BIN" --remote "$SOCK" status --json 2>/dev/null | sed -n 's/.*"pid": *\([0-9][0-9]*\).*/\1/p' | head -1)
if [ -z "${MGR_PID:-}" ]; then MGR_PID=$(pgrep -x daiorama 2>/dev/null | head -1); fi
echo "manager_pid=${MGR_PID:-unknown}"
echo "-- status:"; "$BIN" --remote "$SOCK" status; echo "status_rc=$?"
echo "-- nsroot subtree_control now: $(cat /sys/fs/cgroup/cgroup.subtree_control 2>&1)"
echo "-- keeper procs: $(wc -l < /sys/fs/cgroup/keeper/cgroup.procs 2>&1)"
echo "-- scopes:"; show_scopes

hr "[2] three real sessions"
S1=$("$BIN" --remote "$SOCK" new); echo "new1_rc=$? id=${S1:-<none>}"
S2=$("$BIN" --remote "$SOCK" new); echo "new2_rc=$? id=${S2:-<none>}"
S3=$("$BIN" --remote "$SOCK" new); echo "new3_rc=$? id=${S3:-<none>}"
echo "-- list:"; "$BIN" --remote "$SOCK" list; echo "list_rc=$?"
for s in "${S1:-}" "${S2:-}" "${S3:-}"; do
  [ -n "$s" ] || continue
  echo "-- status $s:"; "$BIN" --remote "$SOCK" status "$s"; echo "rc=$?"
done
echo "-- raw Renderer lines from session instance logs:"
for lg in /tmp/dr-*/hypr/*/hyprland.log; do
  [ -f "$lg" ] || continue
  echo "$lg :: $(grep -m1 'Renderer:' "$lg" 2>/dev/null | tr -d '\r')"
done
echo "-- scopes:"; show_scopes
echo "-- compositors:"; scan_compositors

hr "[3] close one session (${S1:-<none>})"
[ -n "${S1:-}" ] && { "$BIN" --remote "$SOCK" close "$S1"; echo "close_rc=$?"; }
echo "-- list after close:"; "$BIN" --remote "$SOCK" list
echo "-- scopes after close:"; show_scopes

hr "[4] SIGKILL the manager mid-session (pid ${MGR_PID:-unknown})"
kill -9 "${MGR_PID:?manager pid unknown}"; echo "kill_rc=$?"
sleep 2
echo "-- socket file after kill: $(ls -la "$SOCK" 2>&1)"
echo "-- compositors after kill:"; scan_compositors
echo "-- scopes after kill:"; show_scopes

hr "[5] daemon start again — the startup sweep must reap"
"$BIN" daemon start; echo "restart_rc=$?"
sleep 1
echo "-- daemon.log (tail 40):"; tail -n 40 "$DLOG" 2>&1
echo "-- startup sweep lines:"; grep -n 'startup sweep' "$DLOG" 2>&1
echo "-- compositors after restart:"; scan_compositors
echo "-- scopes after restart:"; show_scopes
echo "-- status:"; "$BIN" --remote "$SOCK" status; echo "status_rc=$?"
echo "-- list:"; "$BIN" --remote "$SOCK" list; echo "list_rc=$?"

hr "[6] end + post-state"
"$BIN" daemon end --force; echo "end_rc=$?"
echo "-- compositors final:"; scan_compositors
echo "-- scopes final:"; show_scopes
echo "-- session runtime dirs left: $(ls -d /tmp/dr-* 2>/dev/null | tr '\n' ' ')"

echo "RUN-A-DONE"
