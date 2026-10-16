#!/usr/bin/env bash
# Diagnostic — what does the compositor do when a fractional scale yields a non-integer
# logical size? Minimal chain (mutter --headless -> nested Hyprland), one headless output,
# monitor rules applied one by one with readbacks, plus the instance-log delta per apply.
# This isolates the daiorama-external question behind the round-1(c) readback mismatch:
# daiorama asked for HEADLESS-1,962x722@60,0x0,1.5 and the compositor reported scale=2.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf

echo "===== A. parent: mutter --headless ====="
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 6
echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"

echo "===== B. child: Hyprland nested ====="
WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
HP=$!
sleep 8
echo "hyprland alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no)"
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1)
export HYPRLAND_INSTANCE_SIGNATURE=$SIG
echo "sig=$SIG"
echo "-- version:"; hyprctl version | head -4
HYPRLOG=/tmp/xdg/hypr/$SIG/hyprland.log
echo "instance log: $HYPRLOG ($(wc -l < "$HYPRLOG") lines)"

echo "===== C. headless output ====="
hyprctl output create headless HEADLESS-R 2>&1 | head -2

apply() { # apply <mode> <scale>
  local mode="$1" scale="$2" lines0 lines1
  echo "----- apply monitor rule: HEADLESS-R,${mode}@60,0x0,${scale}"
  lines0=$(wc -l < "$HYPRLOG")
  hyprctl keyword monitor "HEADLESS-R,${mode}@60,0x0,${scale}" 2>&1 | head -3
  sleep 2
  python3 - "$mode" "$scale" <<'PYEOF'
import json, subprocess, sys
out = json.loads(subprocess.run(['hyprctl','monitors','-j'], capture_output=True, text=True).stdout)
for m in out:
    if m.get('name') == 'HEADLESS-R':
        print('readback: requested %s @%s -> w=%s h=%s scale=%s refresh=%s' % (
            sys.argv[1], sys.argv[2], m.get('width'), m.get('height'), m.get('scale'), m.get('refreshRate')))
PYEOF
  lines1=$(wc -l < "$HYPRLOG")
  echo "-- new instance-log lines mentioning scale/monitor ($((lines1-lines0)) new):"
  tail -n +"$((lines0+1))" "$HYPRLOG" | grep -iE 'scale|monitor|mode|res ' | head -14
  echo
}

echo "===== D. probe matrix ====="
apply "962x722" "1.5"
apply "1920x1200" "1.5"
apply "960x720" "1.5"
apply "2560x1440" "1.5"
apply "1920x1000" "1.5"
apply "962x722" "1.4"
apply "962x722" "1.6"
apply "1920x1080" "1.33"
apply "962x722" "1.5"

echo "===== E. full monitors JSON (current state) ====="
hyprctl monitors -j | python3 -m json.tool | head -100

echo "===== F. grep instance log for scale-ish lines (whole file) ====="
grep -inE 'scale|snap|fractional|round' "$HYPRLOG" | tail -25

echo "=== done ==="
