#!/bin/bash
echo "== id =="
id
echo "== /dev/dri =="
ls -ln /dev/dri/ 2>&1
echo "== open+ioctl =="
python3 - <<'PY'
import os, ctypes, fcntl
p = '/dev/dri/renderD128'
fd = os.open(p, os.O_RDWR | os.O_CLOEXEC)
print('open O_RDWR OK')

class V(ctypes.Structure):
    _fields_ = [('major', ctypes.c_int), ('minor', ctypes.c_int), ('patch', ctypes.c_int),
                ('name_len', ctypes.c_size_t), ('name', ctypes.c_char_p),
                ('date_len', ctypes.c_size_t), ('date', ctypes.c_char_p),
                ('desc_len', ctypes.c_size_t), ('desc', ctypes.c_char_p)]

n = ctypes.create_string_buffer(256)
dt = ctypes.create_string_buffer(256)
de = ctypes.create_string_buffer(512)
v = V(0, 0, 0, 256, ctypes.cast(n, ctypes.c_char_p), 256, ctypes.cast(dt, ctypes.c_char_p), 512, ctypes.cast(de, ctypes.c_char_p))
fcntl.ioctl(fd, 0xc0406400, v)
print('DRM version:', v.major, v.minor, v.patch)
print('driver name:', n.value.decode())
print('driver date:', dt.value.decode())
print('driver desc:', de.value.decode())

class U(ctypes.Structure):
    _fields_ = [('len', ctypes.c_size_t), ('ptr', ctypes.c_char_p)]
u = ctypes.create_string_buffer(256)
uu = U(256, ctypes.cast(u, ctypes.c_char_p))
try:
    fcntl.ioctl(fd, 0x80106401, uu)
    print('unique:', u.value.decode())
except OSError as e:
    print('GET_UNIQUE failed:', e)
PY
echo "== done =="
