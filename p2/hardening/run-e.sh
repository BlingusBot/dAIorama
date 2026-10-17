#!/usr/bin/env bash
# Hardening round (e) — soak/leak, run inside the privileged L1 container against the real
# chain: repeated create/status/close cycles plus an RPC hammer, measuring the manager's
# fd/thread/RSS footprint and the process/scope/runtime-dir residue after every step.
#
# What this pins that the earlier rounds did not: (a)-(c) prove each single transition is
# clean; this asks whether the per-session and per-connection resources RETURN to baseline
# when the same path is walked many times. A leak here is invisible to a one-shot round:
#  - fd/thread growth on the manager across closed sessions (socket/bookkeeping leak),
#  - fd growth across N sequential `list` calls (per-connection leak),
#  - scope dirs, compositor processes or /tmp/dr-* runtime dirs left after a close,
#  - residue after a 3-at-once burst closed out of order.
# RSS is measured and reported; the assertion on it is a loose ceiling (allocator caching is
# not a leak), with the per-cycle series left in the raw log for a human trend read.
#
# Self-checking: per-assertion [OK]/[FAIL] + a final FAILS=; exit code = FAILS.
set -u

BIN="${BIN:-/out/hardening/daiorama}"
export XDG_CONFIG_HOME=/tmp/hc XDG_DATA_HOME=/tmp/hd XDG_RUNTIME_DIR=/tmp/hr
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
SOCK="$XDG_RUNTIME_DIR/daiorama/default/manager.sock"
DLOG="$XDG_DATA_HOME/daiorama/default/daemon.log"
SUBTREE=/sys/fs/cgroup/daiorama-sessions
CYCLES="${CYCLES:-24}"
HAMMER="${HAMMER:-400}"
FD_SLACK="${FD_SLACK:-4}"
RSS_CEIL_MB="${RSS_CEIL_MB:-96}"

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
check_le() { # check_le <label> <actual> <max>
  local label="$1" actual="$2" max="$3"
  if [ "${actual:-999999}" -le "$max" ] 2>/dev/null; then
    echo "[OK]   $label ($actual <= $max)"
  else
    echo "[FAIL] $label: got '$actual' want <= $max"
    FAILS=$((FAILS + 1))
  fi
}

count_compositors() {
  local n=0
  for p in /proc/[0-9]*; do
    local comm
    comm=$(cat "$p/comm" 2>/dev/null) || continue
    case "$comm" in mutter|Hyprland) n=$((n + 1));; esac
  done
  echo "$n"
}
count_scopes() {
  local n=0
  [ -d "$SUBTREE" ] || { echo 0; return; }
  for d in "$SUBTREE"/*/; do [ -d "$d" ] && n=$((n + 1)); done
  echo "$n"
}
count_drs() {
  local n=0
  for d in /tmp/dr-*; do [ -e "$d" ] && n=$((n + 1)); done
  echo "$n"
}
count_daiorama() {
  local n
  n=$(pgrep -xc daiorama 2>/dev/null) || true
  echo "${n:-0}"
}
mgr_pid() {
  local p
  p=$("$BIN" --remote "$SOCK" status --json 2>/dev/null \
      | sed -n 's/.*"pid": *\([0-9][0-9]*\).*/\1/p' | head -1)
  [ -n "${p:-}" ] || p=$(pgrep -x daiorama 2>/dev/null | head -1)
  echo "${p:-}"
}
pid_metric() { # pid_metric <pid> <field>  field = fd|threads|rss
  local pid="$1" field="$2"
  case "$field" in
    fd) ls "/proc/$pid/fd" 2>/dev/null | wc -l ;;
    threads) awk '/^Threads:/{print $2}' "/proc/$pid/status" 2>/dev/null ;;
    rss) awk '/^VmRSS:/{print $2}' "/proc/$pid/status" 2>/dev/null ;;
  esac
}
snap() { # snap <label>
  local pid
  pid=$(mgr_pid)
  echo "SNAP $1 pid=${pid:-none} fds=$(pid_metric "$pid" fd) threads=$(pid_metric "$pid" threads) rss_kb=$(pid_metric "$pid" rss) comps=$(count_compositors) scopes=$(count_scopes) drs=$(count_drs) daiorama=$(count_daiorama)"
}
wait_verified() { # wait_verified <id> <file> <tries>
  local id="$1" file="$2" tries="${3:-80}" i=0
  while [ "$i" -lt "$tries" ]; do
    "$BIN" --remote "$SOCK" status "$id" >"$file" 2>&1
    grep -q "renderer: verified" "$file" && { echo "verified_after=${i}"; return 0; }
    sleep 0.25
    i=$((i + 1))
  done
  echo "verified_after=timeout"
  return 1
}
wait_clean() { # wait_clean <max_tries> — residue back to zero (scopes, comps, drs)
  local tries="${1:-80}" i=0
  while [ "$i" -lt "$tries" ]; do
    if [ "$(count_scopes)" = 0 ] && [ "$(count_compositors)" = 0 ] && [ "$(count_drs)" = 0 ]; then
      echo "clean_after=${i}"
      return 0
    fi
    sleep 0.25
    i=$((i + 1))
  done
  echo "clean_after=timeout"
  return 1
}
hr() { echo; echo "==================================================================="; echo "## $*"; echo "==================================================================="; }

