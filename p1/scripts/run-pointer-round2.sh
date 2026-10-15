#!/usr/bin/env bash
# Pointer round 2: focus + click into the window, then wlrctl keyboard vs wtype control.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf AQ_TRACE=1
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 8
WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
HP=$!
sleep 12
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1); export HYPRLAND_INSTANCE_SIGNATURE=$SIG
hyprctl output create headless HEADLESS-T1 >/dev/null 2>&1; sleep 1
WS=$(ls /tmp/xdg | grep '^wayland-' | grep -v '^wayland-m$' | head -1); export WAYLAND_DISPLAY=$WS
foot --app-id=dai-pointer-test > /tmp/foot.log 2>&1 &
sleep 3

echo "== active window before =="; hyprctl activewindow 2>&1 | head -14
echo "== cursorpos before =="; hyprctl cursorpos 2>&1
echo "== move into window + click =="
wlrctl pointer move 200 150 2>&1; echo "move rc=$?"
sleep 1; hyprctl cursorpos 2>&1
wlrctl pointer click left 2>&1; echo "click rc=$?"
sleep 1
echo "== active window after click =="; hyprctl activewindow 2>&1 | head -14

echo "== wlrctl keyboard type =="
wlrctl keyboard type 'WLRCTL-KB-42' 2>&1; echo "rc=$?"
sleep 2
timeout 10 grim /tmp/shot2a.png && cp /tmp/shot2a.png /out/l1-pointer-round2a.png && echo "grim A OK" || echo "grim A FAILED"

echo "== wtype control =="
wtype ' WTYPE-OK-77' 2>&1; echo "rc=$?"
sleep 2
timeout 10 grim /tmp/shot2b.png && cp /tmp/shot2b.png /out/l1-pointer-round2b.png && echo "grim B OK" || echo "grim B FAILED"

cp /tmp/hypr.log /out/hypr-pointer-round2.log 2>/dev/null || true
kill $HP $MP 2>/dev/null; sleep 1; kill -9 $HP $MP 2>/dev/null; true
echo done
