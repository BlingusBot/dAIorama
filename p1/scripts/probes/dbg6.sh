#!/usr/bin/env bash
# dbg6: mutter --headless parent (no shim needed) → Hyprland nested → monitors + capture + protocol check.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y --no-install-recommends mutter > /tmp/apt.log 2>&1; echo "apt rc=$?"
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf AQ_TRACE=1
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 10
echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"
WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr-m.log 2>&1 &
HP=$!
sleep 15
echo "hyprland alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no)"
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1); echo "sig=$SIG"; export HYPRLAND_INSTANCE_SIGNATURE=$SIG
echo "--- instances ---"; hyprctl instances 2>&1 | head -14
echo "--- version ---"; hyprctl version 2>&1 | head -10
echo "--- monitors ---"; hyprctl monitors 2>&1 | head -40
echo "--- output create headless HEADLESS-T1 ---"; hyprctl output create headless HEADLESS-T1 2>&1 | head -3; sleep 2
echo "--- monitors after ---"; hyprctl monitors 2>&1 | head -70
echo "--- activeworkspace ---"; hyprctl activeworkspace 2>&1 | head -12
HS=$(ls /tmp/xdg | grep '^wayland-' | grep -v '^wayland-m$' | head -1); echo "hypr socket: $HS"
if [ -n "$HS" ]; then
    echo "--- grim ---"; WAYLAND_DISPLAY=$HS timeout 10 grim /tmp/shot-m.png 2>&1 && { ls -la /tmp/shot-m.png; cp /tmp/shot-m.png /out/l1-mutter-shot.png; echo "grim OK"; } || echo "grim FAILED"
    echo "--- hyprland global count ---"; WAYLAND_DISPLAY=$HS timeout 10 wayland-info 2>&1 | grep -cE '^interface:'
    echo "--- key protocols present? ---"; WAYLAND_DISPLAY=$HS timeout 10 wayland-info 2>&1 | grep -E "interface: '(zwlr_screencopy_manager_v1|zwlr_virtual_pointer_manager_v1|zwp_virtual_keyboard_manager_v1|zwlr_data_control_manager_v1|zwlr_output_manager_v1|wp_cursor_shape_manager_v1)'" | head -10
fi
echo "--- evidence copy ---"
cp /tmp/hypr-m.log /out/hypr-mutter-nested.log 2>/dev/null
cp /tmp/mutter.log /out/mutter-parent.log 2>/dev/null
kill $HP 2>/dev/null; kill $MP 2>/dev/null; sleep 1; kill -9 $HP $MP 2>/dev/null; true
echo done