hr "[0] pre-state (fresh container)"
id
echo "binary: $(ls -la "$BIN" 2>&1)"
"$BIN" version; echo "version_rc=$?"
echo "nsroot subtree_control: $(cat /sys/fs/cgroup/cgroup.subtree_control 2>&1)"
check_eq "[0] fresh container: no compositors" "$(count_compositors)" "0"
check_eq "[0] fresh container: no scope dirs" "$(count_scopes)" "0"
check_eq "[0] fresh container: no session runtime dirs" "$(count_drs)" "0"

hr "[1] daemon start (cold) — baseline footprint"
"$BIN" daemon start; START_RC=$?; echo "start_rc=$START_RC"
check_rc "[1] cold start" "$START_RC" "0"
sleep 1
MGR=$(mgr_pid)
echo "manager_pid=${MGR:-unknown}"
[ -n "${MGR:-}" ] && echo "manager_cgroup: $(tr '\n' ' ' </proc/$MGR/cgroup 2>&1)" || true
BASE_FD=$(pid_metric "${MGR:-0}" fd); BASE_TH=$(pid_metric "${MGR:-0}" threads); BASE_RSS=$(pid_metric "${MGR:-0}" rss)
echo "baseline: fds=$BASE_FD threads=$BASE_TH rss_kb=$BASE_RSS"
check_le "[1] baseline fd count is sane" "${BASE_FD:-999}" "64"
snap baseline

