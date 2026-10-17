#!/usr/bin/env python3
"""D2 live leg: a scope created by `config scope create` is network-usable at the NEXT
network start (04, 07, 11 P5-D2), proven over a real TCP bridge and a real manager.

The slice's own tests (`tests/config_scope.rs`) drive the CLI cold: path-only answer, 0600,
the three templates, the refusals, no secret at create. This probe drives the claim the docs
make *after* create: "the next network start mints one for every existing scope file" (07).

Case map:
  [A] create --template dashboard: path-only answer, 0600 file, `visibility: all`, no secret
  [B] refusals: an existing file is refused and left byte-identical; `../evil` and an
      uppercase name are `invalid scope name` with nothing written; `--template Dashboard`
      is the pinned template refusal
  [C] first `mcp --listen` bind: instance token + one 0600 secret per existing scope file
  [D] the created scope authenticates a real tools/call over TCP (list_capabilities and
      list_sessions answer results); a wrong scope secret and a missing one are 403s
  [E] a scope created WHILE the bridge runs has no secret and is refused as such; the NEXT
      network start mints it, and the rebind leaves every existing credential unchanged
  [F] no secret value appears in any HTTP answer, and no scope file carries one

Usage: probe-scope-create.py --bin <daiorama> [--port N]
Prints [OK]/[FAIL] per assertion, `FAILS=<n>`; exit code = FAILS.
"""
import argparse
import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time

FAILS = 0


def ok(label, detail=""):
    print(f"[OK]   {label}" + (f" ({detail})" if detail else ""))


def fail(label, detail):
    global FAILS
    FAILS += 1
    print(f"[FAIL] {label} :: {detail}")


def note(label):
    print(f"[NOTE] {label}")


def check(label, cond, detail=""):
    if cond:
        ok(label, detail)
    else:
        fail(label, detail)
    return cond


def free_port(preferred):
    s = socket.socket()
    try:
        s.bind(("127.0.0.1", preferred))
        return preferred
    except OSError:
        s.close()
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def read_response(sock, timeout=5.0):
    sock.settimeout(timeout)
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(65536)
        if not chunk:
            return ("EOF_IN_HEAD", data)
        data += chunk
    head, rest = data.split(b"\r\n\r\n", 1)
    status = head.decode("utf-8", "replace").split("\r\n")[0]
    length = 0
    for line in head.decode("utf-8", "replace").split("\r\n")[1:]:
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
    body = rest
    while len(body) < length:
        chunk = sock.recv(65536)
        if not chunk:
            break
        body += chunk
    return (status, body[:length])


def mcp(port, headers, method, params=None, request_id=1, timeout=5.0):
    payload = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        payload["params"] = params
    body = json.dumps(payload).encode()
    lines = [f"POST /mcp HTTP/1.1", f"Host: 127.0.0.1:{port}",
             f"Content-Length: {len(body)}", "Connection: close"]
    lines += [f"{k}: {v}" for k, v in headers]
    raw = ("\r\n".join(lines) + "\r\n\r\n").encode() + body
    s = socket.create_connection(("127.0.0.1", port), timeout=timeout)
    try:
        s.sendall(raw)
        status, data = read_response(s, timeout)
    finally:
        s.close()
    text = data.decode("utf-8", "replace")
    try:
        obj = json.loads(text)
    except ValueError:
        obj = None
    return status, text, obj


def tool_call(port, headers, name, arguments=None, request_id=7):
    return mcp(port, headers, "tools/call",
               {"name": name, "arguments": arguments or {}}, request_id=request_id)


def wait_accept(port, attempts=100):
    for _ in range(attempts):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
            return True
        except OSError:
            time.sleep(0.05)
    return False


def wait_path(path, attempts=100):
    for _ in range(attempts):
        if os.path.exists(path):
            return True
        time.sleep(0.05)
    return False


def sha(path):
    with open(path, "rb") as handle:
        return hashlib.sha256(handle.read()).hexdigest()


