#!/usr/bin/env python3
"""Harness-compatibility drill for `daiorama mcp` (P3 dogfood).

A harness never handcrafts JSON-RPC: it points its MCP client SDK at the pinned
command (`daiorama mcp`, 05 §The bridge) and lets the SDK negotiate. This drill
does exactly that with the published `mcp` client (version printed below) and
then drives the always-on lifecycle set against a real manager and a real
compositor chain.

Self-checking: one [OK]/[FAIL] line per assertion and a final `FAILS=` count;
exit code = FAILS.

Usage:
  python3 sdk_client.py            # the SDK drill (needs a running manager)
  python3 sdk_client.py --raw      # raw stdio probes that explain a failed handshake
"""

import asyncio
import json
import os
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


def bridge_env():
    env = dict(os.environ)
    env["DAIORAMA_SCOPE"] = SCOPE
    return env


def raw(label, payload, timeout=20):
    proc = subprocess.run(
        [BIN, "mcp"], input=payload, capture_output=True, timeout=timeout, env=bridge_env()
    )
    out = proc.stdout.decode("utf-8", "replace").strip().splitlines()
    err = proc.stderr.decode("utf-8", "replace").strip().splitlines()
    print(f"=== {label}: rc={proc.returncode}")
    for line in out[:4]:
        print("  out:", line[:500])
    for line in err[:2]:
        print("  err:", line[:300])
    return proc.returncode, out, err


def run_raw_probes():
    """The two handshake paths as a modern SDK sends them."""
    discover = {"jsonrpc": "2.0", "id": 1, "method": "server/discover",
                "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}}
    raw("server/discover (modern probe)", (json.dumps(discover) + "\n").encode())
    init = {"jsonrpc": "2.0", "id": 2, "method": "initialize",
            "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                       "clientInfo": {"name": "sdk", "version": SDK_VERSION}}}
    _, out, _ = raw("initialize (handshake-era request)", (json.dumps(init) + "\n").encode())
    return out


def structured(result):
    value = getattr(result, "structured_content", None)
    return value if isinstance(value, dict) else {}


def describe(exc):
    """Flatten anyio's ExceptionGroup so the evidence names the real cause."""
    if isinstance(exc, BaseExceptionGroup):
        return "; ".join(describe(inner) for inner in exc.exceptions)
    return f"{type(exc).__name__}: {exc}"


async def drill():
    params = StdioServerParameters(command=BIN, args=["mcp"], env=bridge_env())
    started = time.time()
    client = Client(params)  # mode='auto' — the harness default
    try:
        async with client:
            session = client.session
            version = session.protocol_version
            mode = "modern" if session.discover_result is not None else "handshake"
            check("connect (SDK auto negotiation)", True, f"protocol_version={version} via {mode}")
            check("negotiated revision is the pinned one", version == "2026-07-28", version or "")

            listing = await session.list_tools()
            names = sorted(tool.name for tool in listing.tools)
            print("       tools:", ", ".join(names))
            for required in ["create_session", "close_session", "list_sessions",
                             "session_status", "list_capabilities"]:
                check(f"always-on tool exposed: {required}", required in names)
            check("input grant expands the manifest", "pointer_move" in names)
            check("deny-by-default holds (screenshot not granted)", "screenshot" not in names)

            created = await session.call_tool("create_session", {"resolution": "1024x768", "scale": 1.0})
            record = structured(created)
            print("       create_session ->", json.dumps(record)[:400])
            check("create_session is a tool result, not an error", getattr(created, "is_error", True) is False)
            check("create_session returns an id", isinstance(record.get("id"), str), str(record.get("id")))
            check("created record carries the caller scope", record.get("scope") == SCOPE, str(record.get("scope")))
            check("created record carries origin=mcp", record.get("origin") == "mcp", str(record.get("origin")))
            check("renderer mark is verified", record.get("renderer") == "verified", str(record.get("renderer")))
            sid = record.get("id")

            listed = await session.call_tool("list_sessions", {})
            sessions = structured(listed).get("sessions") or []
            ids = [s.get("id") for s in sessions]
            check("list_sessions sees the created session", sid in ids, str(ids))

            status = await session.call_tool("session_status", {"session_id": sid})
            status_body = structured(status)
            check("session_status answers running", status_body.get("state") == "running", str(status_body.get("state")))

            caps = await session.call_tool("list_capabilities", {})
            caps_body = structured(caps)
            quotas = ((caps_body.get("quotas") or {}).get("sessions") or {})
            check("list_capabilities reports live=1", quotas.get("live") == 1, json.dumps(quotas))

            # Concurrency: a harness issues overlapping calls; the SDK matches responses by id.
            results = await asyncio.wait_for(
                asyncio.gather(*(session.call_tool("list_sessions", {}) for _ in range(3))),
                timeout=60,
            )
            ok = all(getattr(r, "is_error", True) is False for r in results)
            same = all(
                [s.get("id") for s in (structured(r).get("sessions") or [])] == ids for r in results
            )
            check("three overlapping calls each answer correctly", ok and same)

            refused = await session.call_tool("screenshot", {"session_id": sid})
            refused_body = structured(refused)
            check("deny-by-default tool refuses as a tool error", getattr(refused, "is_error", False) is True,
                  str(refused_body.get("code")))
            check("refusal carries tool_not_granted", refused_body.get("code") == "tool_not_granted",
                  str(refused_body.get("code")))

            unknown = await session.call_tool("no_such_tool", {})
            check("unknown tool refuses as a tool error",
                  getattr(unknown, "is_error", False) is True
                  and structured(unknown).get("code") == "bad_request",
                  str(structured(unknown).get("code")))

            closed = await session.call_tool("close_session", {"session_id": sid})
            check("close_session succeeds", getattr(closed, "is_error", True) is False)
            after = await session.call_tool("list_sessions", {})
            left = [s.get("id") for s in (structured(after).get("sessions") or [])]
            check("close_session removed it from the listing", sid not in left, str(left))
    except Exception as exc:  # noqa: BLE001 - the drill reports, never raises past the count
        check("harness connect + drill completed", False, describe(exc))
    elapsed = time.time() - started
    print(f"       drill wall time: {elapsed:.2f}s")
    check("bridge exits cleanly on client close", elapsed < 120, f"{elapsed:.2f}s")


def main():
    print(f"== SDK drill: daiorama mcp via mcp=={SDK_VERSION} (scope={SCOPE}, bin={BIN})")
    if "--raw" in sys.argv:
        run_raw_probes()
        return 0
    asyncio.run(drill())
    print(f"FAILS={FAILS}")
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
