#!/usr/bin/env python3
"""Two client PROCESSES on one manager: a slow create, and a list/drain observer.

Child process 1 sends `create_session` (large geometry => long compositor chain).
This process sends `list_sessions` + `drain` every 100 ms and stamps every response.
A global stall shows as observer latencies that end exactly when the create does; a
reachable drain race shows as `drain -> {active:true, remaining:0}` while the create is
still running.
"""
import json, os, socket, sys, time

SOCK = sys.argv[1]
RES = sys.argv[2] if len(sys.argv) > 2 else "16384x16384"
RUN_SECONDS = float(sys.argv[3]) if len(sys.argv) > 3 else 12.0
LOG = "/tmp/timeline-events.jsonl"


def connect():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(120)
    s.connect(SOCK)
    f = s.makefile("rwb")
    f.write((json.dumps({"method": "handshake",
                         "params": {"protocol_version": "0.1.0",
                                    "min_supported": "0.1.0"}}) + "\n").encode())
    f.flush()
    f.readline()
    return s, f


def call(f, frame):
    f.write((json.dumps(frame) + "\n").encode())
    f.flush()
    return json.loads(f.readline())


def event(kind, **kw):
    rec = {"t": round(time.time(), 4), "kind": kind, **kw}
    with open(LOG, "a") as fh:
        fh.write(json.dumps(rec) + "\n")
    return rec


# --- child: the create client (separate process, no shared GIL) ---
pid = os.fork()
if pid == 0:
    s, f = connect()
    t0 = time.time()
    out = call(f, {"method": "create_session", "params": {"resolution": RES}})
    event("create", start=round(t0, 4), end=round(time.time(), 4),
          ms=int((time.time() - t0) * 1000),
          id=(out.get("result") or {}).get("id") if isinstance(out, dict) else None,
          raw=json.dumps(out)[:200])
    os._exit(0)

# --- third client: opened *during* the create, times handshake / status / list ---
time.sleep(0.3)
t_open = time.time()
s3, f3 = connect()          # connect() includes the handshake
event("fresh_handshake", lat=round((time.time() - t_open) * 1000, 2))
for meth in ("status", "list_sessions"):
    t_m = time.time()
    out_m = call(f3, {"method": meth})
    res = out_m.get("result") if isinstance(out_m, dict) else None
    sess = res.get("sessions") if isinstance(res, dict) else None
    running = len([x for x in sess if isinstance(x, dict) and x.get("state") == "running"]) \
        if isinstance(sess, list) else None
    event("fresh_" + meth, lat=round((time.time() - t_m) * 1000, 2), running=running,
          raw=json.dumps(out_m)[:160])
s3.close()
event("fresh_client_close")

s, f = connect()
event("observer_start")
start = time.time()
seen_drain = None
while time.time() - start < RUN_SECONDS:
    t0 = time.time()
    out = call(f, {"method": "list_sessions"})
    lat = (time.time() - t0) * 1000
    sess = (out.get("result") or {}).get("sessions", []) if isinstance(out, dict) else []
    running = [x for x in sess if x.get("state") == "running"]
    event("list", lat=round(lat, 2), running=len(running))
    if seen_drain is None and running:
        # a session exists now: ask drain what it thinks, immediately
        t1 = time.time()
        d = call(f, {"method": "drain"})
        event("drain", lat=round((time.time() - t1) * 1000, 2), raw=json.dumps(d)[:200])
        seen_drain = d
        break
    time.sleep(0.1)

os.waitpid(pid, 0)
event("observer_end", drain=json.dumps(seen_drain)[:200] if seen_drain else None)

with open(LOG) as fh:
    for line in fh:
        print(line.rstrip())
