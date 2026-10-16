#!/usr/bin/env bash
# Diagnostic 3 — readback timing: how fast does `hyprctl monitors -j` reflect a freshly
# applied monitor rule (the spawner will verify the readback right after applying)? Plus the
# default state of a virgin headless output, and a re-check of the two diag-2 cases whose
# readback looked stale (1920x1080@0.25, repeat 962x722@1.5).
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n    disable_scale_checks = true\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf

mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
sleep 6
WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
sleep 8
SIG=$(ls /tmp/xdg/hypr/ 2>/dev/null | head -1)
export HYPRLAND_INSTANCE_SIGNATURE=$SIG
echo "sig=$SIG"
hyprctl getoption debug:disable_scale_checks | head -4

mon() { # print all monitors, one line each
  python3 - <<'PYEOF'
import json, subprocess
out = json.loads(subprocess.run(['hyprctl','monitors','-j'], capture_output=True, text=True).stdout)
for m in out:
    print('  mon: %-12s w=%-6s h=%-6s scale=%-8s pos=%sx%s' % (
        m.get('name'), m.get('width'), m.get('height'), m.get('scale'), m.get('x'), m.get('y')))
PYEOF
}

echo "===== A. virgin output: create, then readback with timing ====="
hyprctl output create headless HEADLESS-1 2>&1 | head -2
for t in 0.3 1 2; do sleep $t; echo "after +${t}s:"; mon; done

probe() { # probe <label> <mode> <scale>
  local label="$1" mode="$2" scale="$3"
  echo "===== $label: apply ${mode}@60,0x0,${scale} ====="
  hyprctl keyword monitor "HEADLESS-1,${mode}@60,0x0,${scale}" 2>&1 | head -2
  for t in 0.3 1 2 4; do sleep $t; echo "after +${t}s:"; mon; done
}

probe "B" "962x722" "1.5"
probe "C" "803x603" "1.25"
probe "D" "1920x1080" "0.25"
probe "E-repeat-962" "962x722" "1.5"

echo "===== F. final state + instance log tail ====="
mon
HYPRLOG=/tmp/xdg/hypr/$SIG/hyprland.log
grep -iE 'data dump|Invalid scale|HEADLESS-1' "$HYPRLOG" | tail -20
echo "=== done ==="
