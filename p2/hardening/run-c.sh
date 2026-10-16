#!/usr/bin/env bash
# Hardening round (c) — odd creation params on the real chain, run inside the privileged L1
# container. Each case creates a real session through the manager CLI with unusual
# resolution/scale (scale 1.5; tiny 64x64 and 1x1; huge 7680x4320 and 16384x16384; odd
# 641x481 @1.5 and 642x482 @1.25 exercising the rounding path; 1920x1080 @0.25 for a sub-1
# scale) and asserts the clean-outcome invariants either way: on success the compositor
# carries the computed physical mode (logical × scale, rounded, floor 1) AND the requested
# scale verbatim (the geometry readback — before the fix, the 641x481 @1.5 readback surfaced
# scale 2: the compositor's scale substitution), and the close leaves no trace (no process,
# no scope dir, no runtime dir); on refusal the box is byte-identical.
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
check_absent() { # check_absent <label> <path>
  if [ ! -e "$2" ]; then
    echo "[OK]   $1"
  else
    echo "[FAIL] $1 :: $2 still exists"
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
settle() { # wait for a just-closed session's processes/dirs to be gone
  local i
  for i in $(seq 1 20); do
    [ "$(count_compositors)" = 0 ] && [ "$(count_scopes)" = 0 ] && return 0
    sleep 0.5
  done
  return 1
}
id8() { printf '%s' "${1:0:8}"; }
hr() { echo; echo "=================================================================="; echo "## $*"; echo "=================================================================="; }

case_run() {
  # case_run <label> <resolution> <scale> <expect: win|any> <physical mode WxH>
  local label="$1" res="$2" scale="$3" expect="$4" mode="$5"
  hr "case $label :: new --resolution $res --scale $scale (expect=$expect, physical $mode)"
  local before after rc id rt sig close_rc
  before=$(snapshot); echo "-- snapshot before: $before"
  timeout 120 "$BIN" --remote "$SOCK" new --resolution "$res" --scale "$scale" >/tmp/c3.out 2>/tmp/c3.err
  rc=$?
  id=$(head -n 1 /tmp/c3.out | tr -d '\r\n')
  echo "new_rc=$rc id=${id:-<none>}"
  if [ -s /tmp/c3.err ]; then echo "stderr:"; cat /tmp/c3.err; fi

  if [ "$rc" = 0 ] && [ -n "$id" ]; then
    if [ "$expect" = "refuse" ]; then
      echo "[FAIL] $label: expected a refusal, got id=$id"
      FAILS=$((FAILS + 1))
    else
      echo "[OK]   $label: created id=$id"
    fi
    "$BIN" --remote "$SOCK" status "$id" >/tmp/c3.status 2>&1; echo "status_rc=$?"
    cat /tmp/c3.status
    check "$label status resolution" "resolution: $res" /tmp/c3.status
    check "$label status scale" "scale: $scale" /tmp/c3.status
    check "$label renderer verified" "renderer: verified" /tmp/c3.status
    "$BIN" --remote "$SOCK" list >/tmp/c3.list 2>&1
    check_eq "$label list shows 1 running" "$(grep -c ' running ' /tmp/c3.list || true)" "1"

    rt="/tmp/dr-$(id8 "$id")"
    if [ -d "$rt/hypr" ]; then
      sig=$(ls "$rt/hypr" 2>/dev/null | head -1)
      echo "rt_dir=$rt signature=${sig:-<none>}"
      XDG_RUNTIME_DIR="$rt" HYPRLAND_INSTANCE_SIGNATURE="$sig" hyprctl monitors -j >/tmp/c3.mon.json 2>/tmp/c3.mon.err
      echo "hyprctl_monitors_rc=$? stderr: $(cat /tmp/c3.mon.err 2>/dev/null)"
      python3 - <<'PYEOF' >/tmp/c3.mon.txt 2>&1
import json
out = json.load(open('/tmp/c3.mon.json'))
for m in out:
    print('monitor: %s w=%s h=%s scale=%s refresh=%s' % (
        m.get('name'), m.get('width'), m.get('height'), m.get('scale'), m.get('refreshRate')))
for m in out:
    if m.get('name') == 'HEADLESS-1':
        print('monitor-core: w=%d h=%d scale=%g' % (
            m.get('width'), m.get('height'), float(m.get('scale'))))
PYEOF
      cat /tmp/c3.mon.txt
      if [ "$mode" != "-" ]; then
        check "$label compositor carries the physical mode" "monitor-core: w=${mode%x*} h=${mode#*x} scale=$scale" /tmp/c3.mon.txt
      fi
    else
      echo "[FAIL] $label: runtime dir $rt/hypr missing on a successful create"
      FAILS=$((FAILS + 1))
    fi

    "$BIN" --remote "$SOCK" close "$id" >/tmp/c3.close 2>&1; close_rc=$?
    echo "close_rc=$close_rc $(cat /tmp/c3.close)"
    check_rc "$label close rc" "$close_rc" "0"
    check "$label close reported" "closed session $id" /tmp/c3.close
    settle || true
    check_eq "$label compositors after close" "$(count_compositors)" "0"
    check_eq "$label scope dirs after close" "$(count_scopes)" "0"
    check_absent "$label runtime dir after close" "$rt"
  else
    if [ "$expect" = "win" ]; then
      echo "[FAIL] $label: expected success, rc=$rc"
      FAILS=$((FAILS + 1))
    else
      echo "[OK]   $label: refused cleanly (rc=$rc)"
    fi
    if [ "$rc" != 0 ] && [ ! -s /tmp/c3.err ]; then
      echo "[FAIL] $label: refusal without a stderr message"
      FAILS=$((FAILS + 1))
    fi
    if [ "$rc" = 124 ]; then
      echo "[FAIL] $label: 'new' exceeded the 120s wall (hang)"
      FAILS=$((FAILS + 1))
    fi
    echo "-- daemon.log tail (6):"
    tail -n 6 "$DLOG" 2>&1
  fi

  after=$(snapshot); echo "-- snapshot after: $after"
  check_eq "$label no residue (snapshot unchanged)" "$after" "$before"
  "$BIN" --remote "$SOCK" list >/tmp/c3.list2 2>&1; echo "list_rc=$?"
  cat /tmp/c3.list2
}

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
"$BIN" --remote "$SOCK" status >/tmp/c3.status.1 2>&1; echo "status_rc=$?"
cat /tmp/c3.status.1
check "cold status: drain inactive" "drain: inactive" /tmp/c3.status.1
echo "-- snapshot: $(snapshot)"

case_run "s1.5-1280x800" "1280x800" "1.5" "win" "1920x1200"
case_run "tiny-64x64" "64x64" "1" "any" "64x64"
case_run "tiny-1x1" "1x1" "1" "any" "1x1"
case_run "round-641x481-s1.5" "641x481" "1.5" "win" "962x722"
case_run "round-642x482-s1.25" "642x482" "1.25" "win" "803x603"
case_run "scale-0.25-1920x1080" "1920x1080" "0.25" "any" "480x270"
case_run "huge-7680x4320" "7680x4320" "1" "any" "7680x4320"
case_run "extreme-16384x16384" "16384x16384" "1" "any" "16384x16384"

hr "[Z] end + post-state (leak check)"
"$BIN" daemon end --force >/tmp/c3.end 2>&1; echo "end_rc=$? $(cat /tmp/c3.end)"
sleep 1
echo "-- compositors final:"; scan_compositors
echo "-- scopes final:"; show_scopes
echo "-- snapshot final: $(snapshot)"
echo "-- session runtime dirs left: $(ls -d /tmp/dr-* 2>/dev/null | tr '\n' ' ')"
echo "-- daemon.log tail (30):"
tail -n 30 "$DLOG" 2>&1

echo
echo "FAILS=$FAILS"
echo "RUN-C-DONE"
