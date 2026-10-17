#!/usr/bin/env python3
"""D1 live leg: the active dashboard module, read through a real `daiorama serve`.

`src/config/mod.rs:active_dashboard_module` is unit-tested in-process; this probe drives the
shipped path instead -- a real manager process, real frames over the unix socket, and the
`modules/daiorama-dashboard/active` link changed *while the manager is running* (D1: the
status/readiness read is a readlink per call, never cached, so an install or a revert needs no
restart).

Cases:
  [A] cold manager, no module directory: `status` and `readiness` both report null.
  [B] install while running: `2026.10.0` directory + `active` -> `2026.10.0`; both calls
      report `2026.10.0` with no restart.
  [C] switch to `2026.09.1`: both report the new version (no restart, no stale cache).
  [D] dangling link (version directory removed, link remains): null.
  [E] not a sibling: `../outside`, an absolute path, `.`, and a valid name that is a file all
      answer null -- never an error, never a client-supplied path.
  [F] a directory name that is not a version component (`bad name!`) answers null.

Usage: probe-dashboard-module.py <manager.sock> <data-root>
Prints [OK]/[FAIL] per assertion and FAILS=<n>; exit code = FAILS.
"""
import json
import os
import shutil
import socket
import sys

SOCK = sys.argv[1]
DATA = sys.argv[2]
PARENT = os.path.join(DATA, "modules", "daiorama-dashboard")
SCOPE = "operator"
PROTO = "0.1.0"

fails = 0


def ok(label, detail=""):
    print(f"[OK]   {label}" + (f" ({detail})" if detail else ""))


def fail(label, detail):
    global fails
    fails += 1
    print(f"[FAIL] {label} :: {detail}")


def note(label):
    print(f"[NOTE] {label}")


def check(label, cond, detail):
    if cond:
        ok(label, detail)
    else:
        fail(label, detail)


def connect():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(30)
    s.connect(SOCK)
    f = s.makefile("rwb")
    f.write(
        (
            json.dumps(
                {
                    "method": "handshake",
                    "params": {"protocol_version": PROTO, "min_supported": PROTO},
                }
            )
            + "\n"
        ).encode()
    )
    f.flush()
    reply = json.loads(f.readline())
    if "result" not in reply:
        raise RuntimeError(f"handshake refused: {reply}")
    return s, f


def call(f, method, params=None):
    frame = {"method": method, "params": params or {}, "scope": SCOPE}
    f.write((json.dumps(frame) + "\n").encode())
    f.flush()
    return json.loads(f.readline())


def module_of(f):
    """Return (module_value_or_sentinel, raw_reply) for both read calls."""
    values = {}
    for method in ("status", "readiness"):
        reply = call(f, method)
        if "result" not in reply:
            return ("__error__", reply)
        values[method] = reply["result"]["versions"].get("dashboard_module")
    check(
        "status and readiness agree",
        values["status"] == values["readiness"],
        f"status={values['status']!r} readiness={values['readiness']!r}",
    )
    return values["status"], values


def set_link(target):
    link = os.path.join(PARENT, "active")
    if os.path.islink(link) or os.path.exists(link):
        os.remove(link)
    os.symlink(target, link)


os.makedirs(PARENT, exist_ok=True)

s, f = connect()
try:
    # --------------------------------------------------------------- [A]
    value, raw = module_of(f)
    check("[A] no module installed: status/readiness report null", value is None, repr(value))

    # --------------------------------------------------------------- [B]
    os.makedirs(os.path.join(PARENT, "2026.10.0"), exist_ok=True)
    set_link("2026.10.0")
    value, raw = module_of(f)
    check("[B] install while running is visible with no restart", value == "2026.10.0", repr(value))

    # --------------------------------------------------------------- [C]
    os.makedirs(os.path.join(PARENT, "2026.09.1"), exist_ok=True)
    set_link("2026.09.1")
    value, raw = module_of(f)
    check("[C] a revert reports the other version (no cached link)", value == "2026.09.1", repr(value))

    # --------------------------------------------------------------- [D]
    shutil.rmtree(os.path.join(PARENT, "2026.09.1"))
    value, raw = module_of(f)
    check("[D] dangling link answers null, not an error", value is None, repr(value))

    # --------------------------------------------------------------- [E]
    outside = os.path.join(DATA, "outside")
    os.makedirs(outside, exist_ok=True)
    for target, label in [
        ("../outside", "a parent-climbing target"),
        (outside, "an absolute target outside the module directory"),
        (".", "the dot component"),
    ]:
        set_link(target)
        value, raw = module_of(f)
        check(f"[E] {label} answers null", value is None, repr(value))

    os.makedirs(os.path.join(PARENT, "not-a-dir"), exist_ok=True)
    set_link("not-a-dir")
    shutil.rmtree(os.path.join(PARENT, "not-a-dir"))
    with open(os.path.join(PARENT, "not-a-dir"), "w", encoding="utf-8") as handle:
        handle.write("x")
    value, raw = module_of(f)
    check("[E] a version name that is a file answers null", value is None, repr(value))

    # --------------------------------------------------------------- [F]
    os.makedirs(os.path.join(PARENT, "bad name!"), exist_ok=True)
    set_link("bad name!")
    value, raw = module_of(f)
    check("[F] a non-version directory name answers null", value is None, repr(value))

    # leave a working install behind, and prove the read still works after every refusal
    set_link("2026.10.0")
    value, raw = module_of(f)
    check("[G] a valid install answers again after the refusal cases", value == "2026.10.0", repr(value))
    note("the module link is under <data>/modules/daiorama-dashboard; status/readiness read it per call")
finally:
    s.close()

print(f"FAILS={fails}")
sys.exit(min(fails, 120))
