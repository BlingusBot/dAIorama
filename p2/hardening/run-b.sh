#!/usr/bin/env bash
# Hardening round (b) — drain integration with LIVE sessions on the real chain, run inside
# the privileged L1 container: >=2 real sessions running; `drain` enters and reports
# {active, remaining:n}; `new` is refused with the pinned 04 string before anything spawns;
# closing sessions drops `remaining`; `drain --wait` completes at zero (and its `--timeout`
# bound fires when zero is not reached within it). Raw evidence: this script's stdout.
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
snapshot() {
  echo "compositors=$(count_compositors) scopes=$(count_scopes) daiorama=$(pgrep_count) dr_dirs=$(ls -d /tmp/dr-* 2>/dev/null | wc -l)"
}
hr() { echo; echo "=================================================================="; echo "## $*"; echo "=================================================================="; }

hr "[0] pre-state (fresh container)"
id
echo "binary: $(ls -la "$BIN" 2>&1)"
"$BIN" version; echo "version_rc=$?"
echo "nsroot subtree_control: $(cat /sys/fs/cgroup/cgroup.subtree_control 2>&1)"
echo "-- compositors:"; scan_compositors
echo "-- scopes:"; show_scopes
echo "-- snapshot: $(snapshot)"

hr "[1] daemon start (cold)"
"$BIN" daemon start; echo "start_rc=$?"
sleep 1
"$BIN" --remote "$SOCK" status >/tmp/b.status.1 2>&1; echo "status_rc=$?"
cat /tmp/b.status.1
check "cold status: drain inactive" "drain: inactive" /tmp/b.status.1
echo "-- snapshot: $(snapshot)"

hr "[2] three real sessions"
S1=$("$BIN" --remote "$SOCK" new); echo "new1_rc=$? id=${S1:-<none>}"
S2=$("$BIN" --remote "$SOCK" new); echo "new2_rc=$? id=${S2:-<none>}"
S3=$("$BIN" --remote "$SOCK" new); echo "new3_rc=$? id=${S3:-<none>}"
"$BIN" --remote "$SOCK" list >/tmp/b.list.2 2>&1; echo "list_rc=$?"; cat /tmp/b.list.2
echo "list_running_lines=$(grep -c ' running ' /tmp/b.list.2 || true)"
echo "-- status ${S1:-<none>}:"
"$BIN" --remote "$SOCK" status "${S1:-}" >/tmp/b.status.s1 2>&1; echo "rc=$?"; cat /tmp/b.status.s1
check "S1 renderer verified" "renderer: verified" /tmp/b.status.s1
echo "-- scopes:"; show_scopes
echo "-- snapshot: $(snapshot)"

hr "[3] drain with three live sessions"
"$BIN" --remote "$SOCK" drain >/tmp/b.drain.3 2>/tmp/b.drain.3.err; echo "drain_rc=$?"
echo "stdout:"; cat /tmp/b.drain.3; echo "stderr:"; cat /tmp/b.drain.3.err
check "drain reports 3 remaining" "draining; 3 still running" /tmp/b.drain.3
"$BIN" --remote "$SOCK" drain --json >/tmp/b.drain.3j 2>&1; echo "drain_json_rc=$?"; cat /tmp/b.drain.3j
check "drain --json shape" '{"active":true,"remaining":3}' /tmp/b.drain.3j
"$BIN" --remote "$SOCK" status >/tmp/b.status.3 2>&1; echo "status_rc=$?"; cat /tmp/b.status.3
check "status carries drain state" "drain: active, 3 still running" /tmp/b.status.3
"$BIN" --remote "$SOCK" status --json >/tmp/b.status.3j 2>&1; echo "status_json_rc=$?"
grep -o '"drain":{[^}]*}' /tmp/b.status.3j || echo "(no drain object)"
check "status --json drain object" '"drain":{"active":true,"remaining":3}' /tmp/b.status.3j
"$BIN" --remote "$SOCK" drain --json >/tmp/b.drain.3b 2>&1; echo "drain_again_rc=$?"; cat /tmp/b.drain.3b
check "drain enter is idempotent" '{"active":true,"remaining":3}' /tmp/b.drain.3b
"$BIN" --remote "$SOCK" list >/tmp/b.list.3 2>&1; echo "list_rc=$?"; cat /tmp/b.list.3

hr "[4] new refused while draining — pinned string, nothing spawns"
SNAP_BEFORE=$(snapshot); echo "-- snapshot before: $SNAP_BEFORE"
"$BIN" --remote "$SOCK" new >/tmp/b.new.4 2>/tmp/b.new.4.err; NEW_RC=$?
echo "new_rc=$NEW_RC"; echo "stdout: $(cat /tmp/b.new.4)"
echo "stderr:"; cat /tmp/b.new.4.err
check_rc "new refused exit 1" "$NEW_RC" 1
check "pinned refusal string (3)" "manager is draining for upgrade; new sessions are refused (3 still running). (draining)" /tmp/b.new.4.err
check "refusal hint line" "hint: retry after the upgrade completes" /tmp/b.new.4.err
SNAP_AFTER=$(snapshot); echo "-- snapshot after: $SNAP_AFTER"
check_eq "no spawn on refusal (snapshot unchanged)" "$SNAP_AFTER" "$SNAP_BEFORE"
"$BIN" --remote "$SOCK" list >/tmp/b.list.4 2>&1
check_eq "list still 3 running after refusal" "$(grep -c ' running ' /tmp/b.list.4 || true)" "3"
"$BIN" --remote "$SOCK" new --json >/tmp/b.new.4j 2>/tmp/b.new.4j.err; echo "new_json_rc=$?"
echo "json stderr: $(cat /tmp/b.new.4j.err)"
check "refusal envelope code" '"code":"draining"' /tmp/b.new.4j.err

