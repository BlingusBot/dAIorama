#!/usr/bin/env bash
# L1 FULL CHAIN RUN — the repeatable harness core:
# rootless podman + render node → mutter headless parent → Hyprland nested (headless outputs)
# → virtual input (wtype) → capture (grim) → wayvnc → noVNC(websockify) → browser WebGL (chromium).
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf AQ_TRACE=1
cp /out/webgl-test.html /tmp/webgl-test.html 2>/dev/null || echo "webgl page missing in /out"

echo "===== A. parent: mutter --headless (render node, no KMS) ====="
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 8
echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"; grep -E 'renderD|virtual monitor|Wayland display' /tmp/mutter.log | head -5

echo "===== B. child: Hyprland nested ====="
WAYSOCK=$(ls /tmp/xdg | grep '^wayland-' | head -1)   # may be wayland-m or next
[ -n "$WAYSOCK" ] && [ "$WAYSOCK" != "wayland-m" ] && WAYSOCK=wayland-m
WAYLAND_DISPLAY=$WAYSOCK Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
HP=$!
sleep 12
echo "hyprland alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no)"
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1); export HYPRLAND_INSTANCE_SIGNATURE=$SIG
echo "sig=$SIG"
hyprctl output create headless HEADLESS-T1 2>&1 | head -2
sleep 1
hyprctl monitors 2>&1 | sed -n '1,10p'

echo "===== C. input leg: foot window + wtype text ====="
WS=$(ls /tmp/xdg | grep '^wayland-' | grep -v "^$WAYSOCK$" | head -1); echo "hyprland socket: $WS"
export WAYLAND_DISPLAY=$WS
foot --app-id=dai-input-test > /tmp/foot.log 2>&1 &
FOOTPID=$!
sleep 4
echo "foot alive=$(kill -0 $FOOTPID 2>/dev/null && echo yes || echo no)"
wtype 'dAIorama-input-OK-123'
sleep 1
hyprctl clients 2>&1 | grep -E 'class|title|Window' | head -8
timeout 10 grim /tmp/shot-input.png 2>&1 && { cp /tmp/shot-input.png /out/l1-chain-input.png; echo "grim input-shot OK"; } || echo "grim input-shot FAILED"

echo "===== D. remote leg: wayvnc + websockify/noVNC ====="
wayvnc 127.0.0.1 5900 > /tmp/wayvnc.log 2>&1 &
WV=$!
sleep 3
echo "wayvnc alive=$(kill -0 $WV 2>/dev/null && echo yes || echo no)"; head -10 /tmp/wayvnc.log
ss -ltn 2>/dev/null | grep -E '5900|6080' || netstat -ltn 2>/dev/null | grep -E '5900|6080' || echo "no ss/netstat"
websockify --web=/usr/share/novnc 6080 127.0.0.1:5900 > /tmp/websockify.log 2>&1 &
WSK=$!
sleep 3
echo "websockify alive=$(kill -0 $WSK 2>/dev/null && echo yes || echo no)"
CODE=$(curl -s -o /tmp/vnc.html -w '%{http_code}' http://127.0.0.1:6080/vnc.html 2>&1); echo "noVNC vnc.html HTTP $CODE"; wc -c /tmp/vnc.html 2>/dev/null
head -3 /tmp/websockify.log

echo "===== E. WebGL leg: chromium (wayland) on the test page ====="
timeout 40 chromium --ozone-platform=wayland --no-sandbox --disable-dev-shm-usage \
  --user-data-dir=/tmp/chrome --window-size=1200,700 --window-position=50,50 \
  file:///tmp/webgl-test.html > /tmp/chromium.log 2>&1 &
CR=$!
sleep 15
echo "chromium alive=$(kill -0 $CR 2>/dev/null && echo yes || echo no)"
hyprctl clients 2>&1 | grep -E 'class|title' | head -10
timeout 10 grim /tmp/shot-webgl.png 2>&1 && { cp /tmp/shot-webgl.png /out/l1-chain-webgl.png; echo "grim webgl-shot OK"; } || echo "grim webgl-shot FAILED"

echo "===== F. evidence copies ====="
for f in mutter hypr foot wayvnc websockify chromium; do cp /tmp/$f.log /out/l1-chain-$f.log 2>/dev/null; done
ls -la /out/ | grep -E 'l1-chain|l1-'

echo "===== G. teardown ====="
kill $CR $WSK $WV $FOOTPID $HP $MP 2>/dev/null; sleep 1; kill -9 $CR $WSK $WV $FOOTPID $HP $MP 2>/dev/null; true
echo done
