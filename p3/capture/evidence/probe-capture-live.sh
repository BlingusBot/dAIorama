#!/bin/bash
# Live leg: real manager + real session (Hyprland in the L1 image) + configured fixture
# capture commands. Proves the session env plumbing, the tool dispatch, the stdio content
# mapping (image / resource_link + hint) and the `media fetch` CLI (claim-on-open).
set -u
ROOT=/tmp/dr-capture
export ROOT
rm -rf "$ROOT"
mkdir -p "$ROOT"/config "$ROOT"/data "$ROOT"/run "$ROOT"/live "$ROOT"/tmp
export HOME=/root
export XDG_CONFIG_HOME="$ROOT/config"
export XDG_DATA_HOME="$ROOT/data"
export XDG_RUNTIME_DIR="$ROOT/run"
export TMPDIR="$ROOT/tmp"

python3 - <<'PY'
import base64
png = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
)
open("/tmp/dr-capture/live/tiny.png", "wb").write(png)
PY
cat > "$ROOT/live/shot.sh" <<'EOF'
#!/bin/sh
cp /tmp/dr-capture/live/tiny.png "$1"
EOF
cat > "$ROOT/live/clip.sh" <<'EOF'
#!/bin/sh
printf 'webm-bytes' > "$1"
sleep 30
EOF
chmod +x "$ROOT/live/shot.sh" "$ROOT/live/clip.sh"

daiorama config setup >/dev/null 2>&1 || true

python3 - <<'PY'
import glob, re
paths = glob.glob("/tmp/dr-capture/config/**/config.ron", recursive=True)
print("config files:", paths)
assert paths, "config setup wrote no config"
path = paths[0]
text = open(path).read()
text = re.sub(r'screenshot_command: \[[^\]]*\]', 'screenshot_command: ["/tmp/dr-capture/live/shot.sh"]', text)
text = re.sub(r'clip_command: \[[^\]]*\]', 'clip_command: ["/tmp/dr-capture/live/clip.sh"]', text)
open(path, "w").write(text)
scopes = glob.glob("/tmp/dr-capture/config/**/scopes/*.ron", recursive=True)
print("scopes:", scopes)
for scope in scopes:
    if scope.endswith("operator.ron"):
        body = open(scope).read()
        body = re.sub(
            r'categories: \[[^\]]*\]',
            'categories: ["window", "input", "capture", "query", "launch", "views"]',
            body,
            count=1,
        )
        open(scope, "w").write(body)
PY

daiorama serve > "$ROOT/serve.log" 2>&1 &
SERVE=$!
for _ in $(seq 1 120); do
    [ -S "$ROOT/run/daiorama/default/manager.sock" ] && break
    sleep 0.5
done
ls -la "$ROOT/run/daiorama/default/" 2>/dev/null

daiorama new --json --resolution 800x600 > "$ROOT/new.json" 2> "$ROOT/new.err"
echo "new rc=$?"
cat "$ROOT/new.json" "$ROOT/new.err"
ID=$(python3 -c "import json;print(json.load(open('$ROOT/new.json'))['id'])" 2>/dev/null)
echo "session id=$ID"
if [ -z "$ID" ]; then
    echo "NO SESSION"; tail -40 "$ROOT/serve.log"; kill $SERVE 2>/dev/null; exit 1
fi

DAIORAMA_SCOPE=operator python3 $(dirname "$0")/probe-capture-live.py "$ID" > "$ROOT/drive.out" 2>&1
DRIVE=$?
echo "drive rc=$DRIVE"
cat "$ROOT/drive.out"

daiorama close "$ID" >/dev/null 2>&1
daiorama daemon end >/dev/null 2>&1
kill $SERVE 2>/dev/null
wait $SERVE 2>/dev/null
exit $DRIVE
