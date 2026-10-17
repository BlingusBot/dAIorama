#!/usr/bin/env bash
# Hardening round (d) — concurrency: `drain` entered while a create is in flight, run inside
# the privileged L1 container against the real chain.
#
# Contract under test (04/10 plus the manager's own "no spawn may start" while draining):
# whatever `drain` answers must account for every session that exists when it answers. A
# create that was already spawning when drain was entered is either refused before it starts
# or waited for and counted — never a session that appears after drain reported a smaller
# `remaining` (which is the number `drain --wait` reads to decide "zero").
#
# Round-6 note: this build's dispatch path serves one request at a time on the test box, so
# the live run cannot *force* the interleaving; it pins the outward contract and the unit
# tests (`drain_waits_for_an_in_flight_create_and_counts_it`,
# `a_create_that_queued_past_drain_is_refused_before_it_spawns`) pin the invariant itself.
#
# Self-checking: per-assertion [OK]/[FAIL] + a final FAILS=; exit code = FAILS.
set -u

BIN="${BIN:-/out/hardening/daiorama}"
export XDG_CONFIG_HOME=/tmp/hc XDG_DATA_HOME=/tmp/hd XDG_RUNTIME_DIR=/tmp/hr
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
SOCK="$XDG_RUNTIME_DIR/daiorama/default/manager.sock"
DLOG="$XDG_DATA_HOME/daiorama/default/daemon.log"

FAILS=0
check() {
  local label="$1" needle="$2" file="$3"
  if grep -qF -- "$needle" "$file" 2>/dev/null; then
    echo "[OK]   $label"
  else
    echo "[FAIL] $label :: expected '$needle' in $file"
    FAILS=$((FAILS + 1))
  fi
}
check_absent() {
  local label="$1" needle="$2" file="$3"
  if grep -qF -- "$needle" "$file" 2>/dev/null; then
    echo "[FAIL] $label :: '$needle' must not appear in $file"
    FAILS=$((FAILS + 1))
  else
    echo "[OK]   $label"
  fi
}
check_rc() {
  local label="$1" actual="$2" expected="$3"
  if [ "$actual" = "$expected" ]; then
    echo "[OK]   $label (rc=$actual)"
  else
    echo "[FAIL] $label rc=$actual want $expected"
    FAILS=$((FAILS + 1))
  fi
}
check_eq() {
  local label="$1" actual="$2" expected="$3"
  if [ "$actual" = "$expected" ]; then
    echo "[OK]   $label ($actual)"
  else
    echo "[FAIL] $label: got '$actual' want '$expected'"
    FAILS=$((FAILS + 1))
  fi
}
json_field() {
  python3 - "$1" "$2" <<'PY' 2>/dev/null
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    print("parse-error"); raise SystemExit
try:
    print(eval(sys.argv[2], {"d": d}))
except Exception:
    print("missing")
PY
}
ms() { date +%s%3N; }
running_count() { json_field "$1" "sum(1 for s in d['sessions'] if s.get('state')=='running')"; }
hr() { echo; echo "=================================================================="; echo "## $*"; echo "=================================================================="; }

hr "[0] pre-state (fresh container)"
id
echo "binary: $(ls -la "$BIN" 2>&1)"
"$BIN" version; echo "version_rc=$?"

hr "[1] daemon start (cold)"
"$BIN" daemon start; echo "start_rc=$?"
sleep 1
"$BIN" --remote "$SOCK" status >/tmp/d.status.1 2>&1; echo "status_rc=$?"
cat /tmp/d.status.1
check "cold status: drain inactive" "drain: inactive" /tmp/d.status.1

