#!/usr/bin/env bash
# L1 pointer round: wlrctl (zwlr_virtual_pointer) via Hyprland nested in mutter; cursorpos deltas as proof.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf AQ_TRACE=1
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 8
echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"
WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
HP=$!
sleep 12
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1); export HYPRLAND_INSTANCE_SIGNATURE=$SIG
hyprctl output create headless HEADLESS-T1 2>&1 | head -1
sleep 1
WS=$(ls /tmp/xdg | grep '^wayland-' | grep -v '^wayland-m$' | head -1); export WAYLAND_DISPLAY=$WS
echo "socket=$WS sig=$SIG"

echo "===== renderer evidence (compositor side) ====="
hyprctl systeminfo 2>&1 | grep -iE 'renderer|GPU|Mesa|NV|llvm|gpu' | head -30

echo "===== wlrctl usage ====="
wlrctl --help 2>&1 | head -25
echo "--- pointer subcommand:"
wlrctl pointer --help 2>&1 | head -15

echo "===== baseline ====="
C0=$(hyprctl cursorpos 2>&1); echo "cursorpos: $C0"

echo "===== step 1: wlrctl pointer move 300 200 ====="
wlrctl pointer move 300 200 2>&1; echo "rc=$?"
sleep 1; C1=$(hyprctl cursorpos 2>&1); echo "cursorpos: $C1"

echo "===== step 2: wlrctl pointer move -123 77 ====="
wlrctl pointer move -123 77 2>&1; echo "rc=$?"
sleep 1; C2=$(hyprctl cursorpos 2>&1); echo "cursorpos: $C2"

echo "===== step 3: wlrctl pointer click left (rc only; no client target) ====="
wlrctl pointer click left 2>&1; echo "rc=$?"

echo "===== step 4: wlrctl pointer scroll ====="
wlrctl pointer scroll 5 2>&1 | head -2; echo "rc=$?"

echo "===== step 5: foot + wlrctl keyboard type ====="
foot --app-id=dai-pointer-test > /tmp/foot.log 2>&1 &
sleep 3
wlrctl keyboard type 'wlrctl-keyboard-OK-42' 2>&1; echo "rc=$?"
sleep 1
hyprctl clients 2>&1 | grep -E 'class|title' | head -6

echo "===== capture ====="
timeout 10 grim /tmp/shot-pointer.png && { cp /tmp/shot-pointer.png /out/l1-pointer-round.png; echo "grim OK"; } || echo "grim FAILED"

echo "===== deltas ====="
echo "C0: $C0"; echo "C1: $C1"; echo "C2: $C2"
cp /tmp/hypr.log /out/hypr-pointer-round.log 2>/dev/null || true
cp /tmp/xdg/hypr/$SIG/hyprland.log /out/hyprland-instance.log 2>/dev/null && echo "instance log copied" || echo "instance log copy skipped"

kill $HP $MP 2>/dev/null; sleep 1; kill -9 $HP $MP 2>/dev/null; true
echo done
