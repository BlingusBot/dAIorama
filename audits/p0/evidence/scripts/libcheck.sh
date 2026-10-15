#!/bin/bash
echo "== dri dirs =="; ls /usr/lib/x86_64-linux-gnu/dri/ 2>&1 | head -30
echo "== tools =="; for t in glxinfo eglinfo vulkaninfo python3 modetest drmdevice; do printf '%s: ' "$t"; command -v "$t" || echo missing; done
echo "== libs =="; ls /usr/lib/x86_64-linux-gnu/ 2>/dev/null | grep -iE '^libEGL|^libGL|^libgbm|^libvulkan' | head -20
echo "== done =="
