#!/usr/bin/env python3
"""`mcp --listen` dogfood: a real MCP client SDK against the network bridge.

Every earlier network probe speaks raw HTTP/1.1 by hand. A harness does not: it hands the
endpoint to its MCP client SDK and lets the SDK negotiate (`server/discover` first on
`mode='auto'`, then the initialize handshake), validate every result, and match responses by id.
This drill points the published Python client (`mcp==2.3.0`) at a live `daiorama mcp --listen`,
drives the calls a harness would, and then checks the one thing a raw-socket probe cannot see:
whether a credential refusal actually reaches that client.

Self-checking: one [OK]/[FAIL] line per assertion and a final `FAILS=`; exit code = FAILS.

Run: mcpvenv/bin/python p3/listen/evidence/probe-sdk-http.py [--bin PATH] [--port 18780]
Needs: the `mcp` client SDK (2.3.0) importable by the interpreter running this script.
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import tempfile
import time

import anyio

from mcp import Client
from mcp.client.streamable_http import create_mcp_http_client, streamable_http_client

FAILS = 0
SCOPE = "operator"
# A refusal a client cannot see is indistinguishable from a dead server: a real client matches
# responses by id, so anything past this bound means it waited on its own read timeout instead.
REFUSAL_BUDGET = 2.0


def inner_messages(err, out=None):
    """Flatten an ExceptionGroup to its leaf messages (the SDK's error shape)."""
    out = out if out is not None else []
    out.append(str(err))
    for sub in getattr(err, "exceptions", []) or []:
        inner_messages(sub, out)
    return out


def check(label, ok, detail=""):
    global FAILS
    if ok:
        print(f"[OK]   {label}" + (f" :: {detail}" if detail else ""))
    else:
        print(f"[FAIL] {label}" + (f" :: {detail}" if detail else ""))
        FAILS += 1
    return ok


def free_port(preferred):
    s = socket.socket()
    try:
        s.bind(("127.0.0.1", preferred))
        return preferred
    except OSError:
        s.close()
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        return port
    finally:
        try:
            s.close()
        except OSError:
            pass


class Env:
    def __init__(self, bin_path, port):
        self.bin = bin_path
        self.port = port
        self.root = tempfile.mkdtemp(prefix="dlsdk.")
        self.env = dict(os.environ)
        self.env.update(
            HOME=self.root,
            XDG_CONFIG_HOME=os.path.join(self.root, "config"),
            XDG_DATA_HOME=os.path.join(self.root, "data"),
            XDG_RUNTIME_DIR=os.path.join(self.root, "run"),
            TMPDIR=os.path.join(self.root, "tmp"),
        )
        for key in ("DAIORAMA_TOKEN", "DAIORAMA_SCOPE", "DAIORAMA_SCOPE_TOKEN"):
            self.env.pop(key, None)
        for path in ("config", "data", "run", "tmp"):
            os.makedirs(os.path.join(self.root, path), exist_ok=True)
        self.manager = None
        self.bridge = None
        self.manager_log = open(os.path.join(self.root, "manager.log"), "wb")
        self.bridge_log = open(os.path.join(self.root, "bridge.log"), "wb")

    def run(self, *args, timeout=30):
        return subprocess.run(
            [self.bin, *args], env=self.env, capture_output=True, text=True, timeout=timeout
        )

    def start(self):
        setup = self.run("config", "setup")
        assert setup.returncode == 0, setup.stderr
        mode = self.run("config", "set", "limits.mode", "off")
        assert mode.returncode == 0, mode.stderr
        scopes_dir = os.path.join(self.root, "config", "daiorama", "default", "scopes")
        os.makedirs(scopes_dir, exist_ok=True)
        for name in ("agent1", "agent2"):
            with open(os.path.join(scopes_dir, f"{name}.ron"), "w") as fh:
                fh.write('(sessions: (max: Some(2), visibility: own),\n grant: (categories: ["input"]))\n')

        self.manager = subprocess.Popen(
            [self.bin, "serve"], env=self.env, stdout=self.manager_log, stderr=subprocess.STDOUT
        )
        sock = os.path.join(self.root, "run", "daiorama", "default", "manager.sock")
        for _ in range(100):
            if os.path.exists(sock):
                break
            time.sleep(0.1)
        status = self.run("status", "--json")
        assert status.returncode == 0, f"manager did not come up: {status.stderr}"

        self.bridge = subprocess.Popen(
            [self.bin, "mcp", "--listen", f"127.0.0.1:{self.port}"],
            env=self.env,
            stdout=self.bridge_log,
            stderr=subprocess.STDOUT,
        )
        for _ in range(100):
            with open(os.path.join(self.root, "bridge.log"), "rb") as fh:
                if b"listening on" in fh.read():
                    break
            time.sleep(0.1)

        self.token = self._read("data/daiorama/default/token")
        self.secret = self._read(f"data/daiorama/default/scope-tokens/{SCOPE}")

    def _read(self, rel):
        with open(os.path.join(self.root, rel)) as fh:
            return fh.read().strip()

    def headers(self, token=None, scope=SCOPE, secret=None):
        if secret is None:
            secret = self._read(f"data/daiorama/default/scope-tokens/{scope}")
        return {
            "Daiorama-Token": self.token if token is None else token,
            "Daiorama-Scope": scope,
            "Daiorama-Scope-Token": secret,
        }

    def raw(self, path, headers, body):
        """One raw HTTP exchange, for the shape appendix."""
        conn = socket.create_connection(("127.0.0.1", self.port), timeout=5)
        lines = [f"POST {path} HTTP/1.1"] + [f"{k}: {v}" for k, v in headers.items()]
        lines += [f"Content-Length: {len(body)}", "Connection: close", "", ""]
        conn.sendall("\r\n".join(lines).encode() + body)
        data = b""
        while True:
            chunk = conn.recv(65536)
            if not chunk:
                break
            data += chunk
        conn.close()
        return data.decode("latin-1", "replace")

    def stop(self):
        for proc in (self.bridge, self.manager):
            if proc and proc.poll() is None:
                proc.kill()
                proc.wait(timeout=10)
        self.manager_log.close()
        self.bridge_log.close()


def transport(url, headers):
    return streamable_http_client(url, http_client=create_mcp_http_client(headers=headers))


async def drill_authenticated(url, env):
    """The harness path: one SDK client, discover/initialize, list tools, a relayed call."""
    async with Client(transport(url, env.headers()), mode="auto") as client:
        check(
            "SDK connected (discover/initialize negotiated)",
            True,
            f"protocol={client.protocol_version!r} server={getattr(client.server_info, 'name', None)!r}",
        )
        tools = await client.list_tools()
        names = sorted(t.name for t in tools.tools)
        check("tools/list validates", bool(names), f"{len(names)} tools: {', '.join(names[:6])}…")
        check(
            "list_sessions is advertised",
            "list_sessions" in names,
            "present" if "list_sessions" in names else f"missing from {names}",
        )

        result = await client.call_tool("list_sessions", {})
        structured = result.structured_content or {}
        check(
            "tools/call list_sessions relays (not is_error)",
            not result.is_error,
            json.dumps(structured)[:160],
        )
        sessions = structured.get("sessions", structured.get("result", structured))
        check(
            "list_sessions answer is a list-shaped payload",
            isinstance(sessions, list),
            f"{type(sessions).__name__}: {str(sessions)[:120]}",
        )

        caps = await client.call_tool("list_capabilities", {})
        check(
            "tools/call list_capabilities relays (not is_error)",
            not caps.is_error,
            json.dumps(caps.structured_content or {})[:160],
        )



async def drill_two_scopes(url, env, scopes):
    """Two SDK clients on one bridge: each request must carry its own identity."""
    results = {}
    async with Client(transport(url, env.headers(scope=scopes[0])), mode="auto") as first:
        async with Client(transport(url, env.headers(scope=scopes[1])), mode="auto") as second:
            a = await first.call_tool("list_sessions", {})
            b = await second.call_tool("list_sessions", {})
            results["a"] = a
            results["b"] = b
    ok = not results["a"].is_error and not results["b"].is_error
    check(
        "two concurrent SDK clients on one bridge both answer",
        ok,
        f"{json.dumps(results['a'].structured_content or {})[:80]} / "
        f"{json.dumps(results['b'].structured_content or {})[:80]}",
    )


async def drill_refusal_visibility(url, env):
    """A credential refusal must reach the client: fast, and carrying its code text."""
    cases = {
        "no token": ({"Daiorama-Scope": SCOPE, "Daiorama-Scope-Token": env.secret},
                     ("token required",)),
        "bad token": ({**env.headers(), "Daiorama-Token": "not-a-real-token"},
                      ("token required",)),
        "bad scope secret": ({**env.headers(), "Daiorama-Scope-Token": "nope"},
                             ("requires its scope secret",)),
        # Control: a 403 refusal (origin) of the same envelope family, with good credentials.
        # It answers a non-200 status, so the client reads the body instead of waiting for an id.
        "control: origin mismatch (403)": ({**env.headers(), "Origin": "http://evil.example"},
                                           ("origin not allowed",)),
    }
    for label, (headers, expected) in cases.items():
        started = time.time()
        seen = "connected anyway"
        try:
            async with Client(
                transport(url, headers), mode="auto", read_timeout_seconds=5.0
            ) as client:
                await client.list_tools()
        except BaseException as err:  # noqa: BLE001 - the SDK's error shape is the payload here
            seen = " | ".join(m.strip() for m in inner_messages(err) if m.strip())[-300:]
        elapsed = time.time() - started
        visible = elapsed < REFUSAL_BUDGET and any(code in seen for code in expected)
        check(
            f"refusal ({label}) reaches the client with its code",
            visible,
            f"{elapsed:.1f}s: {seen}",
        )


def drill_idless_statuses(env):
    """One raw leg per id-less status: 403 gate, 400 parse, 413 declared size."""
    host = f"127.0.0.1:{env.port}"
    good = {"Host": host, **env.headers()}
    cases = [
        ("gate refusal answers 403", {"Host": host, "Content-Type": "application/json"},
         b'{"jsonrpc":"2.0","id":7,"method":"server/discover"}', "403", "token_required"),
        ("parse error answers 400", {**good, "Content-Type": "application/json"},
         b"{not json", "400", "parse error"),
    ]
    print("\n--- id-less refusals: one leg per status ---")
    for label, headers, body, status, needle in cases:
        raw = env.raw("/mcp", headers, body)
        head, _, payload = raw.partition("\r\n\r\n")
        ok = (
            head.startswith(f"HTTP/1.1 {status}")
            and needle in payload
            and "connection: close" in head.lower()
        )
        check(label, ok, head.splitlines()[0] if head else "?")
    # Declared-size leg: env.raw() derives Content-Length from the body, so this head is
    # written by hand (a declared body past the cap, none of it sent).
    sock = socket.create_connection(("127.0.0.1", env.port), timeout=5.0)
    head_lines = f"Host: {host}\r\n" + "".join(
        f"{name}: {value}\r\n" for name, value in env.headers().items()
    )
    sock.sendall(
        (
            f"POST /mcp HTTP/1.1\r\n{head_lines}"
            f"Content-Length: {8 * 1024 * 1024 + 1}\r\nConnection: close\r\n\r\n"
        ).encode()
    )
    data = b""
    while True:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    sock.close()
    head, _, payload = data.partition(b"\r\n\r\n")
    ok = (
        head.startswith(b"HTTP/1.1 413")
        and b"request frame must be at most" in payload
        and b"connection: close" in head.lower()
    )
    check("declared body over cap answers 413", ok, head.split(b"\r\n", 1)[0].decode("latin-1", "replace"))


async def main_async(args):
    env = Env(args.bin, args.port)
    url = f"http://127.0.0.1:{args.port}/mcp"
    try:
        env.start()
        print(f"bin: {args.bin}\nurl: {url}\nmanager pid: {env.manager.pid} bridge pid: {env.bridge.pid}")
        await drill_authenticated(url, env)

        await drill_two_scopes(url, env, ["agent1", "agent2"])

        body = b'{"jsonrpc":"2.0","id":7,"method":"server/discover"}'
        print("\n--- raw shape: gate refusal, no token ---")
        print(env.raw("/mcp", {"Host": f"127.0.0.1:{env.port}",
                               "Content-Type": "application/json"}, body))
        print("\n--- raw shape: wrong path (a non-200 refusal of the same family) ---")
        print(env.raw("/nope", {"Host": f"127.0.0.1:{env.port}", **env.headers(),
                                "Content-Type": "application/json"}, body))
        print()
        await drill_refusal_visibility(url, env)
        drill_idless_statuses(env)
    finally:
        bridge_log = os.path.join(env.root, "bridge.log")
        env.stop()
        with open(bridge_log, "rb") as fh:
            tail = fh.read().decode("utf-8", "replace").strip().splitlines()
        # This file is committed: no host paths in the transcript.
        tail = [line.replace(env.root, "<tmp>") for line in tail]
        print("\n--- bridge stderr (tail) ---")
        for line in tail[-8:]:
            print(line)
        # The bridge must not log request targets or credentials.
        blob = "\n".join(tail)
        check(
            "no token/secret material in bridge stderr",
            env.token not in blob and env.secret not in blob,
            "" if env.token not in blob and env.secret not in blob else "credential-shaped string found",
        )
    print(f"\nFAILS={FAILS}")
    return FAILS


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", default="target/debug/daiorama")
    parser.add_argument("--port", type=int, default=18780)
    args = parser.parse_args()
    args.port = free_port(args.port)
    sys.exit(anyio.run(main_async, args))


if __name__ == "__main__":
    main()
