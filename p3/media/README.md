# P3 W2 — `/media/<id>` pass-through on the network listener

Implemented. The network bridge's listener (`daiorama mcp --listen`) serves `GET /media/<id>`
and relays it to the manager's media endpoint, streamed back unchunked. Capture tools
(`clip_video`, the producer of an id) and the manager-side media store are a separate slice;
this one pins the relay against a fake upstream (the live battery below) so the contract is
fixed before both sides are built.

## The contract (as frozen + pinned in `docs/05`, `docs/07`, `docs/11`, `fixtures/README.md`)

- **05 §Capture (A8):** the manager mints the `clip_video` resource link as a *single-use,
  TTL-bounded capability URL on the surface that served the call*; for network-bridge calls
  **the bridge relays `/media/*` pass-through on its existing listener (no validation there —
  the 128-bit opaque id is the credential; ids are never written to history or logs)**.
- **Lifetime:** deleted after the response body completes (an open fd keeps serving concurrent
  readers; first-complete-wins, no Range/resume); later fetches get `404` + `media_expired`.
  TTL `media.ttl = 600` s (04; the A8 decision log's "60 s" was a dropped zero — corrected in
  `docs/11` in this slice).
- **07:** media endpoint exposure is "on the manager surface"; the id is the credential; ids
  never logged.

## The pinned relay shape (signed off with the peer's five amendments)

1. **Auth:** the listener's gate is the **instance token only** (`Daiorama-Token`) — a media
   fetch is not an MCP request, so the `/mcp` scope headers are not part of it; a plain HTTP
   client with the token can fetch. Documented beside the `/mcp` gate in 05.
2. **Path:** `<id>` must be exactly **32 lowercase hex** characters. Any other shape (uppercase,
   length, separators, traversal, `/media`, `/media/`) is `404` + `media_expired` **at the
   bridge**, and never becomes an upstream request. Parsing, not existence validation — A8
   holds.
3. **Methods:** `GET` only on `/media/*`; anything else is `405` + `Allow: GET`
   (`media fetches are GET only`).
4. **Upstream:** HTTP/1.1 `GET /media/<id>` on the manager's own surface addressing (the local
   unix socket: tokenless). `200` + `Content-Type` + `Content-Length`, no chunked, no Range;
   `404` passes through with its `media_expired` body. An upstream answer without
   `Content-Length`, or chunked, is `502` + `internal`
   (`media upstream answered without a Content-Length`) — the bridge does not guess where the
   body ends. Manager down → `502` + `manager_unreachable`.
5. **Logging:** the request target is never written to stderr, refusals included.

## Evidence

- `p3/media/evidence/probe-media.txt` — live battery against the real binary + a fake manager
  over the instance socket: **13/13 PASS, FAILS=0**. Covers binary-safe 200, 401 token-only
  gate, 405, the 8-shape path matrix (manager untouched), upstream 404, both malformed-upstream
  shapes, Range ignored, fetch-once double-GET, streaming (first bytes at 0.00 s, body total
  0.50 s), and ids absent from stderr.
- Rust tests in `tests/mcp_listen.rs` (`media_*`, 8 cases) pin the same contract in-process,
  including a deterministic streaming test (the fake holds the second half until the client has
  read the first).

## Test plan status

All cases from the design note + the peer's additions are covered by the battery and the Rust
tests (path-shape matrix including traversal and the 31/33/uppercase/non-hex arms, `Range`
ignored, token-only auth, fetch-once double-GET). Gates on the head: fmt/clippy clean ·
**165 (+5i) · 4 · 2 · 27 · 7 · 22 mcp_listen**.
