# `--listen` adversarial HTTP battery

`probe-http.py` runs a real manager (off-mode) plus a real `daiorama mcp --listen` and speaks
raw HTTP/1.1 over sockets. It exists because the network transport has shapes a handcrafted
JSON-RPC client never produces: header duplication, whitespace before a colon, `Content-Length`
forms a proxy reads differently, path forms, scope names, and connection lifetimes.

    python3 p3/listen/evidence/probe-http.py            # against target/debug/daiorama

`probe-http-before.txt` is the same battery against the merged W1b tree (`7c9d11c`) before the
strict-head fix; `probe-http.txt` is the fixed tree. The four differences are the finding:

| case | before | after |
|---|---|---|
| duplicate `Host` (two values, or the same twice) | 200, last value wins | 400 `malformed HTTP request` |
| empty `Host` | 200 (passed the "Host required" check) | 400 `malformed HTTP request` |
| `Host : value` (space before the colon) | 200, name trimmed | 400 `malformed HTTP request` |
| `Content-Length: +N` | 200, body framed | 400 `invalid Content-Length: "+N"` |

The last section of the run is exploratory (`WATCH`): bare-LF heads (accepted, RFC 7230 allows
it), leading-zero lengths (accepted, they are digits), scope-name traversal (all answer
`scope_unknown` — `load_scope` runs first, no file is read), origin shapes, pipelining, and the
connection-lifetime gap: 64 idle connections stay open, a slow body and an idle head are held
with no timeout. That last item is fixed now — see `probe-timeout.txt` and the decisions in
`docs/11`.

## probe-timeout.txt — transport limits

Five checks, live against the real binary (`probe-timeout.py`): the 64-connection cap closes an
over-cap socket on accept while held connections still answer; and the 10 s progress timeout
closes an idle kept-alive connection, a stalled head and a stalled body, each at 10.00 s.

| Check | Expected | Result |
|---|---|---|
| over-cap connection | closed on accept, fast | closed after 0.00 s |
| held connection under the cap | answers | `HTTP/1.1 200 OK` |
| idle keep-alive | close at ~10 s | 10.00 s |
| stalled head | close at ~10 s | 10.00 s |
| stalled body | close at ~10 s | 10.00 s |

`FAILS=0`.

## probe-head-deadline.txt — the total head window

The progress bound is per read, so a head dribbled one short line at a time never stalls long
enough to fire it: every line makes progress, and the slot is held until the client chooses to
finish. `probe-head-deadline.py` measures the whole head phase.

| Check | Before | After |
|---|---|---|
| head dribbled one line every 2 s | answered at 24.01 s (client chose to finish) | connection closed at 15.00 s, nothing written |
| head written in one go | 200 | 200 |

The WATCH line is the same shape one layer down and is **not** fixed here: a body trickled one
byte every 2 s is progress by the per-read bound, so the slot is still held (observed past 25 s).
A total body deadline was rejected for slow authentic uploads (`docs/11`); the honest state is
that a trickled body can still hold a slot — recorded so it is a decision, not an oversight.

## probe-lingering-close.txt — a refused burst keeps its refusal

`probe-lingering-close.py` bursts the body on the same connection a refusal is written to. A
client without `Expect: 100-continue` starts sending immediately; an abrupt close turns the
pending body into a reset and the refusal's body is lost.

| Burst | Before (`29e0417`) | After |
|---|---|---|
| 256 KiB | 251 bytes, intact | 251 bytes, intact |
| 1 MiB | 251 bytes, intact | 251 bytes, intact |
| 4 MiB | head 200, **0 body bytes**, ECONNRESET | 251 bytes, intact (4/4 runs) |

The fix: one `refuse()` helper — write the reply, half-close the write side (the FIN rides out
with the response), then drain a bounded slice (<= 64 KiB, <= 100 ms) before closing, unless the
request carried `Expect: 100-continue`. Every refusal site routes through it.

All refusal classes, 4 MiB body in flight (`delivered` = the refusal's JSON body arrived whole):

| Class | Before (`29e0417`) | After |
|---|---|---|
| gate (no creds) | 0 bytes | 408 bytes `token_required` |
| 404 wrong path | 0 | 242 `bad_request` |
| 405 GET /mcp | 0 | 242 |
| 400 duplicate Host | 0 | 214 |
| 400 missing Host | 0 | 208 |
| 403 origin mismatch | 0 | 256 |
| 400 chunked | 0 | 250 |
| 400 duplicate Content-Length | 0 | 218 |
| 400 invalid Content-Length | 0 | 232 |

Delivered: **0/9 before, 9/9 after**.

## probe-sdk-http.txt — the listener under a real MCP client SDK (dogfood)

Every probe above speaks raw HTTP/1.1 by hand, so none of them can see what an actual harness
sees. `probe-sdk-http.py` points the published Python client (`mcp==2.3.0`, `mode='auto'` —
discover first, then the initialize handshake) at a live `daiorama mcp --listen` and drives the
calls a harness makes, with two SDK clients on two scopes sharing one bridge.

    mcpvenv/bin/python p3/listen/evidence/probe-sdk-http.py --bin target/debug/daiorama

Green: the SDK connects (`protocol='2026-07-28'`, `server='daiorama'`), `tools/list` validates
(5 tools), `tools/call list_sessions` and `list_capabilities` relay, two concurrent scoped
clients both answer, and no credential material reaches bridge stderr.

### F1 — a credential refusal is invisible to a real client

| Refusal | Status / body on the wire | What the SDK reports |
|---|---|---|
| no token | `200`, envelope `token_required`, **`"id": null`** | **15.0 s**, `Request 'initialize' timed out` |
| bad token | same | **15.0 s**, `Request 'initialize' timed out` |
| bad scope secret | same | **15.0 s**, `Request 'initialize' timed out` |
| control: origin mismatch | `403`, envelope `origin not allowed` | **0.0 s**, `origin not allowed: "http://evil.example"` |

The gate runs on the head alone (11 §"The identity gate answers before the body is read"), so it
has no request id to echo: it answers `200` with a JSON-RPC error whose `id` is `null`. A raw
socket reads the envelope and is happy; a real client matches responses by id, cannot match
`null`, and waits for its own read timeout — the `token_required` text never reaches it. The
control line is the shape that works: a non-200 status makes the client read the body, so the
same envelope arrives instantly.

Why it matters: the wrong/rotated token is the most common harness misconfiguration, and it
presents as a 15 s hang ending in `Request 'initialize' timed out` — indistinguishable from a
dead or overloaded server, with the actionable `token_required` hint discarded.

Adopted (05 §bridge, 11): an id-less refusal answers its HTTP status with the same envelope and
`Connection: close` — `403` for the identity gate (`token_required`, `scope_auth_required`,
`scope_unknown`), `400` for a body parse error, `413` for a declared body over
`MAX_FRAME_BYTES`. `401` is never used (the OAuth-discovery trigger); the 200 envelope stays for
refusals that can name the request id. The media endpoint follows the same rule (`403` +
`token_required`).

After the fix, this probe is `FAILS=0`: all three credential refusals reach the SDK in **0.0 s**
with their message text, and the three raw legs show `403` / `400` / `413` with the envelope and
`Connection: close`.
