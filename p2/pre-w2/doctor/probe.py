#!/usr/bin/env python3
"""P2 doctor host-probe (prototype) — emits a JSON report for W2/W3 doctor wiring.

Checks: tool presence + versions (Hyprland range), GPU render node + GL renderer
(surfaceless EGL), containment class (cgroup v2 writability/delegation), session
prerequisites. Runs inside a session-capable container (dai-l1 image) or on a host.
"""
import ctypes, json, os, shutil, subprocess, sys, time

def run(cmd, env=None):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=20, env=env)
        return (r.stdout + r.stderr).strip()
    except Exception as e:
        return f"error: {e}"

def tool_versions():
    tools = {}
    for t in ["Hyprland", "mutter", "wayvnc", "websockify", "wtype", "wlrctl", "grim",
              "foot", "chromium", "dbus-run-session"]:
        tools[t] = shutil.which(t) or "missing"
    vers = {}
    vers["hyprland"] = run(["Hyprland", "--version"])[:200]
    vers["mutter"] = run(["mutter", "--version"])[:200]
    vers["chromium"] = run(["chromium", "--version"])[:200]
    vers["wayvnc"] = run(["wayvnc", "--version"])[:200]
    return {"paths": tools, "versions": vers}

def gpu_report():
    out = {"render_node": os.path.exists("/dev/dri/renderD128")}
    try:
        egl = ctypes.CDLL("libEGL.so.1")
        egl.eglGetDisplay.restype = ctypes.c_void_p
        d = egl.eglGetDisplay(ctypes.c_void_p(0))  # EGL_DEFAULT_DISPLAY -> surfaceless via env
        major, minor = ctypes.c_int(), ctypes.c_int()
        ok = egl.eglInitialize(ctypes.c_void_p(d), ctypes.byref(major), ctypes.byref(minor))
        out["egl_init"] = bool(ok)
        out["egl_version"] = f"{major.value}.{minor.value}"
        # full context before glGetString (a null context returns NULL, not a string)
        egl.eglBindAPI.argtypes = [ctypes.c_uint]
        egl.eglBindAPI(0x30A0)  # EGL_OPENGL_ES_API
        egl.eglChooseConfig.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int), ctypes.c_void_p, ctypes.c_int, ctypes.POINTER(ctypes.c_int)]
        attribs = (ctypes.c_int * 5)(0x3033, 0x0001, 0x3040, 0x0004, 0x3038)  # SURFACE_TYPE=PBUFFER, RENDERABLE=ES2, NONE
        cfg = ctypes.c_void_p(0)
        num = ctypes.c_int(0)
        egl.eglChooseConfig(ctypes.c_void_p(d), attribs, ctypes.byref(cfg), 1, ctypes.byref(num))
        surf_attribs = (ctypes.c_int * 5)(0x3057, 16, 0x3056, 16, 0x3038)
        egl.eglCreatePbufferSurface.restype = ctypes.c_void_p
        egl.eglCreatePbufferSurface.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
        surf = egl.eglCreatePbufferSurface(ctypes.c_void_p(d), cfg, surf_attribs)
        ctx_attribs = (ctypes.c_int * 3)(0x3098, 2, 0x3038)
        egl.eglCreateContext.restype = ctypes.c_void_p
        egl.eglCreateContext.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
        ctx = egl.eglCreateContext(ctypes.c_void_p(d), cfg, None, ctx_attribs)
        egl.eglMakeCurrent.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
        egl.eglMakeCurrent(ctypes.c_void_p(d), ctypes.c_void_p(surf), ctypes.c_void_p(surf), ctypes.c_void_p(ctx))
        gl = ctypes.CDLL("libGLESv2.so.2")
        gl.glGetString.restype = ctypes.c_char_p
        for name, enum in (("gl_renderer", 0x1F01), ("gl_vendor", 0x1F00)):
            v = gl.glGetString(enum)
            out[name] = v.decode() if v else None
        out["is_llvmpipe"] = bool(out.get("gl_renderer") and "llvmpipe" in out["gl_renderer"].lower())
    except Exception as e:
        out["egl_error"] = str(e)
    return out

def containment_report():
    rep = {
        "subtree_control": run(["cat", "/sys/fs/cgroup/cgroup.subtree_control"]),
        "controllers": run(["cat", "/sys/fs/cgroup/cgroup.controllers"]),
    }
    probe = "/sys/fs/cgroup/dai-doctor-probe"
    try:
        os.mkdir(probe)
        rep["writable_cgroupfs"] = True
        ctl = run(["cat", os.path.join(probe, "cgroup.controllers")])
        rep["probe_controllers"] = ctl
        rep["class"] = "on containerized"
        try:
            os.rmdir(probe)
        except OSError:
            pass
    except OSError as e:
        rep["writable_cgroupfs"] = False
        rep["class"] = "off-mode class (no writable cgroupfs)"
        rep["mkdir_error"] = str(e)
    return rep

def main():
    rep = {
        "collected_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "uid": os.getuid(),
        "uname": os.uname().release,
        "tools": tool_versions(),
        "gpu": gpu_report(),
        "containment": containment_report(),
        "runtime_dir": os.environ.get("XDG_RUNTIME_DIR", "(unset)"),
    }
    print(json.dumps(rep, indent=2, sort_keys=True))

if __name__ == "__main__":
    main()
