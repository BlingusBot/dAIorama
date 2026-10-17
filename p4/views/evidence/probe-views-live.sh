#!/bin/bash
# Live views leg: real manager + real session + real wayvnc in the L1 image.
set -u
ROOT=/tmp/dr-views
rm -rf "$ROOT"
mkdir -p "$ROOT"/config "$ROOT"/data "$ROOT"/run "$ROOT"/live "$ROOT"/tmp
export HOME=/root
export XDG_CONFIG_HOME="$ROOT/config"
export XDG_DATA_HOME="$ROOT/data"
export XDG_RUNTIME_DIR="$ROOT/run"
export TMPDIR="$ROOT/tmp"

daiorama config setup >/dev/null 2>&1 || true
# A minimal asset tree: the live leg proves routing and the gate, not noVNC's JS bundle.
mkdir -p "$ROOT/data/daiorama/default/novnc"
printf '<html><body>noVNC fixture</body></html>\n' > "$ROOT/data/daiorama/default/novnc/vnc.html"

daiorama serve > "$ROOT/serve.log" 2>&1 &
SERVE=$!
for _ in $(seq 1 120); do
    [ -S "$ROOT/run/daiorama/default/manager.sock" ] && break
    sleep 0.5
done

daiorama new --json --resolution 800x600 > "$ROOT/new.json" 2> "$ROOT/new.err"
echo "new rc=$?"
cat "$ROOT/new.json" "$ROOT/new.err"
ID=$(python3 -c "import json;print(json.load(open('$ROOT/new.json'))['id'])" 2>/dev/null)
echo "session id=$ID"
if [ -z "$ID" ]; then
    echo "NO SESSION"; tail -30 "$ROOT/serve.log"; kill $SERVE 2>/dev/null; exit 1
fi

python3 $(dirname "$0")/probe-views-live.py "$ID" > "$ROOT/drive.out" 2>&1
DRIVE=$?
echo "drive rc=$DRIVE"
cat "$ROOT/drive.out"

daiorama close "$ID" >/dev/null 2>&1
daiorama daemon end >/dev/null 2>&1
kill $SERVE 2>/dev/null
wait $SERVE 2>/dev/null
exit $DRIVE
