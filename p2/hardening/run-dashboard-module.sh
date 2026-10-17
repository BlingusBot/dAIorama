#!/bin/bash
# D1 live leg: one real `daiorama serve` process and a real unix socket, with the dashboard
# module activation link (`<data>/modules/daiorama-dashboard/active`) changed between calls.
# No compositor and no session are needed: `status`/`readiness` are answered by the manager
# before any session lookup.
#
# Usage: run-dashboard-module.sh <binary> <label>
set -u
BIN=$(readlink -f "$1")
LABEL=${2:-run}
ROOT=/tmp/dr-module-$LABEL
rm -rf "$ROOT"
mkdir -p "$ROOT"/config/daiorama/default "$ROOT"/data "$ROOT"/run
printf '(version: 1, limits: (mode: off))\n' > "$ROOT"/config/daiorama/default/config.ron

export XDG_CONFIG_HOME="$ROOT/config"
export XDG_DATA_HOME="$ROOT/data"
export XDG_RUNTIME_DIR="$ROOT/run"
export TMPDIR="$ROOT"

"$BIN" serve > "$ROOT/serve.log" 2>&1 &
SERVE=$!
trap 'kill $SERVE 2>/dev/null; wait $SERVE 2>/dev/null' EXIT

for _ in $(seq 1 100); do
    [ -S "$ROOT/run/daiorama/default/manager.sock" ] && break
    sleep 0.1
done
if [ ! -S "$ROOT/run/daiorama/default/manager.sock" ]; then
    echo "manager socket never appeared; serve.log:"
    tail -20 "$ROOT/serve.log"
    exit 90
fi

python3 "$(dirname "$0")/probe-dashboard-module.py" \
    "$ROOT/run/daiorama/default/manager.sock" \
    "$ROOT/data/daiorama/default"
RC=$?
echo "dashboard_module_rc=$RC"
if [ "$RC" -eq 0 ]; then
    echo "VERDICT=PASS"
else
    echo "VERDICT=FAIL"
fi
exit "$RC"
