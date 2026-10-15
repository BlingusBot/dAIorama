#!/usr/bin/env bash
# L1 attempt 4: same as 3 + verbose logging (debug:disable_logs=false) + /run/udev visibility.
set -u
export HOME=/root
export XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
mkdir -p /root/.config/hypr
cat > /root/hypr.conf <<'EOF'
debug {
    disable_logs = false
    enable_stdout_logs = true
}
misc {
    disable_autoreload = true
}
EOF
export HYPRLAND_CONFIG=/root/hypr.conf
export AQ_NO_KMS_REQUIREMENT=1
export AQ_DRM_DEVICES=/dev/dri/renderD128
export AQ_TRACE=1
export HYPRLAND_TRACE=1
echo "== /run/udev visible? =="; ls -la /run/udev/ 2>&1 | head -8; ls /run/udev/data 2>/dev/null | head -5; echo
echo "== /sys/class/drm =="; ls /sys/class/drm/ 2>&1 | head -12; echo

attempt() {
    MODE=$1
    echo "===== ATTEMPT: LIBSEAT_BACKEND=$MODE ====="
    if [ "$MODE" = "seatd" ]; then
        /usr/sbin/seatd -g root > /tmp/seatd.log 2>&1 &
        sleep 1
    fi
    export LIBSEAT_BACKEND=$MODE
    Hyprland --i-am-really-stupid > /tmp/hypr-$MODE.log 2>&1 &
    HP=$!
    sleep 15
    echo "pid=$HP alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no)"
    if kill -0 $HP 2>/dev/null; then
        echo "--- hyprctl monitors ---"; hyprctl monitors 2>&1 | head -25
        echo "--- output create headless ---"; hyprctl output create headless 2>&1 | head -6; sleep 2
        echo "--- monitors again ---"; hyprctl monitors 2>&1 | head -45
        W=$(ls /tmp/xdg 2>/dev/null | grep -m1 '^wayland-[0-9]$' || true); echo "socket=$W"
        if [ -n "$W" ]; then
            export WAYLAND_DISPLAY=$W
            echo "--- grim ---"; timeout 10 grim /tmp/shot.png 2>&1 && { cp /tmp/shot.png /out/l1-shot-$MODE.png; echo "grim OK"; } || echo "grim FAILED"
        fi
    fi
    echo "--- hypr-$MODE.log: aquamarine/drm/session lines ---"
    grep -nE 'aquamarine|drm:|seat|session|Backend|backend|headless|CRIT' /tmp/hypr-$MODE.log 2>/dev/null | tail -100
    echo "--- log tail 60 ---"; tail -60 /tmp/hypr-$MODE.log 2>/dev/null
    kill $HP 2>/dev/null; sleep 1; kill -9 $HP 2>/dev/null; true
    echo
}

attempt builtin
attempt seatd
echo "done rc=$?"
