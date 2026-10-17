#!/usr/bin/env python3
"""Latency hammer: one connection, `list_sessions` every 50 ms for N seconds.

Run while another *process* holds a create in flight (shell background) — no Python
threading involved. Prints each round-trip so a global stall is visible as a run of
latencies that end when the create does.
"""
import json, socket, sys, time

SOCK = sys.argv[1]
DURATION = float(sys.argv[2]) if len(sys.argv) > 2 else 2.0
INTERVAL = 0.05

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(60)
s.connect(SOCK)
f = s.makefile("rwb")


def call(frame):
    f.write((json.dumps(frame) + "\n").encode())
    f.flush()
    return json.loads(f.readline())


call({"method": "handshake", "params": {"protocol_version": "0.1.0", "min_supported": "0.1.0"}})
print(f"probe connected; hammering list_sessions every {int(INTERVAL*1000)}ms for {DURATION}s")
start = time.monotonic()
n = 0
while time.monotonic() - start < DURATION:
    t0 = time.monotonic()
    out = call({"method": "list_sessions"})
    dt = (time.monotonic() - t0) * 1000
    n += 1
    running = len([x for x in out.get("result", {}).get("sessions", []) if x.get("state") == "running"])
    print(f"  t=+{(t0-start)*1000:7.1f}ms list_rtt={dt:7.1f}ms running={running}")
    time.sleep(INTERVAL)
print(f"done: {n} calls")
