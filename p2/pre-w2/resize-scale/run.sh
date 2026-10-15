#!/usr/bin/env bash
# P2 resize/scale L1-extension — resolution + scale at creation, live resize, capture dims.
# Runs inside dai-l1 container: mutter parent -> Hyprland nested -> foot -> grim/wtype.
# Expected semantics (to be confirmed by THIS run): Hyprland `monitor` resolution = output MODE
# (buffer pixels); scale divides to logical size. Capture (grim/screencopy) = buffer pixels.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf AQ_TRACE=1

echo "===== A. parent: mutter --headless (render node, no KMS) ====="
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 8
echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"

echo "===== B. child: Hyprland nested ====="
WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
HP=$!
sleep 12
echo "hyprland alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no)"
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1); export HYPRLAND_INSTANCE_SIGNATURE=$SIG
echo "sig=$SIG"
hyprctl output create headless HEADLESS-R 2>&1 | head -2
sleep 1
echo "--- compositor renderer ---"
grep -E 'Creating the Hypr OpenGL Renderer|Using: OpenGL|Renderer:' /tmp/xdg/hypr/$SIG/hyprland.log 2>/dev/null | head -4

echo "===== C. foot window + initial input ====="
WS=$(ls /tmp/xdg | grep '^wayland-' | grep -v "^wayland-m$" | head -1)
echo "hyprland socket: $WS"
export WAYLAND_DISPLAY=$WS
foot --app-id=dai-resize-test > /tmp/foot.log 2>&1 &
FOOTPID=$!
sleep 4
wtype 'dAIorama-resize-input-OK'
sleep 1

apply_and_shot() { # $1=label $2=mode $3=scale
  echo "===== STEP $1: mode=$2 scale=$3 ====="
  hyprctl keyword monitor "HEADLESS-R,$2,0x0,$3" 2>&1 | head -2
  sleep 2
  timeout 10 grim /tmp/rs-$1.png 2>&1 && echo "grim OK" || echo "grim FAILED"
  cp /tmp/rs-$1.png /out/p2-resize-$1.png 2>/dev/null || true
  python3 - "$1" <<'PYEOF'
import json, subprocess, struct, sys
out = json.loads(subprocess.run(['hyprctl','monitors','-j'], capture_output=True, text=True).stdout)
for m in out:
    if m.get('name') == 'HEADLESS-R':
        print('monitor-json: w=%s h=%s scale=%s physW=%s physH=%s refresh=%s' % (
            m.get('width'), m.get('height'), m.get('scale'),
            m.get('physicalWidth','?'), m.get('physicalHeight','?'), m.get('refreshRate')))
try:
    d = open('/tmp/rs-%s.png' % sys.argv[1],'rb').read(33)
    w,h = struct.unpack('>II', d[16:24])
    print('PNG dims: %dx%d' % (w,h))
except Exception as e:
    print('PNG read fail:', e)
PYEOF
}

echo "===== D. resize/scale matrix ====="
apply_and_shot 2560x1440-s1 "2560x1440@60" 1
apply_and_shot 1920x1080-s2 "1920x1080@60" 2
apply_and_shot 3840x2160-s2 "3840x2160@60" 2
apply_and_shot 1280x800-s1 "1280x800@60" 1
apply_and_shot 1920x1080-s15 "1920x1080@60" 1.5

echo "===== E. input survival after live resize ====="
wtype ' resize4-OK'
sleep 1
hyprctl clients 2>&1 | grep -E 'class|title' | head -6
timeout 10 grim /tmp/rs-final.png 2>&1 && { cp /tmp/rs-final.png /out/p2-resize-final.png; echo "final grim OK"; } || echo "final grim FAILED"
hyprctl cursorpos 2>&1
hyprctl monitors -j > /out/p2-resize-monitors-final.json 2>/dev/null || true

echo "===== F. evidence copies ====="
for f in mutter hypr foot; do cp /tmp/$f.log /out/p2-resize-$f.log 2>/dev/null; done
cp /tmp/xdg/hypr/$SIG/hyprland.log /out/p2-resize-hyprland-instance.log 2>/dev/null || true
ls -la /out/p2-resize-* 2>/dev/null
echo "=== done ==="
