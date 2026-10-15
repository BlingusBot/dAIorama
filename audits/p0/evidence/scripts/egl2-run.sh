#!/bin/bash
OUT=/out/egl2.out
exec >"$OUT" 2>&1
echo "== python surfaceless (eglGetDisplay default) =="
EGL_PLATFORM=surfaceless python3 /out/egl2-test.py
echo "rc=$?"
echo
echo "== python gbm repeat (record of the crash path) =="
python3 /out/egl-test.py
echo "rc=$?"
echo
echo "== apt: mesa-utils for eglinfo =="
apt-get update -qq >/dev/null 2>&1
apt-get install -y -qq mesa-utils 2>&1 | tail -2
apt-get install -y -qq mesa-utils-bin 2>&1 | tail -2
for t in eglinfo glxinfo es2_info; do command -v "$t" || echo "$t missing"; done
echo
echo "== eglinfo surfaceless =="
EGL_PLATFORM=surfaceless timeout 30 eglinfo 2>&1 | head -80
echo "== eglinfo done =="
echo
echo "== mesa pkg version =="
dpkg -l | grep -i 'libegl-mesa0\|mesa-utils\|libgl1-mesa-dri' | awk '{print $2, $3}'
echo "== END =="
