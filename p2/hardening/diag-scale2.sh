#!/usr/bin/env bash
# Diagnostic 2 — does `debug:disable_scale_checks = true` in the session config make the
# compositor honor the requested scale verbatim, and what do the readbacks look like then?
# Same minimal chain as diag-scale.sh; the session conf now carries the opt-out stanza.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n    disable_scale_checks = true\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf

echo "===== A. parent: mutter --headless ====="
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 6
echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"

echo "===== B. child: Hyprland nested (conf has disable_scale_checks = true) ====="
WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
HP=$!
sleep 8
echo "hyprland alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no)"
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1)
export HYPRLAND_INSTANCE_SIGNATURE=$SIG
echo "sig=$SIG"
HYPRLOG=/tmp/xdg/hypr/$SIG/hyprland.log
echo "-- config value seen by the compositor:"
hyprctl getoption debug:disable_scale_checks 2>&1 | head -6
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
  echo "-- new instance-log ERR/scale lines:"
  tail -n +"$((lines0+1))" "$HYPRLOG" | grep -iE 'scale|ERR|data dump' | head -8
  echo
}

echo "===== D. probe matrix (opt-out active) ====="
apply "962x722" "1.5"
apply "2560x1440" "1.5"
apply "1920x1080" "1.33"
apply "962x722" "1.4"
apply "803x603" "1.25"
apply "1920x1080" "0.25"

echo "===== E. confirm the option really is what unblocked it: flip back on, retry ====="
hyprctl keyword debug:disable_scale_checks false 2>&1 | head -2
apply "962x722" "1.5"

echo "=== done ==="
