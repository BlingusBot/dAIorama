#!/usr/bin/env python3
"""Live views leg: real manager + real session + real wayvnc.

1. `daiorama view <id> --json` -> address/credential/generation.
2. Front page with the credential -> 200; without -> 403.
3. WebSocket upgrade with Origin + credential -> 101, first frame carries the RFB banner.
4. `view --stop` -> front port closed, no wayvnc process left.
5. `view --json` again -> generation 2, new credential.
"""
import http.client
import json
import os
import socket
import subprocess
import sys
import time

session_id = sys.argv[1]


def cli(*args):
    return subprocess.run(
        ["daiorama", *args], capture_output=True, text=True, env=dict(os.environ)
    )


def get_view():
    out = cli("view", session_id, "--json")
    if out.returncode != 0:
        raise SystemExit(f"view failed: {out.stdout} {out.stderr}")
    return json.loads(out.stdout)


first = get_view()
print("view:", json.dumps({k: first[k] for k in ("generation",)}))
address = first["address"]
credential = first["credential"]
assert address.startswith("http://127.0.0.1:"), address
assert len(credential) == 64, credential
host_port = address.split("//", 1)[1].split("/", 1)[0]
host, port = host_port.split(":")
port = int(port)

page = http.client.HTTPConnection(host, port, timeout=5)
page.request("GET", f"/?credential={credential}")
reply = page.getresponse()
body = reply.read()
print("page with credential:", reply.status, len(body), "cookie:", bool(reply.getheader("Set-Cookie")))
assert reply.status == 200, reply.status
assert reply.getheader("Referrer-Policy") == "no-referrer"

anonymous = http.client.HTTPConnection(host, port, timeout=5)
anonymous.request("GET", "/")
reply = anonymous.getresponse()
reply.read()
print("page without credential:", reply.status)
assert reply.status == 403, reply.status

# WebSocket upgrade, Origin required, credential in the query (the noVNC bootstrap form).
sock = socket.create_connection((host, port), timeout=5)
handshake = (
    f"GET /websockify?credential={credential} HTTP/1.1\r\n"
    f"Host: {host}:{port}\r\n"
    f"Origin: http://{host}:{port}\r\n"
    "Upgrade: websocket\r\nConnection: Upgrade\r\n"
    "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
)
sock.sendall(handshake.encode())
head = b""
while b"\r\n\r\n" not in head:
    chunk = sock.recv(4096)
    if not chunk:
        break
    head += chunk
head_text = head.split(b"\r\n\r\n", 1)[0].decode("latin-1")
print("upgrade:", head_text.splitlines()[0])
assert head_text.startswith("HTTP/1.1 101"), head_text
assert "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=" in head_text, head_text

# The RFB banner arrives as a binary frame (0x82) — wayvnc speaks first.
banner = b""
deadline = time.time() + 5
while b"RFB" not in banner and time.time() < deadline:
    banner += sock.recv(4096)
print("banner payload:", banner[:24])
assert b"RFB 003.008" in banner, banner[:32]
sock.close()

stopped = cli("view", session_id, "--stop")
print("stop rc:", stopped.returncode, stopped.stdout.strip(), stopped.stderr.strip())
assert stopped.returncode == 0

try:
    socket.create_connection((host, port), timeout=2)
    raise SystemExit("front still accepting after stop")
except OSError:
    print("front closed after stop: ok")
left = subprocess.run(["pgrep", "-c", "wayvnc"], capture_output=True, text=True).stdout.strip()
print("wayvnc processes left:", left or "0")
assert left in ("", "0"), left

second = get_view()
print("second generation:", second["generation"])
assert second["generation"] == first["generation"] + 1
assert second["credential"] != credential
cli("view", session_id, "--stop")
print("LIVE VIEWS OK")