hr "[2] sequential soak — $CYCLES create/status/close cycles"
CYCLE_IDX=0
RES=(1280x800 1920x1080 640x480 1280x800 800x600 1920x1080 640x480 1280x800)
SCA=(1 1 1 1.5 2 1 1.5 1)
while [ "$CYCLE_IDX" -lt "$CYCLES" ]; do
  RES_V="${RES[$((CYCLE_IDX % ${#RES[@]}))]}"
  SC_V="${SCA[$((CYCLE_IDX % ${#SCA[@]}))]}"
  N=$((CYCLE_IDX + 1))
  hr "[2.$N] new --resolution $RES_V --scale $SC_V"
  SID=$("$BIN" --remote "$SOCK" new --resolution "$RES_V" --scale "$SC_V" 2>/tmp/e.new.err)
  NRC=$?
  echo "new_rc=$NRC id=${SID:-<none>} stderr=$(cat /tmp/e.new.err)"
  check_rc "[2.$N] new" "$NRC" "0"
  if [ -n "${SID:-}" ]; then
    echo "-- status poll: $(wait_verified "$SID" /tmp/e.status)"
    check "[2.$N] renderer verified" "renderer: verified" /tmp/e.status
    check "[2.$N] session reports running" "state: running" /tmp/e.status
    if [ "$N" = "1" ]; then
      echo "-- raw renderer lines:"
      for lg in /tmp/dr-*/hypr/*/hyprland.log; do
        [ -f "$lg" ] || continue
        echo "$lg :: $(grep -m1 'Renderer:' "$lg" 2>/dev/null | tr -d '\r')"
      done > /tmp/e.renderer
      cat /tmp/e.renderer
      check "[2.$N] hardware renderer string in the session log" "Renderer: NV" /tmp/e.renderer
    fi
    LIVE_FD=$(pid_metric "$MGR" fd); LIVE_RSS=$(pid_metric "$MGR" rss)
    echo "live: fds=$LIVE_FD rss_kb=$LIVE_RSS comps=$(count_compositors) scopes=$(count_scopes) drs=$(count_drs)"
    check_eq "[2.$N] one scope for the live session" "$(count_scopes)" "1"
    "$BIN" --remote "$SOCK" close "$SID" >/tmp/e.close 2>&1; CRC=$?
    echo "close_rc=$CRC $(cat /tmp/e.close)"
    check_rc "[2.$N] close" "$CRC" "0"
    echo "-- residue poll: $(wait_clean 80)"
    check_eq "[2.$N] zero scope dirs after close" "$(count_scopes)" "0"
    check_eq "[2.$N] zero compositors after close" "$(count_compositors)" "0"
    check_eq "[2.$N] zero session runtime dirs after close" "$(count_drs)" "0"
    "$BIN" --remote "$SOCK" list >/tmp/e.list 2>&1
    check "[2.$N] list empty after close" "no sessions" /tmp/e.list
    snap "cycle$N"
    AFTER_FD=$(pid_metric "$MGR" fd); AFTER_TH=$(pid_metric "$MGR" threads); AFTER_RSS=$(pid_metric "$MGR" rss)
    check_le "[2.$N] manager fds back at baseline+$FD_SLACK" "${AFTER_FD:-99999}" "$((BASE_FD + FD_SLACK))"
    check_le "[2.$N] manager threads back at baseline+$FD_SLACK" "${AFTER_TH:-99999}" "$((BASE_TH + FD_SLACK))"
    echo "[OBS]  [2.$N] rss_kb=$AFTER_RSS (baseline $BASE_RSS)"
  fi
  CYCLE_IDX=$((CYCLE_IDX + 1))
done

hr "[3] RPC hammer — $HAMMER sequential list calls on one manager"
FD0=$(pid_metric "$MGR" fd); RSS0=$(pid_metric "$MGR" rss)
echo "before: fds=$FD0 rss_kb=$RSS0"
HFAIL=0
i=0
while [ "$i" -lt "$HAMMER" ]; do
  "$BIN" --remote "$SOCK" list >/dev/null 2>&1 || HFAIL=$((HFAIL + 1))
  i=$((i + 1))
done
echo "hammer_calls=$HAMMER failures=$HFAIL"
check_eq "[3] every list call answered" "$HFAIL" "0"
sleep 1
FD1=$(pid_metric "$MGR" fd); RSS1=$(pid_metric "$MGR" rss)
echo "after: fds=$FD1 rss_kb=$RSS1"
check_le "[3] manager fds back at baseline+$FD_SLACK after $HAMMER calls" "${FD1:-99999}" "$((BASE_FD + FD_SLACK))"
snap "post-hammer"

hr "[4] burst soak — 3 sessions at once, closed out of order, x${BURSTS:-5}"
B=0
while [ "$B" -lt "${BURSTS:-5}" ]; do
  B=$((B + 1))
  hr "[4.$B] three concurrent sessions"
  ID1=$("$BIN" --remote "$SOCK" new --resolution 1280x800 --scale 1); R1=$?
  ID2=$("$BIN" --remote "$SOCK" new --resolution 640x480 --scale 1.5); R2=$?
  ID3=$("$BIN" --remote "$SOCK" new --resolution 800x600 --scale 2); R3=$?
  echo "ids: ${ID1:-<none>} ${ID2:-<none>} ${ID3:-<none>} rcs=$R1/$R2/$R3"
  check_eq "[4.$B] three creates" "$R1$R2$R3" "000"
  echo "-- status poll 1: $(wait_verified "${ID1:-}" /tmp/e.s1)"; check "[4.$B] s1 verified" "renderer: verified" /tmp/e.s1
  echo "-- status poll 2: $(wait_verified "${ID2:-}" /tmp/e.s2)"; check "[4.$B] s2 verified" "renderer: verified" /tmp/e.s2
  echo "-- status poll 3: $(wait_verified "${ID3:-}" /tmp/e.s3)"; check "[4.$B] s3 verified" "renderer: verified" /tmp/e.s3
  echo "live: fds=$(pid_metric "$MGR" fd) rss_kb=$(pid_metric "$MGR" rss) comps=$(count_compositors) scopes=$(count_scopes) drs=$(count_drs)"
  check_eq "[4.$B] three scopes for three live sessions" "$(count_scopes)" "3"
  "$BIN" --remote "$SOCK" close "${ID2:-}" >/dev/null 2>&1
  "$BIN" --remote "$SOCK" close "${ID3:-}" >/dev/null 2>&1
  "$BIN" --remote "$SOCK" close "${ID1:-}" >/dev/null 2>&1
  echo "-- residue poll: $(wait_clean 120)"
  check_eq "[4.$B] zero scope dirs after all closes" "$(count_scopes)" "0"
  check_eq "[4.$B] zero compositors after all closes" "$(count_compositors)" "0"
  check_eq "[4.$B] zero session runtime dirs after all closes" "$(count_drs)" "0"
  "$BIN" --remote "$SOCK" list >/tmp/e.list 2>&1
  check "[4.$B] list empty after the burst" "no sessions" /tmp/e.list
  snap "burst$B"
  AFTER_FD=$(pid_metric "$MGR" fd)
  check_le "[4.$B] manager fds back at baseline+$FD_SLACK after the burst" "${AFTER_FD:-99999}" "$((BASE_FD + FD_SLACK))"
done

hr "[5] RSS ceiling (loose) and end-state"
RSS_END=$(pid_metric "$MGR" rss)
GROWTH_MB=$(( (${RSS_END:-0} - ${BASE_RSS:-0}) / 1024 ))
echo "rss_kb baseline=$BASE_RSS end=$RSS_END growth_mb=$GROWTH_MB"
check_le "[5] manager RSS growth <= ${RSS_CEIL_MB} MB" "$GROWTH_MB" "$RSS_CEIL_MB"
"$BIN" daemon end --force >/tmp/e.end 2>&1; ERC=$?
echo "end_rc=$ERC $(cat /tmp/e.end)"
check_rc "[5] daemon end" "$ERC" "0"
sleep 1
echo "-- compositors: $(count_compositors)"; echo "-- scopes: $(count_scopes)"; echo "-- dr dirs: $(count_drs)"; echo "-- daiorama procs: $(count_daiorama)"
check_eq "[5] no compositors after end" "$(count_compositors)" "0"
check_eq "[5] no scope dirs after end" "$(count_scopes)" "0"
check_eq "[5] no session runtime dirs after end" "$(count_drs)" "0"
check_eq "[5] no daiorama processes after end" "$(count_daiorama)" "0"

echo
echo "FAILS=$FAILS"
echo "RUN-E-DONE"
exit "$FAILS"
