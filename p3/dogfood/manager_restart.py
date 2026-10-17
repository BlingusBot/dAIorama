#!/usr/bin/env python3
"""P3 dogfood: a harness that outlives its manager (crash / restart).

A harness keeps ONE MCP session open — it connects to the pinned entry point
(`daiorama mcp`) once and then issues calls for the life of the run. A manager
crash-and-restart (operator upgrade, crash recovery) happens underneath it.

This drill asks the question the conformance drills cannot: after the manager
process is SIGKILLed and a new one comes up on the same socket, does the open
harness session keep working? The first post-restart call is the interesting
one — a bridge that cached the dead link and only evicts it on a failure the
caller happens to make first would leave one surface (the capability poll a
harness does every turn) permanently dead.

Self-checking: one [OK]/[FAIL] per assertion, final `FAILS=`; exit code = FAILS.
"""

import asyncio
import json
import os
import re
import signal
import subprocess
import sys
import time

import mcp
from mcp import Client
from mcp.client.stdio import StdioServerParameters

try:
    import importlib.metadata as metadata

    SDK_VERSION = metadata.version("mcp")
except Exception:  # pragma: no cover - diagnostics only
    SDK_VERSION = "unknown"

BIN = os.environ.get("DAIORAMA_BIN", "target/debug/daiorama")
SCOPE = os.environ.get("DAIORAMA_SCOPE", "agent")

FAILS = 0


def check(label, ok, detail=""):
    global FAILS
    if ok:
        print(f"[OK]   {label}" + (f" :: {detail}" if detail else ""))
    else:
        print(f"[FAIL] {label}" + (f" :: {detail}" if detail else ""))
        FAILS += 1
    return ok


def structured(result):
    value = getattr(result, "structured_content", None)
    return value if isinstance(value, dict) else {}


def describe(exc):
    if isinstance(exc, BaseExceptionGroup):
        return "; ".join(describe(inner) for inner in exc.exceptions)
    return f"{type(exc).__name__}: {exc}"


def manager_start():
    """`daemon start`; returns (pid, raw line)."""
    proc = subprocess.run([BIN, "daemon", "start"], capture_output=True, text=True, timeout=120)
    raw = (proc.stdout + proc.stderr).strip()
    match = re.search(r"pid=(\d+)", raw)
    return (int(match.group(1)) if match else None), raw, proc.returncode


def manager_wait(deadline_s=30):
    """Wait until `status` answers (a manager is serving on the socket)."""
    end = time.time() + deadline_s
    last = ""
    while time.time() < end:
        proc = subprocess.run([BIN, "status"], capture_output=True, text=True, timeout=10)
        if proc.returncode == 0:
            return True, proc.stdout.strip()
        last = (proc.stdout + proc.stderr).strip()
        time.sleep(0.2)
    return False, last


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def compositors():
    """mutter/Hyprland processes visible in this (privileged) container."""
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/comm") as handle:
                comm = handle.read().strip()
        except OSError:
            continue
        if comm in ("mutter", "Hyprland"):
            found.append((int(entry), comm))
    return sorted(found)


async def drill():
    started_pid, raw, rc = manager_start()
    check("manager start (cold)", rc == 0 and started_pid is not None, raw.splitlines()[-1] if raw else "")
    if started_pid is None:
        return
    check("manager process is alive", alive(started_pid), f"pid={started_pid}")

    params = StdioServerParameters(command=BIN, args=["mcp"], env=dict(os.environ, DAIORAMA_SCOPE=SCOPE))
    client = Client(params)  # mode='auto' — the harness default
    try:
        async with client:
            session = client.session
            check("harness connected", True, f"protocol_version={session.protocol_version}")

            created = await session.call_tool("create_session", {"resolution": "1024x768", "scale": 1.0})
            record = structured(created)
            sid = record.get("id")
            check("session created under the old manager", isinstance(sid, str), str(sid))
            check("renderer mark is verified with a hardware string",
                  isinstance(record.get("renderer"), str) and record.get("renderer") != "software",
                  str(record.get("renderer")))
            print("       create_session ->", json.dumps(record)[:300])
            check("compositors running before the crash", len(compositors()) > 0, str(compositors()))

            # --- the manager dies under the open harness session -------------------
            os.kill(started_pid, signal.SIGKILL)
            end = time.time() + 10
            while time.time() < end and alive(started_pid):
                time.sleep(0.1)
            check("manager gone after SIGKILL", not alive(started_pid), f"pid={started_pid}")
            survivors = compositors()
            print("       compositors surviving the crash:", survivors)
            check("a compositor survived the crash (PDEATHSIG gap, sweep's job)", len(survivors) > 0,
                  str(len(survivors)))

            # --- a new manager comes up on the same socket --------------------------
            new_pid, raw, rc = manager_start()
            check("manager restart", rc == 0 and new_pid is not None and new_pid != started_pid,
                  (raw.splitlines()[-1] if raw else "") + f" old={started_pid} new={new_pid}")
            ready, detail = manager_wait()
            check("restarted manager answers status", ready, detail)

            # The startup sweep reaps the crashed sessions' scopes (cgroup.kill).
            end = time.time() + 15
            while time.time() < end and compositors():
                time.sleep(0.2)
            left = compositors()
            check("startup sweep reaped the surviving compositor", not left, str(left))

            # --- THE assertion: the open harness session keeps working --------------
            caps = await session.call_tool("list_capabilities", {})
            caps_body = structured(caps)
            ok = getattr(caps, "is_error", True) is False
            check("first post-restart call (list_capabilities) answers", ok,
                  str(caps_body.get("code")) if not ok else "ok")
            if ok:
                quotas = (caps_body.get("quotas") or {}).get("sessions") or {}
                check("live count is zero after the restart", quotas.get("live") == 0, json.dumps(quotas))

            listed = await session.call_tool("list_sessions", {})
            listed_body = structured(listed)
            check("list_sessions answers after the restart", getattr(listed, "is_error", True) is False,
                  str(listed_body.get("code")) if getattr(listed, "is_error", True) else "ok")
            check("the crashed session is not resurrected",
                  [s.get("id") for s in (listed_body.get("sessions") or [])] == [],
                  str([s.get("id") for s in (listed_body.get("sessions") or [])]))

            again = await session.call_tool("create_session", {"resolution": "800x600", "scale": 1.0})
            again_record = structured(again)
            new_sid = again_record.get("id")
            check("create_session works after the restart", getattr(again, "is_error", True) is False,
                  str(again_record.get("code")) if getattr(again, "is_error", True) else str(new_sid))
            if isinstance(new_sid, str):
                closed = await session.call_tool("close_session", {"session_id": new_sid})
                check("close_session works after the restart", getattr(closed, "is_error", True) is False)
    except Exception as exc:  # noqa: BLE001 - the drill reports, never raises past the count
        check("harness session completed", False, describe(exc))
    finally:
        subprocess.run([BIN, "daemon", "end", "--force"], capture_output=True, text=True, timeout=60)


def main():
    print(f"== manager-restart drill: daiorama mcp via mcp=={SDK_VERSION} (scope={SCOPE}, bin={BIN})")
    started = time.time()
    asyncio.run(drill())
    print(f"       drill wall time: {time.time() - started:.2f}s")
    print(f"FAILS={FAILS}")
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
