# P4-VIEWS — design note (runtime side)

Scope: `get_view` / `stop_view` on the manager's dispatch, the wayvnc process per session, and
the manager-owned front that serves noVNC and proxies RFB-over-WebSocket. Auth is **07's rule,
implemented once** — no second path invented on the front.

## Registry and lifecycle (06 B2, pinned)

- One view per session: `{state: starting|running|down, address, front_port, vnc_port,
  generation}` in the session record; generation increments on every start.
- `get_view` is session-scoped, deny-by-default, idempotent: start if absent/down, return
  `{session_id, address, credential, generation}` either way. `stop_view` stops only the view;
  the session is untouched. Session close and session death stop the view first.
- One start path, serialized per session (a lock), losers read the winner's address.
- `front_port` allocates from `view.port_base` upward (first free, released on stop/death);
  `vnc_port` is ephemeral loopback, manager-known, **not** part of the address contract.
- wayvnc runs with the session's `XDG_RUNTIME_DIR` / `WAYLAND_DISPLAY`, PDEATHSIG to the
  manager, and is killed by `stop_view`, session close, or session death.
- View death ≠ session death: mark `down`, leave the session running; the next `get_view`
  starts a new generation.

## wayvnc

`view.wayvnc_command` (argv array, default `["wayvnc"]`) — the manager appends
`127.0.0.1 <vnc_port>`; no shell, no password (the front is the gate; loopback wayvnc is local
trust, same as the socket). The command is config so a test can substitute a fake. Nothing
about the view credential ever reaches wayvnc, its argv, or its environment.

## The front (07, pinned)

In-process HTTP/1.1 server (reusing the strict head parser: single `Host`, no chunked, bounded
head) on `127.0.0.1:<front_port>`, serving:

- `GET /` — noVNC page (static assets from `<data>/novnc`, packaging ships them in P5; a
  missing tree answers `503` + the pinned `view assets not installed` message, never a blank
  page);
- `GET /<asset>` — static files under that tree only (path traversal refused);
- `GET /websockify` with `Upgrade: websocket` — handshake, then a transparent relay between
  WebSocket binary frames and the loopback wayvnc TCP socket (no RFB inspection, no
  client-supplied host/port/path).

**Credential (07 verbatim).** `credential = HMAC-SHA256(instance_token, "view:" || session_id
|| ":" || generation)`, lowercase hex; multi-use; valid only for the current generation
(structurally dead after stop/death). Returned by `get_view` as a query parameter on the
address **and** as a separate `credential` field; session status/detail never echo it. The
front gates **every** request — page, assets, and the WS upgrade (which also validates
`Origin`) — accepts it as `?credential=` or `X-Daiorama-View` header, compares
constant-time, and on the first credentialed page load sets an HttpOnly `SameSite=Strict`
cookie scoped to the front origin (cookie value = the same credential; no second secret).
Request targets are never logged, and the credential never appears in stderr, history, or
session logs — same rule as media ids.

HMAC-SHA256 is implemented in-crate (SHA-256 + HMAC, ~150 lines with RFC test vectors) — no new
dependency, and the same primitive the token lifecycle slice may reuse.

## Errors (05 codes)

- no such session / not visible → `session_not_found`
- noVNC assets missing → `view_unavailable`, pinned message
- wayvnc fails to start / dies before ready → `view_unavailable` with the command + exit detail
  (never the credential, never a request target)
- port allocation exhausted → `backend_busy`

## Tests

1. **Unit**: generation/idempotency, serialized start (two racing get_views → one process),
   stop_view leaves the session running, session close kills the view, view death → `down` +
   next get_view new generation, port allocation from `view.port_base`, HMAC vectors
   (RFC 4231), cookie set/verify, gate matrix (no credential / wrong credential / stale
   generation / valid query / valid header / cookie-only), Origin validation on the upgrade,
   traversal refusal, WS relay echo against a fake wayvnc (binary frames both ways), asset
   missing → `view_unavailable`.
2. **Live leg** (L1 image, real wayvnc): `get_view` → address + credential; fetch the page with
   the credential (200), without (403); WS handshake → first bytes are the RFB banner
   (`RFB 003.008\n`) relayed from wayvnc; `stop_view` closes the socket and the process;
   generation moved on restart. A fake-wayvnc leg covers the relay in CI where the image lacks
   wayvnc.

## Open questions before implementation

- **Q1** In-process front (HTTP + WS relay, no websockify dependency) — approve? It reuses the
  strict parser and keeps 07's gate in one place.
- **Q2** `view.wayvnc_command` as a config key (argv array, defaults `["wayvnc"]`) — or fixed
  argv? Recommend config for the same reason as `capture.*`.
- **Q3** noVNC assets at `<data>/novnc` with `view_unavailable` when missing, packaging in P5 —
  or vendor a minimal page now?
- **Q4** Cookie value = the same multi-use credential (generation-bound), not a second secret —
  approve?
