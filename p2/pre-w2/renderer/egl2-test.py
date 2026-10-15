#!/usr/bin/env python3
"""EGL probe v3: surfaceless platform via eglGetDisplay(EGL_DEFAULT_DISPLAY)."""
import ctypes, os
from ctypes import c_int, c_uint, c_void_p, c_char_p, POINTER, byref

EGL_NONE = 0x3038
EGL_VENDOR = 0x3053
EGL_VERSION = 0x3054
EGL_CLIENT_APIS = 0x308D
EGL_SURFACE_TYPE = 0x3033
EGL_PBUFFER_BIT = 0x0001
EGL_RENDERABLE_TYPE = 0x3040
EGL_OPENGL_ES2_BIT = 0x0004
EGL_OPENGL_ES_API = 0x30A0
EGL_WIDTH = 0x3057
EGL_HEIGHT = 0x3056
EGL_CONTEXT_CLIENT_VERSION = 0x3098

egl = ctypes.CDLL("libEGL.so.1")
egl.eglGetDisplay.restype = c_void_p
egl.eglGetDisplay.argtypes = [c_void_p]
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

print("EGL_PLATFORM env:", os.environ.get("EGL_PLATFORM"))
dpy = egl.eglGetDisplay(None)
print("eglGetDisplay(EGL_DEFAULT_DISPLAY):", hex(dpy) if dpy else None, "| err:", hex(egl.eglGetError()))
if not dpy:
    raise SystemExit("no display")
maj, mnr = c_int(0), c_int(0)
ok = egl.eglInitialize(dpy, byref(maj), byref(mnr))
print("eglInitialize:", bool(ok), "| version", maj.value, mnr.value, "| err:", hex(egl.eglGetError()))
print("  EGL_VENDOR:", egl.eglQueryString(dpy, EGL_VENDOR))
print("  EGL_VERSION:", egl.eglQueryString(dpy, EGL_VERSION))
print("  EGL_CLIENT_APIS:", egl.eglQueryString(dpy, EGL_CLIENT_APIS))

ok = egl.eglBindAPI(EGL_OPENGL_ES_API)
print("eglBindAPI(ES):", bool(ok), "| err:", hex(egl.eglGetError()))
attribs = (c_int * 5)(EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE)
cfg = c_void_p(0)
num = c_int(0)
ok = egl.eglChooseConfig(dpy, attribs, byref(cfg), 1, byref(num))
print("eglChooseConfig:", bool(ok), "| configs:", num.value, "| err:", hex(egl.eglGetError()))
surf_attribs = (c_int * 5)(EGL_WIDTH, 16, EGL_HEIGHT, 16, EGL_NONE)
surf = egl.eglCreatePbufferSurface(dpy, cfg, surf_attribs)
print("pbuffer surface:", bool(surf), "| err:", hex(egl.eglGetError()))
ctx_attribs = (c_int * 3)(EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE)
ctx = egl.eglCreateContext(dpy, cfg, None, ctx_attribs)
print("context:", bool(ctx), "| err:", hex(egl.eglGetError()))
ok = egl.eglMakeCurrent(dpy, surf, surf, ctx)
print("makeCurrent:", bool(ok), "| err:", hex(egl.eglGetError()))

gl = ctypes.CDLL("libGLESv2.so.2")
gl.glGetString.restype = c_char_p
gl.glGetString.argtypes = [c_uint]
for name, enum in (("GL_VENDOR", 0x1F00), ("GL_RENDERER", 0x1F01), ("GL_VERSION", 0x1F02)):
    v = gl.glGetString(enum)
    print(f"  {name}:", v.decode() if v else None)