hr "[2] race A — drain entered while a create is in flight (3 iterations)"
for i in 1 2 3; do
  echo
  echo "---- iteration $i ----"
  T0=$(ms)
  (
    "$BIN" --remote "$SOCK" new >"/tmp/d.create.a$i" 2>"/tmp/d.create.a$i.err"
    echo "create_rc=$?" >>"/tmp/d.create.a$i"
    echo "create_done_ms=$(( $(ms) - T0 ))" >>"/tmp/d.create.a$i"
  ) &
  CPID=$!
  sleep 0.35
  T_DRAIN=$(ms)
  "$BIN" --remote "$SOCK" drain --json >"/tmp/d.drain.a$i" 2>"/tmp/d.drain.a$i.err"
  DRAIN_RC=$?
  T_DRAIN_RET=$(ms)
  echo "drain rc=$DRAIN_RC answered at +$(( T_DRAIN_RET - T0 ))ms (sent +$(( T_DRAIN - T0 ))ms): $(cat "/tmp/d.drain.a$i")"
  [ -s "/tmp/d.drain.a$i.err" ] && echo "drain stderr: $(cat "/tmp/d.drain.a$i.err")"
  "$BIN" --remote "$SOCK" list --json >"/tmp/d.list.during$i" 2>&1
  echo "sessions running while the drain command was in flight/returned: $(running_count "/tmp/d.list.during$i")"
  wait "$CPID"
  cat "/tmp/d.create.a$i"
  CREATE_DONE=$(grep -o 'create_done_ms=[0-9]*' "/tmp/d.create.a$i" | cut -d= -f2)
  echo "create finished at +${CREATE_DONE}ms"
  "$BIN" --remote "$SOCK" list --json >"/tmp/d.list.a$i" 2>&1
  RUNNING=$(running_count "/tmp/d.list.a$i")
  REMAINING=$(json_field "/tmp/d.drain.a$i" "d['remaining']")
  ACTIVE=$(json_field "/tmp/d.drain.a$i" "str(d['active']).lower()")
  echo "drain said: active=$ACTIVE remaining=$REMAINING; sessions running after the create settled: $RUNNING"
  echo "list: $(cat "/tmp/d.list.a$i")"
  check_eq "A$i: drain entered" "$ACTIVE" "true"
  check_eq "A$i: drain accounts for every session that exists when it answers" "$RUNNING" "$REMAINING"
  if [ "$T_DRAIN_RET" -lt "$CREATE_DONE" ]; then
    echo "note: drain answered at +$(( T_DRAIN_RET - T0 ))ms, before the create finished at +${CREATE_DONE}ms — the interleaving was exercised"
  else
    echo "note: drain answered after the create finished (+$(( T_DRAIN_RET - T0 ))ms vs +${CREATE_DONE}ms) — serialized on this box; the unit tests pin the interleaving"
  fi
  for id in $(python3 -c "
import json
d = json.load(open('/tmp/d.list.a$i'))
print(' '.join(s['id'] for s in d['sessions'] if s.get('state') == 'running'))
" 2>/dev/null); do
    "$BIN" --remote "$SOCK" close "$id" >/dev/null 2>&1; echo "closed $id"
  done
  "$BIN" daemon end --force >/tmp/d.end.a$i 2>&1; echo "end_rc=$? $(cat /tmp/d.end.a$i)"
  sleep 0.5
  "$BIN" daemon start >>/tmp/d.end.a$i 2>&1; echo "restart_rc=$?"
  sleep 0.5
done

hr "[3] race B — drain --wait entered while a create is in flight: never completes at zero"
T0=$(ms)
(
  "$BIN" --remote "$SOCK" new >/tmp/d.create.b 2>/tmp/d.create.b.err
  echo "create_rc=$?" >>/tmp/d.create.b
  echo "create_done_ms=$(( $(ms) - T0 ))" >>/tmp/d.create.b
) &
CPID=$!
sleep 0.35
"$BIN" --remote "$SOCK" drain --wait --timeout 3 >/tmp/d.wait.b 2>/tmp/d.wait.b.err
WAIT_RC=$?
echo "drain --wait rc=$WAIT_RC at +$(( $(ms) - T0 ))ms"
echo "stdout: $(cat /tmp/d.wait.b)"
echo "stderr: $(cat /tmp/d.wait.b.err)"
wait "$CPID"
cat /tmp/d.create.b
"$BIN" --remote "$SOCK" list --json >/tmp/d.list.b 2>&1
RUNNING_B=$(running_count /tmp/d.list.b)
echo "sessions running after --wait gave up: $RUNNING_B"
echo "list: $(cat /tmp/d.list.b)"
check_absent "B: --wait never reported a false 'drain complete'" "drain complete" /tmp/d.wait.b
check_eq "B: the bound fires because a live session exists (not zero)" "$WAIT_RC" "1"
check "B: the bound names the live session it sees" "(1 still running)" /tmp/d.wait.b.err
check_eq "B: exactly the one session is running — nothing else landed" "$RUNNING_B" "1"

hr "[4] drain refuses new with the pinned string and spawns nothing"
SNAP_BEFORE="$(ls -d /tmp/dr-* 2>/dev/null | wc -l) scope-dirs=$(ls -d /sys/fs/cgroup/daiorama-sessions/*/ 2>/dev/null | wc -l)"
echo "-- before: $SNAP_BEFORE"
"$BIN" --remote "$SOCK" new >/tmp/d.new.4 2>/tmp/d.new.4.err; NEW_RC=$?
echo "new_rc=$NEW_RC stderr: $(cat /tmp/d.new.4.err)"
SNAP_AFTER="$(ls -d /tmp/dr-* 2>/dev/null | wc -l) scope-dirs=$(ls -d /sys/fs/cgroup/daiorama-sessions/*/ 2>/dev/null | wc -l)"
echo "-- after:  $SNAP_AFTER"
check_rc "new refused while draining" "$NEW_RC" "1"
check "pinned refusal string" "manager is draining for upgrade; new sessions are refused (1 still running). (draining)" /tmp/d.new.4.err
check_eq "no spawn on refusal" "$SNAP_AFTER" "$SNAP_BEFORE"

hr "[5] end + post-state (leak check)"
"$BIN" daemon end --force >/tmp/d.end 2>&1; echo "end_rc=$?"; cat /tmp/d.end
sleep 1
echo "-- mutter/Hyprland processes:"
found=0
for p in /proc/[0-9]*; do
  comm=$(cat "$p/comm" 2>/dev/null) || continue
  case "$comm" in
    mutter|Hyprland) found=1; echo "proc ${p#/proc/}: $comm";;
  esac
done
[ "$found" = 0 ] && echo "(none)"
echo "-- session scopes:"
if [ -d /sys/fs/cgroup/daiorama-sessions ]; then
  any=0
  for d in /sys/fs/cgroup/daiorama-sessions/*/; do
    [ -d "$d" ] || continue
    any=1; echo "scope $(basename "$d"): procs=[$(tr '\n' ' ' <"$d/cgroup.procs" 2>/dev/null)]"
  done
  [ "$any" = 0 ] && echo "(subtree present, no scope dirs)"
else
  echo "(no session subtree)"
fi
echo "-- daemon.log tail (12):"; tail -n 12 "$DLOG" 2>&1

echo
echo "FAILS=$FAILS"
exit "$FAILS"
