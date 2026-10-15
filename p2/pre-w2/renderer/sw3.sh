#!/usr/bin/env bash
# v3: soft-compositor session (GPU mutter + llvmpipe Hyprland) — capture-route variants,
# wayvnc viability, chromium software (SwiftShader) browser leg.
set -u
export HOME=/root XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
printf 'debug {\n    disable_logs = false\n}\n' > /root/hypr.conf
export HYPRLAND_CONFIG=/root/hypr.conf
SOFT="LIBGL_ALWAYS_SOFTWARE=1 MESA_LOADER_DRIVER_OVERRIDE=llvmpipe"

mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m > /tmp/mutter.log 2>&1 &
MP=$!; sleep 8; echo "mutter alive=$(kill -0 $MP 2>/dev/null && echo yes || echo no)"

env $SOFT WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid > /tmp/hypr.log 2>&1 &
HP=$!
SIG=""
for i in $(seq 1 20); do sleep 1; SIG=$(ls -t /tmp/xdg/hypr/ 2>/dev/null | head -1); [ -n "$SIG" ] && grep -q 'Creating the Hypr OpenGL Renderer' /tmp/xdg/hypr/$SIG/hyprland.log 2>/dev/null && break; done
sleep 2; export HYPRLAND_INSTANCE_SIGNATURE=$SIG
echo "hypr alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no) sig=$SIG"
echo "renderer: $(grep -E 'Renderer:' /tmp/xdg/hypr/$SIG/hyprland.log 2>/dev/null | head -1)"
hyprctl output create headless HEADLESS-D 2>&1 | head -1
WS=$(ls /tmp/xdg | grep '^wayland-' | grep -v '^wayland-m$' | head -1); echo "hypr sock: $WS"

echo "== grim variants =="
echo "-- v1 plain:"; WAYLAND_DISPLAY=$WS timeout 10 grim /tmp/g1.png 2>&1; echo "rc=$?"
echo "-- v2 ppm:"; WAYLAND_DISPLAY=$WS timeout 10 grim -t ppm /tmp/g2.ppm 2>&1; echo "rc=$?"
echo "-- v3 soft-env png:"; WAYLAND_DISPLAY=$WS timeout 20 env $SOFT grim /tmp/g3.png 2>&1; echo "rc=$?"
echo "-- v4 soft-env ppm:"; WAYLAND_DISPLAY=$WS timeout 20 env $SOFT grim -t ppm /tmp/g4.ppm 2>&1; echo "rc=$?"
ls -la /tmp/g* 2>/dev/null

echo "== wayvnc on soft session =="
WAYLAND_DISPLAY=$WS timeout 30 wayvnc 127.0.0.1 5900 > /tmp/wvnc.log 2>&1 &
WV=$!; sleep 5
echo "wayvnc alive=$(kill -0 $WV 2>/dev/null && echo yes || echo no)"; head -8 /tmp/wvnc.log
ss -ltn 2>/dev/null | grep 5900 || echo "no 5900 listen"
kill $WV 2>/dev/null

echo "== chromium software (swiftshader) leg =="
cp /out/webgl-test.html /tmp/webgl-test.html 2>/dev/null
timeout 60 env $SOFT WAYLAND_DISPLAY=$WS chromium --ozone-platform=wayland --no-sandbox --disable-dev-shm-usage \
  --use-angle=swiftshader --enable-unsafe-swiftshader --user-data-dir=/tmp/chrome-sw --window-size=1200,700 \
  file:///tmp/webgl-test.html > /tmp/chromium-sw.log 2>&1 &
CR=$!; sleep 18
echo "-- titles:"; hyprctl clients 2>&1 | grep -iE 'title' | head -4
WAYLAND_DISPLAY=$WS timeout 20 env $SOFT grim /tmp/sw-browser.png 2>&1 && { cp /tmp/sw-browser.png /out/p2-renderer-sw-browser.png; echo "browser grim OK"; } || echo "browser grim FAILED"

echo "== evidence =="
cp /tmp/wvnc.log /out/p2-renderer-sw-wayvnc.log 2>/dev/null
cp /tmp/chromium-sw.log /out/p2-renderer-sw-chromium-sw.log 2>/dev/null
cp /tmp/xdg/hypr/$SIG/hyprland.log /out/p2-renderer-sw2-instance.log 2>/dev/null
kill $CR $HP $MP 2>/dev/null; sleep 1; kill -9 $CR $HP $MP 2>/dev/null
echo done
