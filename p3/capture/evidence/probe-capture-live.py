#!/usr/bin/env python3
"""Drive the stdio bridge through one screenshot and one clip, then fetch the clip with
the CLI. Prints every step; exits non-zero on the first failed leg."""
import json
import os
import subprocess
import sys

session_id = sys.argv[1]
root = os.environ["ROOT"]

bridge = subprocess.Popen(
    ["daiorama", "mcp"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    text=True,
    env=dict(os.environ),
)


def call(payload):
    bridge.stdin.write(json.dumps(payload) + "\n")
    bridge.stdin.flush()
    line = bridge.stdout.readline()
    if not line:
        raise SystemExit(f"bridge closed after {payload}")
    return json.loads(line)


init = call(
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"protocolVersion": "2026-07-28"},
    }
)
print("initialize ok:", "result" in init)

shot = call(
    {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "screenshot", "arguments": {"session_id": session_id}},
    }
)
result = shot.get("result", {})
blocks = [block.get("type") for block in result.get("content") or []]
print("screenshot isError:", result.get("isError"), "blocks:", blocks)
assert blocks == ["image"], blocks
assert result["structuredContent"]["width"] == 1, result["structuredContent"]
assert result["structuredContent"]["height"] == 1, result["structuredContent"]
print("screenshot image block: bytes=", result["structuredContent"]["bytes"])

clip = call(
    {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "clip_video",
            "arguments": {"session_id": session_id, "duration_s": 1},
        },
    }
)
result = clip.get("result", {})
blocks = [block.get("type") for block in result.get("content") or []]
print("clip isError:", result.get("isError"), "blocks:", blocks)
assert blocks == ["resource_link", "text"], blocks
link = next(block for block in result["content"] if block["type"] == "resource_link")
media_id = link["uri"].rsplit("/", 1)[1]
print("resource_link uri:", link["uri"])
assert link["uri"].startswith("daiorama://media/"), link["uri"]

target = os.path.join(root, "live", "out.webm")
fetch = subprocess.run(
    ["daiorama", "media", "fetch", media_id, "--out", target],
    capture_output=True,
    text=True,
    env=dict(os.environ),
)
print("media fetch rc:", fetch.returncode, fetch.stdout.strip(), fetch.stderr.strip())
assert fetch.returncode == 0
with open(target, "rb") as handle:
    assert handle.read() == b"webm-bytes"

again = subprocess.run(
    ["daiorama", "media", "fetch", media_id, "--out", target + "2"],
    capture_output=True,
    text=True,
    env=dict(os.environ),
)
print("second fetch rc:", again.returncode, (again.stdout + again.stderr).strip()[:160])
assert again.returncode != 0
assert "media_expired" in (again.stdout + again.stderr)

bridge.stdin.close()
bridge.wait(timeout=5)
print("LIVE OK")
