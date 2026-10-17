#!/usr/bin/env python3
"""Live battery for the `/media/<id>` network pass-through (05 A8).

Runs the real binary's `mcp --listen` against a fake manager that speaks the media surface
over the instance's unix socket, and checks the pinned relay contract:

  200 pass-through (binary-safe, Content-Type/Length preserved), 404 + media_expired
  pass-through, 403 token-only gate, 405 + Allow: GET, the path-shape matrix (bridge-side
  404, manager never contacted), malformed upstream (no length / chunked) -> 502 internal,
  manager down -> 502 manager_unreachable, Range ignored, fetch-once double-GET, streaming
  (first bytes arrive while the manager still holds the rest), and ids absent from stderr.

    python3 p3/media/evidence/probe-media.py --bin PATH
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time

ID_OK = "a" * 32
ID_404 = "b" * 32
ID_MAL = "c" * 32
ID_TE = "d" * 32
ID_STREAM = "e" * 32
ID_ONCE = "f" * 32
BINARY_BODY = bytes(range(256)) * 4  # 1024 bytes, every byte value

failures = 0


def report(ok, label, detail):
    global failures
    if not ok:
        failures += 1
    print(f"[{'PASS' if ok else 'FAIL'}] {label} :: {detail}")


def read_head(conn):
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = conn.recv(4096)
        if not chunk:
            return buf
        buf += chunk
    return buf.split(b"\r\n\r\n", 1)[0]


def http_get(port, target, headers=(), timeout=5.0):
    s = socket.create_connection(("127.0.0.1", port), timeout=timeout)
    req = f"GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n".encode()
    for name, value in headers:
        req += f"{name}: {value}\r\n".encode()
    req += b"Connection: close\r\n\r\n"
    s.sendall(req)
    return read_response(s)


def http_method(port, method, target, headers=()):
    s = socket.create_connection(("127.0.0.1", port), timeout=5.0)
    req = f"{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n".encode()
    for name, value in headers:
        req += f"{name}: {value}\r\n".encode()
    req += b"Content-Length: 0\r\nConnection: close\r\n\r\n"
    s.sendall(req)
    return read_response(s)


def read_response(s, timeout=5.0):
    s.settimeout(timeout)
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    head, _, body = buf.partition(b"\r\n\r\n")
    status = head.split(b"\r\n", 1)[0].decode("latin-1", "replace") if head else "?"
    headers = {}
    for line in head.split(b"\r\n")[1:]:
        name, _, value = line.partition(b":")
        headers.setdefault(name.strip().lower().decode(), []).append(value.strip().decode("latin-1"))
    length = int((headers.get("content-length") or ["0"])[0] or 0)
    while len(body) < length:
        chunk = s.recv(65536)
        if not chunk:
            break
        body += chunk
    s.close()
    return status, headers, body[:length]


def start_fake(root, requests):
    socket_path = os.path.join(root, "run/daiorama/default/manager.sock")
    os.makedirs(os.path.dirname(socket_path), exist_ok=True)
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(socket_path)
    srv.listen(8)
    once_state = {"served": False}

    def handle(conn):
        head = read_head(conn)
        line = head.split(b"\r\n", 1)[0].decode("latin-1")
        requests.append(line)
        target = line.split()[1] if len(line.split()) > 1 else ""
        if target.endswith(ID_STREAM):
            head_out = (
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n"
                "Content-Length: 4096\r\n\r\n"
            )
            conn.sendall(head_out.encode() + b"a" * 2048)
            time.sleep(0.5)
            conn.sendall(b"b" * 2048)
        elif target.endswith(ID_ONCE):
            if not once_state["served"]:
                once_state["served"] = True
                body = b"once-body"
                conn.sendall(
                    (
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n"
                        f"Content-Length: {len(body)}\r\n\r\n"
                    ).encode()
                    + body
                )
            else:
                body = json.dumps({"code": "media_expired", "message": "media link expired"}).encode()
                conn.sendall(
                    (
                        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\n"
                        f"Content-Length: {len(body)}\r\n\r\n"
                    ).encode()
                    + body
                )
        elif target.endswith(ID_MAL):
            conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\r\njunk")
        elif target.endswith(ID_TE):
            conn.sendall(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\njunk\r\n0\r\n\r\n"
            )
        elif target.endswith(ID_404):
            body = json.dumps({"code": "media_expired", "message": "media link expired"}).encode()
            conn.sendall(
                (
                    "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\n"
                    f"Content-Length: {len(body)}\r\n\r\n"
                ).encode()
                + body
            )
        else:
            conn.sendall(
                (
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n"
                    f"Content-Length: {len(BINARY_BODY)}\r\n\r\n"
                ).encode()
                + BINARY_BODY
            )
        conn.close()

    def loop():
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            threading.Thread(target=handle, args=(conn,), daemon=True).start()

    threading.Thread(target=loop, daemon=True).start()
    return srv


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--port", type=int, default=18775)
    args = ap.parse_args()
    binary = os.path.abspath(args.bin)
    root = tempfile.mkdtemp(prefix="dlmedia.", dir="/tmp")
    env = dict(os.environ)
    env.update(
        {
            "HOME": root,
            "XDG_CONFIG_HOME": f"{root}/config",
            "XDG_DATA_HOME": f"{root}/data",
            "XDG_RUNTIME_DIR": f"{root}/run",
            "TMPDIR": f"{root}/tmp",
        }
    )
    for d in ("config", "data", "run", "tmp"):
        os.makedirs(f"{root}/{d}", exist_ok=True)
    for key in ("DAIORAMA_TOKEN", "DAIORAMA_SCOPE", "DAIORAMA_SCOPE_TOKEN"):
        env.pop(key, None)
    subprocess.run([binary, "config", "setup"], env=env, capture_output=True, check=True)
    bridge = subprocess.Popen(
        [binary, "mcp", "--listen", f"127.0.0.1:{args.port}"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    for _ in range(100):
        line = bridge.stderr.readline().decode(errors="replace")
        if "listening on" in line:
            break
    token = open(f"{root}/data/daiorama/default/token").read().strip()
    requests = []
    print(f"bin={os.path.basename(binary)} port={args.port}")

    # Manager down first (no fake socket yet).
    status, _, body = http_get(args.port, f"/media/{ID_OK}", [("Daiorama-Token", token)])
    report(
        status.startswith("HTTP/1.1 502") and b"manager_unreachable" in body,
        "manager down -> 502 manager_unreachable",
        f"{status!r} body={body[:60]!r}",
    )

    srv = start_fake(root, requests)

    status, headers, body = http_get(args.port, f"/media/{ID_OK}", [("Daiorama-Token", token)])
    ok = (
        status.startswith("HTTP/1.1 200")
        and body == BINARY_BODY
        and (headers.get("content-type") or [""])[0] == "application/octet-stream"
    )
    report(ok, "200 pass-through is binary-safe", f"{status!r} bytes={len(body)}")

    status, headers, body = http_get(args.port, f"/media/{ID_OK}")
    report(
        status.startswith("HTTP/1.1 403") and b"token_required" in body,
        "no token -> 403 token_required",
        f"{status!r} body={body[:50]!r}",
    )
    status, _, _ = http_get(args.port, f"/media/{ID_OK}", [("Daiorama-Token", "wrong")])
    report(status.startswith("HTTP/1.1 403"), "wrong token -> 403", repr(status))

    status, headers, body = http_method(
        args.port, "POST", f"/media/{ID_OK}", [("Daiorama-Token", token)]
    )
    report(
        status.startswith("HTTP/1.1 405")
        and (headers.get("allow") or [""])[0] == "GET"
        and b"media fetches are GET only" in body,
        "POST /media -> 405 + Allow: GET",
        f"{status!r} allow={(headers.get('allow') or ['-'])[0]}",
    )

    before = len(requests)
    shapes = [
        "/media",
        "/media/",
        "/media/" + "a" * 31,
        "/media/" + "a" * 33,
        "/media/" + "A" * 32,
        "/media/" + "g" * 32,
        "/media/../../token",
        "/media/" + "a" * 32 + "/extra",
    ]
    all_404 = True
    for shape in shapes:
        status, _, body = http_get(args.port, shape, [("Daiorama-Token", token)])
        if not (status.startswith("HTTP/1.1 404") and b"media_expired" in body):
            all_404 = False
            print(f"  shape {shape!r} -> {status!r}")
    report(
        all_404 and len(requests) == before,
        "path-shape matrix -> 404 media_expired, manager untouched",
        f"{len(shapes)} shapes, upstream calls +{len(requests) - before}",
    )

    status, headers, body = http_get(args.port, f"/media/{ID_404}", [("Daiorama-Token", token)])
    report(
        status.startswith("HTTP/1.1 404") and b"media_expired" in body,
        "upstream 404 passes through with its body",
        f"{status!r} body={body[:50]!r}",
    )

    for ident, label in ((ID_MAL, "no Content-Length"), (ID_TE, "chunked upstream")):
        status, _, body = http_get(args.port, f"/media/{ident}", [("Daiorama-Token", token)])
        report(
            status.startswith("HTTP/1.1 502")
            and b"media upstream answered without a Content-Length" in body,
            f"malformed upstream ({label}) -> 502 internal",
            f"{status!r}",
        )

    status, _, body = http_get(
        args.port, f"/media/{ID_OK}", [("Daiorama-Token", token), ("Range", "bytes=0-9")]
    )
    report(
        status.startswith("HTTP/1.1 200") and body == BINARY_BODY,
        "Range ignored -> full 200 body",
        f"{status!r} bytes={len(body)}",
    )

    status, _, body = http_get(args.port, f"/media/{ID_ONCE}", [("Daiorama-Token", token)])
    first_ok = status.startswith("HTTP/1.1 200")
    status2, _, body2 = http_get(args.port, f"/media/{ID_ONCE}", [("Daiorama-Token", token)])
    report(
        first_ok and status2.startswith("HTTP/1.1 404") and b"media_expired" in body2,
        "fetch-once: second GET is media_expired",
        f"first={status!r} second={status2!r}",
    )

    start = time.monotonic()
    s = socket.create_connection(("127.0.0.1", args.port), timeout=10.0)
    s.sendall(
        f"GET /media/{ID_STREAM} HTTP/1.1\r\nHost: 127.0.0.1:{args.port}\r\n"
        f"Daiorama-Token: {token}\r\nConnection: close\r\n\r\n".encode()
    )
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += s.recv(65536)
    _, _, rest = buf.partition(b"\r\n\r\n")
    first_at = time.monotonic() - start
    got = len(rest)
    while got < 4096:
        chunk = s.recv(65536)
        if not chunk:
            break
        got += len(chunk)
    total_at = time.monotonic() - start
    s.close()
    report(
        first_at < total_at - 0.30 and got == 4096,
        "streams before the upstream finishes",
        f"first={first_at:.2f}s total={total_at:.2f}s bytes={got}",
    )

    bridge.terminate()
    stderr = bridge.stderr.read().decode(errors="replace")
    leaked = [ident for ident in (ID_OK, ID_404, ID_MAL, ID_TE, ID_STREAM, ID_ONCE) if ident in stderr]
    report(not leaked, "ids absent from bridge stderr", f"leaks={leaked}")
    srv.close()
    print(f"\nupstream requests seen: {len(requests)}")
    for line in requests[:4]:
        print("  ", line.replace(ID_OK, "<id>")[:80])
    print(f"\nFAILS={failures}")
    sys.exit(0 if failures == 0 else 1)


main()
