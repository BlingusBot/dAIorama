#!/usr/bin/env bash
# Hardening round (a) — full lifecycle churn on the real chain, run inside the privileged
# L1 container: create several real sessions, list/status (renderer marks), close some,
# SIGKILL the manager mid-session, start it again — the startup sweep must reap the orphaned
# scopes and compositors (/proc + the scope subtree checked after). Self-checking since
# round 2: per-assertion [OK]/[FAIL] + a final FAILS=; exit code = FAILS.
# Raw evidence: this script's stdout.
set -u

BIN="${BIN:-/out/hardening/daiorama}"
export XDG_CONFIG_HOME=/tmp/hc XDG_DATA_HOME=/tmp/hd XDG_RUNTIME_DIR=/tmp/hr
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
SOCK="$XDG_RUNTIME_DIR/daiorama/default/manager.sock"
DLOG="$XDG_DATA_HOME/daiorama/default/daemon.log"
SUBTREE=/sys/fs/cgroup/daiorama-sessions

FAILS=0
check() { # check <label> <fixed-needle> <file>
  local label="$1" needle="$2" file="$3"
  if grep -qF -- "$needle" "$file" 2>/dev/null; then
    echo "[OK]   $label"
  else
    echo "[FAIL] $label :: expected '$needle' in $file"
    FAILS=$((FAILS + 1))
  fi
}
check_rc() { # check_rc <label> <actual> <expected>
  local label="$1" actual="$2" expected="$3"
  if [ "$actual" = "$expected" ]; then
    echo "[OK]   $label (rc=$actual)"
  else
    echo "[FAIL] $label rc=$actual want $expected"
    FAILS=$((FAILS + 1))
  fi
}
check_eq() { # check_eq <label> <actual> <expected>
  local label="$1" actual="$2" expected="$3"
  if [ "$actual" = "$expected" ]; then
    echo "[OK]   $label ($actual)"
  else
    echo "[FAIL] $label: got '$actual' want '$expected'"
    FAILS=$((FAILS + 1))
  fi
}
check_ge() { # check_ge <label> <actual> <min>
  local label="$1" actual="$2" min="$3"
  if [ "${actual:-0}" -ge "$min" ] 2>/dev/null; then
    echo "[OK]   $label ($actual >= $min)"
  else
    echo "[FAIL] $label: got '$actual' want >= $min"
    FAILS=$((FAILS + 1))
  fi
}
pgrep_count() { # daiorama process count, robust to pgrep's exit-1-on-zero
  local n
  n=$(pgrep -xc daiorama 2>/dev/null) || true
  echo "${n:-0}"
}

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
count_compositors() {
  local n
  n=$(scan_compositors | grep -c '^proc ')
  echo "$n"
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
count_scopes() {
  local n
  n=$(show_scopes | grep -c '^scope ')
  echo "$n"
}
hr() { echo; echo "=================================================================="; echo "## $*"; echo "=================================================================="; }

hr "[0] pre-state (fresh container)"
id
echo "binary: $(ls -la "$BIN" 2>&1)"
"$BIN" version; echo "version_rc=$?"
echo "nsroot subtree_control: $(cat /sys/fs/cgroup/cgroup.subtree_control 2>&1)"
echo "subtree listing: $(ls -d $SUBTREE 2>&1)"
echo "-- compositors:"; scan_compositors
check_eq "[0] fresh container: no compositors" "$(count_compositors)" "0"
echo "-- scopes:"; show_scopes
check_eq "[0] fresh container: no scope dirs" "$(count_scopes)" "0"

hr "[1] daemon start (cold)"
"$BIN" daemon start; START_RC=$?; echo "start_rc=$START_RC"
check_rc "[1] cold start" "$START_RC" "0"
sleep 1
MGR_PID=$("$BIN" --remote "$SOCK" status --json 2>/dev/null | sed -n 's/.*"pid": *\([0-9][0-9]*\).*/\1/p' | head -1)
if [ -z "${MGR_PID:-}" ]; then MGR_PID=$(pgrep -x daiorama 2>/dev/null | head -1); fi
echo "manager_pid=${MGR_PID:-unknown}"
echo "-- status:"; "$BIN" --remote "$SOCK" status >/tmp/a.status1 2>&1; echo "status_rc=$?"; cat /tmp/a.status1
check "[1] manager running" "state: running" /tmp/a.status1
check "[1] zero sessions on the cold start" "sessions: 0 running, 0 dead" /tmp/a.status1
echo "-- nsroot subtree_control now: $(cat /sys/fs/cgroup/cgroup.subtree_control 2>&1)"
check "[1] controllers delegated to the session subtree" "cpu memory pids" /sys/fs/cgroup/cgroup.subtree_control
echo "-- keeper procs: $(wc -l < /sys/fs/cgroup/keeper/cgroup.procs 2>&1)"
echo "-- scopes:"; show_scopes

hr "[2] three real sessions"
S1=$("$BIN" --remote "$SOCK" new); R1=$?; echo "new1_rc=$R1 id=${S1:-<none>}"
S2=$("$BIN" --remote "$SOCK" new); R2=$?; echo "new2_rc=$R2 id=${S2:-<none>}"
S3=$("$BIN" --remote "$SOCK" new); R3=$?; echo "new3_rc=$R3 id=${S3:-<none>}"
check_rc "[2] new1" "$R1" "0"
check_rc "[2] new2" "$R2" "0"
check_rc "[2] new3" "$R3" "0"
echo "-- list:"; "$BIN" --remote "$SOCK" list >/tmp/a.list2 2>&1; echo "list_rc=$?"; cat /tmp/a.list2
check_eq "[2] list shows 3 running" "$(grep -c ' running ' /tmp/a.list2 || true)" "3"
for s in "${S1:-}" "${S2:-}" "${S3:-}"; do
  [ -n "$s" ] || continue
  echo "-- status $s:"; "$BIN" --remote "$SOCK" status "$s" >/tmp/a.status.s 2>&1; echo "rc=$?"; cat /tmp/a.status.s
  check "[2] $s state running" "state: running" /tmp/a.status.s
  check "[2] $s renderer verified" "renderer: verified" /tmp/a.status.s
done
echo "-- raw Renderer lines from session instance logs:"
NV_LINES=0
for lg in /tmp/dr-*/hypr/*/hyprland.log; do
  [ -f "$lg" ] || continue
  rline=$(grep -m1 'Renderer:' "$lg" 2>/dev/null | tr -d '\r')
  echo "$lg :: $rline"
  case "$rline" in *"Renderer: NV"*) NV_LINES=$((NV_LINES + 1));; esac
done
check_ge "[2] hardware renderer string in the session logs" "$NV_LINES" "3"
echo "-- scopes:"; show_scopes
check_eq "[2] one scope per session" "$(count_scopes)" "3"
echo "-- compositors:"; scan_compositors
check_ge "[2] compositors up for 3 sessions" "$(count_compositors)" "3"

hr "[3] close one session (${S1:-<none>})"
if [ -n "${S1:-}" ]; then
  "$BIN" --remote "$SOCK" close "$S1" >/tmp/a.close1 2>&1; C1=$?; echo "close_rc=$C1 $(cat /tmp/a.close1)"
  check_rc "[3] close rc" "$C1" "0"
  check "[3] close reported" "closed session $S1" /tmp/a.close1
else
  echo "[FAIL] [3] no S1 to close"
  FAILS=$((FAILS + 1))
fi
echo "-- list after close:"; "$BIN" --remote "$SOCK" list >/tmp/a.list3 2>&1; cat /tmp/a.list3
check_eq "[3] list shows 2 running" "$(grep -c ' running ' /tmp/a.list3 || true)" "2"
echo "-- scopes after close:"; show_scopes
check_eq "[3] closed session's scope is gone" "$(count_scopes)" "2"

hr "[4] SIGKILL the manager mid-session (pid ${MGR_PID:-unknown})"
if [ -n "${MGR_PID:-}" ]; then
  kill -9 "$MGR_PID"; KR=$?; echo "kill_rc=$KR"
  check_rc "[4] SIGKILL fired" "$KR" "0"
else
  echo "[FAIL] [4] manager pid unknown"
  FAILS=$((FAILS + 1))
fi
sleep 2
echo "-- socket file after kill: $(ls -la "$SOCK" 2>&1)"
echo "-- compositors after kill:"; scan_compositors
echo "-- scopes after kill:"; show_scopes
check_ge "[4] orphaned compositor(s) survive the SIGKILL (the sweep's job)" "$(count_compositors)" "1"
check_eq "[4] orphaned scope dirs remain" "$(count_scopes)" "2"

hr "[5] daemon start again — the startup sweep must reap"
"$BIN" daemon start >/tmp/a.start5 2>&1; R5=$?; cat /tmp/a.start5; echo "restart_rc=$R5"
check_rc "[5] restart" "$R5" "0"
sleep 1
MGR_PID2=$("$BIN" --remote "$SOCK" status --json 2>/dev/null | sed -n 's/.*"pid": *\([0-9][0-9]*\).*/\1/p' | head -1)
echo "manager_pid=${MGR_PID2:-unknown}"
echo "-- manager cgroup: $(cat /proc/${MGR_PID2:-0}/cgroup 2>&1 | tr '\n' ' ')"
echo "-- nsroot subtree_control now: $(cat /sys/fs/cgroup/cgroup.subtree_control 2>&1)"
echo "-- daemon.log (tail 40):"; tail -n 40 "$DLOG" 2>&1
echo "-- startup sweep lines:"; grep -n 'startup sweep' "$DLOG" 2>&1
check "[5] sweep reaped both orphaned scopes" "startup sweep: 2 stale session scope(s) reaped" "$DLOG"
echo "-- compositors after restart:"; scan_compositors
check_eq "[5] no compositors after the sweep" "$(count_compositors)" "0"
echo "-- scopes after restart:"; show_scopes
check_eq "[5] no scope dirs after the sweep" "$(count_scopes)" "0"
echo "-- status:"; "$BIN" --remote "$SOCK" status >/tmp/a.status5 2>&1; echo "status_rc=$?"; cat /tmp/a.status5
check "[5] recovered manager: zero sessions" "sessions: 0 running, 0 dead" /tmp/a.status5
echo "-- list:"; "$BIN" --remote "$SOCK" list >/tmp/a.list5 2>&1; echo "list_rc=$?"; cat /tmp/a.list5
check "[5] list empty after the sweep" "no sessions" /tmp/a.list5

hr "[5b] a new session after the restart (recovered manager is fully functional)"
S4=$("$BIN" --remote "$SOCK" new); R4=$?; echo "new4_rc=$R4 id=${S4:-<none>}"
check_rc "[5b] new after restart" "$R4" "0"
if [ -n "${S4:-}" ]; then
  echo "-- status $S4:"; "$BIN" --remote "$SOCK" status "$S4" >/tmp/a.status.s4 2>&1; echo "rc=$?"; cat /tmp/a.status.s4
  check "[5b] recovered-session renderer verified" "renderer: verified" /tmp/a.status.s4
  echo "-- close $S4:"; "$BIN" --remote "$SOCK" close "$S4" >/tmp/a.close4 2>&1; C4=$?; echo "close4_rc=$C4 $(cat /tmp/a.close4)"
  check_rc "[5b] close after restart" "$C4" "0"
else
  echo "[FAIL] [5b] no session created after restart"
  FAILS=$((FAILS + 1))
fi
echo "-- list after post-restart close:"; "$BIN" --remote "$SOCK" list >/tmp/a.list5b 2>&1; cat /tmp/a.list5b
check "[5b] list empty after close" "no sessions" /tmp/a.list5b
echo "-- scopes after post-restart close:"; show_scopes
check_eq "[5b] scope dirs clean after close" "$(count_scopes)" "0"

hr "[6] end + post-state"
"$BIN" daemon end --force >/tmp/a.end 2>&1; ER=$?; echo "end_rc=$ER $(cat /tmp/a.end)"
check_rc "[6] daemon end" "$ER" "0"
echo "-- compositors final:"; scan_compositors
check_eq "[6] no compositors after end" "$(count_compositors)" "0"
echo "-- scopes final:"; show_scopes
check_eq "[6] no scope dirs after end" "$(count_scopes)" "0"
echo "-- session runtime dirs left: $(ls -d /tmp/dr-* 2>/dev/null | tr '\n' ' ')"

hr "[6b] doctor in the keeper-prepared container (re-entry, after the fix)"
echo "-- this shell's cgroup: $(cat /proc/self/cgroup 2>&1 | tr '\n' ' ')"
"$BIN" doctor >/tmp/a.doctor 2>&1; DR=$?; cat /tmp/a.doctor
check_rc "[6b] doctor" "$DR" "0"
check "[6b] keeper re-entry recognized (not a refusal)" "keeper-prepared container re-entered" /tmp/a.doctor

echo
echo "FAILS=$FAILS"
echo "RUN-A-DONE"
exit "$FAILS"