hr "[5] closing live sessions drops remaining"
"$BIN" --remote "$SOCK" close "${S1:?}" >/tmp/b.close.s1 2>&1; echo "close1_rc=$?"; cat /tmp/b.close.s1
check "close 1 reported" "closed session ${S1:-}" /tmp/b.close.s1
"$BIN" --remote "$SOCK" drain --json >/tmp/b.drain.5a 2>&1; echo "drain_json_rc=$?"; cat /tmp/b.drain.5a
check "remaining drops to 2" '{"active":true,"remaining":2}' /tmp/b.drain.5a
"$BIN" --remote "$SOCK" status >/tmp/b.status.5 2>&1; cat /tmp/b.status.5
check "status: active, 2" "drain: active, 2 still running" /tmp/b.status.5
echo "-- scopes after close1:"; show_scopes
echo "-- snapshot: $(snapshot)"
"$BIN" --remote "$SOCK" close "${S2:?}" >/tmp/b.close.s2 2>&1; echo "close2_rc=$?"; cat /tmp/b.close.s2
"$BIN" --remote "$SOCK" drain --json >/tmp/b.drain.5b 2>&1; echo "drain_json_rc=$?"; cat /tmp/b.drain.5b
check "remaining drops to 1" '{"active":true,"remaining":1}' /tmp/b.drain.5b
echo "-- scopes after close2:"; show_scopes
echo "-- snapshot: $(snapshot)"

hr "[6a] drain --wait bound fires while remaining > 0 (timeout drill, 1 left)"
START=$(date +%s)
"$BIN" --remote "$SOCK" drain --wait --timeout 2 >/tmp/b.wait.t.out 2>/tmp/b.wait.t.err; WT_RC=$?
ELAPSED=$(( $(date +%s) - START ))
echo "wait_timeout_rc=$WT_RC elapsed=${ELAPSED}s"
echo "stdout: $(cat /tmp/b.wait.t.out)"; echo "stderr:"; cat /tmp/b.wait.t.err
check_rc "bounded --wait exits 1" "$WT_RC" 1
check "bound message names what remains" "drain did not reach zero running sessions within 2s (1 still running)" /tmp/b.wait.t.err
"$BIN" --remote "$SOCK" drain --json >/tmp/b.drain.6a 2>&1; cat /tmp/b.drain.6a
check "drain state unchanged by the bound" '{"active":true,"remaining":1}' /tmp/b.drain.6a

hr "[6b] drain --wait completes at zero (close lands while it polls)"
"$BIN" --remote "$SOCK" drain --wait --timeout 60 >/tmp/b.wait.out 2>/tmp/b.wait.err &
WAIT_PID=$!
sleep 2
if kill -0 "$WAIT_PID" 2>/dev/null; then
  echo "[OK]   --wait still polling 2s in (remaining=1)"
else
  echo "[FAIL] --wait exited before zero was reached"
  FAILS=$((FAILS + 1))
fi
"$BIN" --remote "$SOCK" close "${S3:?}" >/tmp/b.close.s3 2>&1; echo "close3_rc=$?"; cat /tmp/b.close.s3
"$BIN" --remote "$SOCK" drain --json >/tmp/b.drain.6b 2>&1; cat /tmp/b.drain.6b
check "remaining at zero after close3" '{"active":true,"remaining":0}' /tmp/b.drain.6b
for _ in $(seq 1 40); do
  kill -0 "$WAIT_PID" 2>/dev/null || break
  sleep 0.25
done
wait "$WAIT_PID"; W_RC=$?
echo "wait_rc=$W_RC"
echo "stdout: $(cat /tmp/b.wait.out)"; echo "stderr: $(cat /tmp/b.wait.err)"
check_rc "--wait completes rc 0" "$W_RC" 0
check "--wait completion line" "drain complete" /tmp/b.wait.out

hr "[7] drain is sticky at zero until the manager stops"
"$BIN" --remote "$SOCK" status >/tmp/b.status.7 2>&1; cat /tmp/b.status.7
check "status: active, 0" "drain: active, 0 still running" /tmp/b.status.7
SNAP7_BEFORE=$(snapshot); echo "-- snapshot before refused new: $SNAP7_BEFORE"
"$BIN" --remote "$SOCK" new >/tmp/b.new.7 2>/tmp/b.new.7.err; N7_RC=$?
echo "new_rc=$N7_RC"; echo "stderr:"; cat /tmp/b.new.7.err
check_rc "new still refused at zero" "$N7_RC" 1
check "pinned refusal string (0)" "manager is draining for upgrade; new sessions are refused (0 still running). (draining)" /tmp/b.new.7.err
SNAP7_AFTER=$(snapshot); echo "-- snapshot after refused new: $SNAP7_AFTER"
check_eq "no spawn at zero (snapshot unchanged)" "$SNAP7_AFTER" "$SNAP7_BEFORE"
"$BIN" --remote "$SOCK" list >/tmp/b.list.7 2>&1; echo "list rc=$?"; cat /tmp/b.list.7

hr "[8] end + post-state (leak check)"
"$BIN" daemon end --force >/tmp/b.end 2>&1; echo "end_rc=$?"; cat /tmp/b.end
sleep 1
echo "-- compositors final:"; scan_compositors
echo "-- scopes final:"; show_scopes
echo "-- snapshot final: $(snapshot)"
echo "-- session runtime dirs left: $(ls -d /tmp/dr-* 2>/dev/null | tr '\n' ' ')"
echo "-- daemon.log tail (20):"; tail -n 20 "$DLOG" 2>&1

echo
echo "FAILS=$FAILS"
echo "RUN-B-DONE"
