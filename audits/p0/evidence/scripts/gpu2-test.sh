#!/bin/bash
echo "== id =="
id
echo "== /dev/dri =="
ls -ln /dev/dri/ 2>&1
echo "== open+ioctl (baseline) =="
python3 - <<'PY'
import os, ctypes, fcntl
fd = os.open('/dev/dri/renderD128', os.O_RDWR | os.O_CLOEXEC)
print('open O_RDWR OK')
class V(ctypes.Structure):
    _fields_ = [('major', ctypes.c_int), ('minor', ctypes.c_int), ('patch', ctypes.c_int),
                ('name_len', ctypes.c_size_t), ('name', ctypes.c_char_p),
                ('date_len', ctypes.c_size_t), ('date', ctypes.c_char_p),
                ('desc_len', ctypes.c_size_t), ('desc', ctypes.c_char_p)]
n = ctypes.create_string_buffer(256); dt = ctypes.create_string_buffer(256); de = ctypes.create_string_buffer(512)
v = V(0, 0, 0, 256, ctypes.cast(n, ctypes.c_char_p), 256, ctypes.cast(dt, ctypes.c_char_p), 512, ctypes.cast(de, ctypes.c_char_p))
fcntl.ioctl(fd, 0xc0406400, v)
print('DRM version:', v.major, v.minor, v.patch, '| name:', n.value.decode())
PY
echo "== EGL/GBM GLES context test =="
python3 /out/egl-test.py
echo "== cgroup rw test (embedded-shape precondition) =="
mkdir /sys/fs/cgroup/dai 2>&1 && { rmdir /sys/fs/cgroup/dai; echo "cgroup mkdir OK"; } || echo "cgroup mkdir FAIL (read-only cgroupfs)"
echo "== done =="
