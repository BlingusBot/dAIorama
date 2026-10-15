#!/usr/bin/env bash
# P2 renderer-gate diag — why does Hyprland abort under software-GL env, and does llvmpipe
# work at all? Control + 4 env variants, each captured with renderer line + grim if alive.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf

echo "===== A. llvmpipe GL sanity (surfaceless EGL) ====="
for variant in "NONE" "LIBGL_ALWAYS_SOFTWARE=1" "MESA_LOADER_DRIVER_OVERRIDE=llvmpipe" "GALLIUM_DRIVER=llvmpipe"; do
  echo "-- variant: $variant"
  if [ "$variant" = "NONE" ]; then
    EGL_PLATFORM=surfaceless python3 /out/p2-egl2-test.py 2>&1 | grep -E 'eglInitialize|EGL_VENDOR|GL_RENDERER|GL_VENDOR|Renderer' | head -5
  else
    env $variant EGL_PLATFORM=surfaceless python3 /out/p2-egl2-test.py 2>&1 | grep -E 'eglInitialize|EGL_VENDOR|GL_RENDERER|GL_VENDOR|Renderer' | head -5
  fi
  echo "   rc=$?"
done

echo "===== B. mutter parent ====="
mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!
sleep 8
echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"

try_hypr() { # $1=label, EXTRA env via global
  local label=$1
  echo "env-mark: ${EXTRA:-none} | inside: $(env | grep -cE 'LIBGL_ALWAYS_SOFTWARE|MESA_LOADER_DRIVER_OVERRIDE|GALLIUM_DRIVER' || true)"
  echo "===== ATTEMPT $label (${EXTRA:-no extra env}) ====="
  timeout --kill-after=2 90 env ${EXTRA:-} WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/$label.log 2>&1 &
  local TP=$!
  local ok=no i
  for i in $(seq 1 20); do
    sleep 1
    grep -q 'Creating the Hypr OpenGL Renderer' /tmp/$label.log 2>/dev/null && { ok=yes; break; }
    kill -0 $TP 2>/dev/null || break
  done
  sleep 2
  if kill -0 $TP 2>/dev/null; then
    echo "RUNNING (renderer line found: $ok)"
    grep -E 'Renderer:|Using: OpenGL' /tmp/$label.log | head -3
    local SIG
    SIG=$(ls -t /tmp/xdg/hypr/ 2>/dev/null | head -1)
    HYPRLAND_INSTANCE_SIGNATURE=$SIG hyprctl output create headless HEADLESS-D 2>&1 | head -1
    sleep 1
    timeout 10 grim /tmp/$label.png 2>&1 && { cp /tmp/$label.png /out/p2-diag-$label.png; python3 -c "import struct;d=open('/tmp/$label.png','rb').read(33);w,h=struct.unpack('>II',d[16:24]);print('grim OK %dx%d'%(w,h))"; } || echo "grim fail"
    kill $TP 2>/dev/null; sleep 1; kill -9 $TP 2>/dev/null
  else
    echo "DEAD before readiness; tail:"
    tail -7 /tmp/$label.log
  fi
  cp /tmp/$label.log /out/p2-diag-$label.log 2>/dev/null
  echo
}

try_hypr h-default
EXTRA="LIBGL_ALWAYS_SOFTWARE=1" try_hypr h-soft
EXTRA="LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe" try_hypr h-soft-ovr
EXTRA="MESA_LOADER_DRIVER_OVERRIDE=llvmpipe" try_hypr h-ovr
EXTRA="GALLIUM_DRIVER=llvmpipe" try_hypr h-gallium

echo "===== C. cleanup ====="
kill $MP 2>/dev/null; kill -9 $MP 2>/dev/null; true
echo done
