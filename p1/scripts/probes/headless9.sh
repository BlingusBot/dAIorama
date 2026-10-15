#!/usr/bin/env bash
# L1 attempt 8: Hyprland nested with bind-clamp shim, two parents tried (weston, sway).
set -u
export HOME=/root
export XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 700 /tmp/xdg
cat > /root/hypr.conf <<'EOF'
debug {
    disable_logs = false
}
EOF
export HYPRLAND_CONFIG=/root/hypr.conf
export AQ_TRACE=1
CLAMP=/usr/local/lib/xdg-clamp.so
ls -la $CLAMP 2>&1 || echo "SHIM MISSING"

try_parent() {
    PNAME=$1; PSOCKHINT=$2; shift 2
    echo "===== parent: $PNAME ====="
    "$@" > /tmp/parent-$PNAME.log 2>&1 &
    PP=$!
    sleep 6
    echo "parent pid=$PP alive=$(kill -0 $PP 2>/dev/null && echo yes || echo no)"
    head -15 /tmp/parent-$PNAME.log
    PSOCK=$(ls /tmp/xdg 2>/dev/null | grep -E "^${PSOCKHINT}$" | head -1)
    [ -z "$PSOCK" ] && PSOCK=$(ls /tmp/xdg 2>/dev/null | grep -E '^wayland-' | head -1)
    echo "parent socket: $PSOCK"
    echo "-- shim load check:"; LD_PRELOAD=$CLAMP ldd $CLAMP 2>&1 | head -4
    if [ -z "$PSOCK" ]; then kill $PP 2>/dev/null; return; fi
    echo "--- parent globals (seat/compositor/xdg/dmabuf) ---"
    WAYLAND_DISPLAY=$PSOCK wayland-info 2>&1 | grep -E "interface: '(wl_seat|wl_compositor|xdg_wm_base|zwp_linux_dmabuf_v1)'" | head -10
    echo "--- launch Hyprland nested (LD_PRELOAD=shim) ---"
    env WAYLAND_DISPLAY=$PSOCK LD_PRELOAD=$CLAMP Hyprland --i-am-really-stupid > /tmp/hypr8-$PNAME.log 2>&1 &
    HP=$!
    sleep 14
    echo "hyprland pid=$HP alive=$(kill -0 $HP 2>/dev/null && echo yes || echo no)"
    if kill -0 $HP 2>/dev/null; then
        echo "--- hyprctl version ---"; hyprctl version 2>&1 | head -8
        echo "--- hyprctl monitors ---"; hyprctl monitors 2>&1 | head -30
        echo "--- output create headless HEADLESS-T1 ---"; hyprctl output create headless HEADLESS-T1 2>&1 | head -4; sleep 2
        echo "--- monitors after ---"; hyprctl monitors 2>&1 | head -55
        HS=$(ls /tmp/xdg 2>/dev/null | grep '^wayland-[0-9]$' | grep -v "^$PSOCK$" | head -1)
        echo "hyprland socket: $HS"
        if [ -n "$HS" ]; then
            WAYLAND_DISPLAY=$HS timeout 10 grim /tmp/shot8.png 2>&1 && { ls -la /tmp/shot8.png; cp /tmp/shot8.png /out/l1-nested8-$PNAME-shot.png; echo "grim OK"; } || echo "grim FAILED"
        fi
    fi
    echo "--- hypr8-$PNAME.log key lines ---"
    grep -nE 'aquamarine|Wayland|seat|xdg|CRIT|clamp:|error' /tmp/hypr8-$PNAME.log 2>/dev/null | tail -70
    echo "--- tail 40 ---"; tail -40 /tmp/hypr8-$PNAME.log 2>/dev/null
    kill $HP 2>/dev/null; sleep 1; kill -9 $HP 2>/dev/null
    kill $PP 2>/dev/null; sleep 1; kill -9 $PP 2>/dev/null
    echo
}

try_parent weston wayland-w weston --backend=headless-backend.so --renderer=gl --socket=wayland-w --width=1920 --height=1080
try_parent sway wayland-1 env WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=gles2 WLR_RENDER_DRM_DEVICE=/dev/dri/renderD128 WLR_HEADLESS_OUTPUTS=1 sway
echo "== done =="
