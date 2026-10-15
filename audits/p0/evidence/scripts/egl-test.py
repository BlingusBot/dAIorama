#!/usr/bin/env python3
"""EGL/GBM probe v2: open render node via GBM, create GLES2 context, print GL renderer."""
import ctypes, os
from ctypes import c_int, c_uint, c_void_p, c_char_p, POINTER, byref, CFUNCTYPE

EGL_NONE = 0x3038
EGL_PLATFORM_GBM_KHR = 0x31D7
EGL_VENDOR = 0x3053
EGL_VERSION = 0x3054
EGL_CLIENT_APIS = 0x308D
EGL_EXTENSIONS = 0x3055
EGL_SURFACE_TYPE = 0x3033
EGL_PBUFFER_BIT = 0x0001
EGL_RENDERABLE_TYPE = 0x3040
EGL_OPENGL_ES2_BIT = 0x0004
EGL_OPENGL_ES_API = 0x30A0
EGL_WIDTH = 0x3057
EGL_HEIGHT = 0x3056
EGL_CONTEXT_CLIENT_VERSION = 0x3098

gbm = ctypes.CDLL("libgbm.so.1")
gbm.gbm_create_device.restype = c_void_p
gbm.gbm_create_device.argtypes = [c_int]

egl = ctypes.CDLL("libEGL.so.1")
egl.eglGetProcAddress.restype = c_void_p
egl.eglGetProcAddress.argtypes = [c_char_p]
egl.eglInitialize.restype = c_uint
egl.eglInitialize.argtypes = [c_void_p, POINTER(c_int), POINTER(c_int)]
egl.eglQueryString.restype = c_char_p
egl.eglQueryString.argtypes = [c_void_p, c_int]
egl.eglGetError.restype = c_uint
egl.eglBindAPI.restype = c_uint
egl.eglBindAPI.argtypes = [c_uint]
egl.eglChooseConfig.restype = c_uint
egl.eglChooseConfig.argtypes = [c_void_p, POINTER(c_int), POINTER(c_void_p), c_int, POINTER(c_int)]
egl.eglCreatePbufferSurface.restype = c_void_p
egl.eglCreatePbufferSurface.argtypes = [c_void_p, c_void_p, POINTER(c_int)]
egl.eglCreateContext.restype = c_void_p
egl.eglCreateContext.argtypes = [c_void_p, c_void_p, c_void_p, POINTER(c_int)]
egl.eglMakeCurrent.restype = c_uint
egl.eglMakeCurrent.argtypes = [c_void_p, c_void_p, c_void_p, c_void_p]

display_proto = CFUNCTYPE(c_void_p, c_uint, c_void_p, c_void_p)


def get_platform_display(dev):
    for name in (b"eglGetPlatformDisplayEXT", b"eglGetPlatformDisplay"):
        addr = egl.eglGetProcAddress(name)
        print(f"  proc {name.decode()}: {'0x%x' % addr if addr else 'missing'}")
        if addr:
            fn = ctypes.cast(addr, POINTER(display_proto)).contents
            d = fn(EGL_PLATFORM_GBM_KHR, c_void_p(dev), None)
            if d:
                return d
    return None


fd = os.open("/dev/dri/renderD128", os.O_RDWR | os.O_CLOEXEC)
dev = gbm.gbm_create_device(fd)
print("gbm_create_device:", bool(dev))
disp = get_platform_display(dev)
print("display:", bool(disp), "| err:", hex(egl.eglGetError()))
if not disp:
    raise SystemExit("no EGL display")
maj, mnr = c_int(0), c_int(0)
ok = egl.eglInitialize(disp, byref(maj), byref(mnr))
print("eglInitialize:", bool(ok), "| version", maj.value, mnr.value, "| err:", hex(egl.eglGetError()))
print("  EGL_VENDOR:", egl.eglQueryString(disp, EGL_VENDOR))
print("  EGL_VERSION:", egl.eglQueryString(disp, EGL_VERSION))
print("  EGL_CLIENT_APIS:", egl.eglQueryString(disp, EGL_CLIENT_APIS))

ok = egl.eglBindAPI(EGL_OPENGL_ES_API)
print("eglBindAPI(ES):", bool(ok), "| err:", hex(egl.eglGetError()))
attribs = (c_int * 5)(EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE)
cfg = c_void_p(0)
num = c_int(0)
ok = egl.eglChooseConfig(disp, attribs, byref(cfg), 1, byref(num))
print("eglChooseConfig:", bool(ok), "| configs:", num.value, "| err:", hex(egl.eglGetError()))
surf_attribs = (c_int * 5)(EGL_WIDTH, 16, EGL_HEIGHT, 16, EGL_NONE)
surf = egl.eglCreatePbufferSurface(disp, cfg, surf_attribs)
print("pbuffer surface:", bool(surf), "| err:", hex(egl.eglGetError()))
ctx_attribs = (c_int * 3)(EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE)
ctx = egl.eglCreateContext(disp, cfg, None, ctx_attribs)
print("context:", bool(ctx), "| err:", hex(egl.eglGetError()))
ok = egl.eglMakeCurrent(disp, surf, surf, ctx)
print("makeCurrent:", bool(ok), "| err:", hex(egl.eglGetError()))

gl = ctypes.CDLL("libGLESv2.so.2")
gl.glGetString.restype = c_char_p
gl.glGetString.argtypes = [c_uint]
for name, enum in (("GL_VENDOR", 0x1F00), ("GL_RENDERER", 0x1F01), ("GL_VERSION", 0x1F02)):
    v = gl.glGetString(enum)
    print(f"  {name}:", v.decode() if v else None)
