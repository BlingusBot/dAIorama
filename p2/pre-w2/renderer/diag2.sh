#!/usr/bin/env bash
# v2: proper captures. Phase 1: mutter GPU + hypr soft (control + soft + browser). Phase 2: mutter soft + hypr soft.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf

start_mutter() { # $1=label, global MUTTER_ENV
  timeout --kill-after=2 600 env ${MUTTER_ENV:-} mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter-$1.log 2>&1 &
  MP=$!
  sleep 8
  echo "mutter($1, env=${MUTTER_ENV:-gpu}) alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"
}

run_hypr() { # $1=label, $2=env string
  local label=$1 extra=$2
  echo "===== HYPR $label (env: ${extra:-none}) ====="
  timeout --kill-after=2 150 env ${extra:-} WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/$label.log 2>&1 &
  local TP=$!
  local SIG="" i
  for i in $(seq 1 25); do
    sleep 1
    kill -0 $TP 2>/dev/null || break
    SIG=$(ls -t /tmp/xdg/hypr/ 2>/dev/null | head -1)
    [ -n "$SIG" ] && grep -q 'Creating the Hypr OpenGL Renderer' /tmp/xdg/hypr/$SIG/hyprland.log 2>/dev/null && break
  done
  sleep 2
  if kill -0 $TP 2>/dev/null; then
    SIG=$(ls -t /tmp/xdg/hypr/ 2>/dev/null | head -1)
    echo "ALIVE sig=$SIG"
    echo "-- instance renderer:"; grep -E 'Renderer:|Using: OpenGL' /tmp/xdg/hypr/$SIG/hyprland.log 2>/dev/null | head -3
    export HYPRLAND_INSTANCE_SIGNATURE=$SIG
    hyprctl output create headless HEADLESS-D 2>&1 | head -1
    sleep 1
    hyprctl monitors 2>&1 | grep -E 'HEADLESS-D' | head -2
    local WS
    WS=$(ls /tmp/xdg | grep '^wayland-' | grep -v '^wayland-m$' | head -1)
    echo "hypr socket: $WS"
    WAYLAND_DISPLAY=$WS timeout 10 grim /tmp/$label.png 2>&1 && { cp /tmp/$label.png /out/p2-diag2-$label.png; python3 -c "import struct;d=open('/tmp/$label.png','rb').read(33);w,h=struct.unpack('>II',d[16:24]);print('grim OK %dx%d'%(w,h))"; } || echo "grim FAILED"
    if [ "${DO_BROWSER:-0}" = "1" ]; then
      cp /out/webgl-test.html /tmp/webgl-test.html 2>/dev/null
      timeout 40 env ${extra:-} WAYLAND_DISPLAY=$WS chromium --ozone-platform=wayland --no-sandbox --disable-dev-shm-usage \
        --user-data-dir=/tmp/chrome-$label --window-size=1200,700 file:///tmp/webgl-test.html > /tmp/chromium-$label.log 2>&1 &
      local CR=$!
      sleep 15
      echo "-- browser titles:"; hyprctl clients 2>&1 | grep -iE 'title' | head -4
      WAYLAND_DISPLAY=$WS timeout 10 grim /tmp/$label-browser.png 2>&1 && { cp /tmp/$label-browser.png /out/p2-diag2-$label-browser.png; echo "browser grim OK"; } || echo "browser grim FAILED"
      kill $CR 2>/dev/null; kill -9 $CR 2>/dev/null
    fi
    kill $TP 2>/dev/null; sleep 1; kill -9 $TP 2>/dev/null
  else
    echo "DEAD; stdout tail:"; tail -6 /tmp/$label.log
    SIG=$(ls -t /tmp/xdg/hypr/ 2>/dev/null | head -1)
    [ -n "$SIG" ] && { echo "instance tail:"; tail -8 /tmp/xdg/hypr/$SIG/hyprland.log 2>/dev/null; }
  fi
  cp /tmp/$label.log /out/p2-diag2-$label.stdout.log 2>/dev/null
  SIG=$(ls -t /tmp/xdg/hypr/ 2>/dev/null | head -1)
  [ -n "$SIG" ] && cp /tmp/xdg/hypr/$SIG/hyprland.log /out/p2-diag2-$label.instance.log 2>/dev/null
  echo
}

echo "========== PHASE 1: mutter GPU =========="
MUTTER_ENV="" start_mutter gpu
run_hypr h1-gpu-cntrl ""
run_hypr h2-gpu-soft "LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe"
DO_BROWSER=1 run_hypr h2b-gpu-soft-browser "LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe"
kill $MP 2>/dev/null; sleep 1; kill -9 $MP 2>/dev/null

echo "========== PHASE 2: mutter software =========="
MUTTER_ENV="LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe" start_mutter soft
run_hypr h3-soft-soft "LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe"
kill $MP 2>/dev/null; sleep 1; kill -9 $MP 2>/dev/null
cp /tmp/mutter-gpu.log /out/p2-diag2-mutter-gpu.log 2>/dev/null
cp /tmp/mutter-soft.log /out/p2-diag2-mutter-soft.log 2>/dev/null
echo done