def mode(path):
    return os.stat(path).st_mode & 0o777


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--port", type=int, default=18770)
    args = ap.parse_args()
    binary = os.path.abspath(args.bin)
    if not os.path.exists(binary):
        sys.exit(f"no binary at {binary}")

    root = tempfile.mkdtemp(prefix="drscope.", dir="/tmp")
    env = dict(os.environ)
    env.update({
        "HOME": root,
        "XDG_CONFIG_HOME": f"{root}/config",
        "XDG_DATA_HOME": f"{root}/data",
        "XDG_RUNTIME_DIR": f"{root}/run",
        "TMPDIR": f"{root}/tmp",
    })
    for d in ("config", "data", "run", "tmp"):
        os.makedirs(f"{root}/{d}", exist_ok=True)
    for var in ("DAIORAMA_TOKEN", "DAIORAMA_SCOPE", "DAIORAMA_SCOPE_TOKEN", "DAIORAMA_MODE"):
        env.pop(var, None)
    conf = f"{root}/config/daiorama/default"
    data = f"{root}/data/daiorama/default"
    scopes = f"{conf}/scopes"
    secrets = f"{data}/scope-tokens"
    sock_path = f"{root}/run/daiorama/default/manager.sock"
    token_path = f"{data}/token"

    def run(*argv):
        return subprocess.run([binary, *argv], env=env, capture_output=True, text=True)

    procs = []
    try:
        subprocess.run([binary, "--version"], env=env, capture_output=True, check=True)
        setup = run("config", "setup")
        if setup.returncode != 0:
            sys.exit(f"config setup failed: {setup.stderr}")
        limit = run("config", "set", "limits.mode", "off")
        if limit.returncode != 0:
            sys.exit(f"config set limits.mode failed: {limit.stderr}")

        print("=== [A] create --template dashboard ===")
        created = run("config", "scope", "create", "dashboard", "--template", "dashboard")
        dash = f"{scopes}/dashboard.ron"
        check("create exits 0", created.returncode == 0, created.stderr.strip())
        check("the answer is the path only", created.stdout == f"{dash}\n", repr(created.stdout))
        check("file exists", os.path.exists(dash))
        if os.path.exists(dash):
            check("file is 0600", mode(dash) == 0o600, oct(mode(dash)))
            text = open(dash).read()
            check("dashboard template has visibility all", "visibility: all" in text)
            check("the file is commented", "//" in text)
        check("create writes no scope secret", not os.path.exists(f"{secrets}/dashboard"))

        print("\n=== [B] refusals ===")
        before = sha(dash) if os.path.exists(dash) else None
        again = run("config", "scope", "create", "dashboard", "--template", "dashboard")
        check("second create refuses", again.returncode != 0)
        check("refusal is the pinned string",
              'scope "dashboard" already exists' in again.stderr, again.stderr.strip()[:120])
        after = sha(dash) if os.path.exists(dash) else None
        check("an existing scope file is left byte-identical", before == after)

        escape = run("config", "scope", "create", "../evil", "--template", "all")
        check("`../evil` refuses", escape.returncode != 0)
        check("`../evil` is `invalid scope name`", "invalid scope name" in escape.stderr,
              escape.stderr.strip()[:120])
        check("nothing escaped the scopes dir",
              not os.path.exists(f"{conf}/evil.ron") and not os.path.exists(f"{root}/evil.ron"))

        upper = run("config", "scope", "create", "BadName")
        check("uppercase name refuses", upper.returncode != 0
              and "invalid scope name" in upper.stderr, upper.stderr.strip()[:120])
        check("no file for the refused name", not os.path.exists(f"{scopes}/BadName.ron"))

        bad_template = run("config", "scope", "create", "extra", "--template", "Dashboard")
        check("`--template Dashboard` refuses", bad_template.returncode != 0)
        check("template refusal is the pinned string",
              'invalid scope template "Dashboard"; expected own, all, or dashboard'
              in bad_template.stderr, bad_template.stderr.strip()[:160])
        check("no file for the refused template", not os.path.exists(f"{scopes}/extra.ron"))

        own = run("config", "scope", "create", "own", "--template", "own")
        own_path = f"{scopes}/own.ron"
        check("create own exits 0", own.returncode == 0, own.stderr.strip()[:120])
        check("own file is 0600", os.path.exists(own_path) and mode(own_path) == 0o600)
        check("own template has visibility own",
              os.path.exists(own_path) and "visibility: own" in open(own_path).read())

        print("\n=== [C] first network start mints the credentials ===")
        manager = subprocess.Popen([binary, "serve"], env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        procs.append(manager)
        check("manager socket appears", wait_path(sock_path), sock_path)

        port = free_port(args.port)
        bridge_log = open(f"{root}/bridge1.log", "w")
        bridge = subprocess.Popen([binary, "mcp", "--listen", f"127.0.0.1:{port}"], env=env,
                                  stdout=subprocess.DEVNULL, stderr=bridge_log)
        procs.append(bridge)
        check("bridge accepts on the bound port", wait_accept(port), f"port {port}")
        check("instance token file exists", wait_path(token_path))
        check("instance token is 0600", os.path.exists(token_path) and mode(token_path) == 0o600,
              oct(mode(token_path)) if os.path.exists(token_path) else "missing")
        token = open(token_path).read().strip() if os.path.exists(token_path) else ""
        secret = {}
        for name in ("operator", "dashboard", "own"):
            path = f"{secrets}/{name}"
            present = wait_path(path, attempts=20)
            check(f"secret for {name} minted", present)
            if present:
                check(f"secret for {name} is 0600", mode(path) == 0o600, oct(mode(path)))
                secret[name] = open(path).read().strip()
        if not token or len(secret) < 3:
            print("cannot continue without the minted credentials")
            return 91

        def headers_for(scope, use_secret=None, instance=None):
            return [("Daiorama-Token", instance or token), ("Daiorama-Scope", scope),
                    ("Daiorama-Scope-Token", secret.get(scope) if use_secret is None else use_secret)]

        print("\n=== [D] the created scope authenticates a real call over TCP ===")
        status, text, obj = mcp(port, headers_for("dashboard"), "initialize")
        check("initialize answers 200", status.startswith("HTTP/1.1 200"), status)
        check("initialize carries a result", obj is not None and "result" in obj)

        status, text, obj = tool_call(port, headers_for("dashboard"), "list_capabilities")
        check("list_capabilities answers 200", status.startswith("HTTP/1.1 200"), status)
        check("list_capabilities is not an error",
              obj is not None and "error" not in obj
              and obj.get("result", {}).get("isError", False) is False,
              (text or "")[:160])
        caps = (obj or {}).get("result", {})
        check("capabilities carry the always-on set",
              "always_on" in json.dumps(caps), str(list(caps)[:4]))

        status, text, obj = tool_call(port, headers_for("dashboard"), "list_sessions")
        check("list_sessions answers 200", status.startswith("HTTP/1.1 200"), status)
        check("list_sessions is not an error",
              obj is not None and "error" not in obj
              and obj.get("result", {}).get("isError", False) is False,
              (text or "")[:160])

        status, text, obj = tool_call(port, headers_for("dashboard", use_secret=secret["own"]),
                                      "list_sessions")
        check("a foreign scope secret is 403", status.startswith("HTTP/1.1 403"), status)
        check("the refusal names scope_auth_required", "scope_auth_required" in text,
              text[:160])

        nowhere = [("Daiorama-Token", token), ("Daiorama-Scope", "dashboard")]
        status, text, obj = mcp(port, nowhere, "tools/call",
                                {"name": "list_sessions", "arguments": {}})
        check("a missing scope secret is 403", status.startswith("HTTP/1.1 403"), status)
        check("the missing-secret refusal is scope_auth_required",
              "scope_auth_required" in text, text[:160])

        status, text, obj = tool_call(port, headers_for("dashboard", instance="not-the-token"),
                                      "list_capabilities")
        check("a wrong instance token is 403", status.startswith("HTTP/1.1 403"), status)
        check("the identity refusal is token_required", "token_required" in text, text[:160])

        print("\n=== [E] a scope created while the bridge runs is usable at the NEXT start ===")
        late = run("config", "scope", "create", "late", "--template", "all")
        check("late create exits 0", late.returncode == 0, late.stderr.strip()[:120])
        check("late has no secret while the bridge is up", not os.path.exists(f"{secrets}/late"))
        status, text, obj = tool_call(port,
                                      [("Daiorama-Token", token), ("Daiorama-Scope", "late"),
                                       ("Daiorama-Scope-Token", secret["own"])], "list_sessions")
        check("late is refused before it has a secret", status.startswith("HTTP/1.1 403"), status)
        check("the refusal says the scope has no secret", "has no secret" in text, text[:160])

        bridge.terminate()
        try:
            bridge.wait(timeout=5)
        except subprocess.TimeoutExpired:
            bridge.kill()
        bridge_log.close()
        bridge = subprocess.Popen([binary, "mcp", "--listen", f"127.0.0.1:{port}"], env=env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        procs.append(bridge)
        check("the bridge rebinds", wait_accept(port), f"port {port}")
        check("the next start mints the late secret", wait_path(f"{secrets}/late", attempts=40))
        check("late secret is 0600",
              os.path.exists(f"{secrets}/late") and mode(f"{secrets}/late") == 0o600)
        check("the instance token is unchanged by the rebind",
              open(token_path).read().strip() == token)
        check("the dashboard secret is unchanged by the rebind",
              open(f"{secrets}/dashboard").read().strip() == secret["dashboard"])
        if os.path.exists(f"{secrets}/late"):
            late_secret = open(f"{secrets}/late").read().strip()
            status, text, obj = tool_call(port,
                                          [("Daiorama-Token", token), ("Daiorama-Scope", "late"),
                                           ("Daiorama-Scope-Token", late_secret)],
                                          "list_sessions")
            check("the late scope answers after the rebind",
                  status.startswith("HTTP/1.1 200") and obj is not None
                  and "error" not in obj
                  and obj.get("result", {}).get("isError", False) is False,
                  (text or "")[:160])

        print("\n=== [F] no secret leaves a file that should not carry one ===")
        blobs = [text]
        _, text2, _ = mcp(port, headers_for("dashboard"), "initialize", request_id=99)
        blobs.append(text2)
        leaked = []
        for label, value in [("instance token", token)] + \
                [(f"secret {name}", value) for name, value in secret.items()]:
            if any(value and value in blob for blob in blobs):
                leaked.append(label)
        check("no credential appears in an HTTP answer", not leaked, ",".join(leaked))
        scope_bodies = "".join(open(p).read() for p in (dash, own_path)
                               if os.path.exists(p))
        check("no credential appears in a scope file",
              all(value not in scope_bodies for value in secret.values()) and
              token not in scope_bodies)
        print(f"[NOTE] secrets checked: instance token + {len(secret)} scope secrets + late")
    finally:
        for proc in procs:
            if proc.poll() is None:
                proc.terminate()
        for proc in procs:
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
        shutil.rmtree(root, ignore_errors=True)

    print(f"FAILS={FAILS}")
    return 0 if FAILS == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
