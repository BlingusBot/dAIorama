# P3 W2 — `/media/<id>` pass-through on the network listener

Design note before implementation. The slice: the network bridge's listener
(`daiorama mcp --listen`) serves `GET /media/<id>` and relays it to the manager's media
endpoint. Capture tools (`clip_video`, the producer of the id) and the manager-side media
store are a separate slice; this one pins the relay against a fake upstream so the
contract is fixed before both sides are built.

## What the frozen docs pin

- **05 §Capture, media (A8):** the manager mints the `clip_video` resource link as a
  *single-use, TTL-bounded capability URL on the surface that served the call*; for
  network-bridge calls **the bridge relays `/media/*` pass-through on its existing
  listener (no validation there — the 128-bit opaque id is the credential; ids are never
  written to history or logs)**.
- **Lifetime:** deleted after the response body completes (an open fd keeps serving
  concurrent readers; first-complete-wins, no Range/resume); later fetches get `404` with
  `media_expired`. TTL default `media.ttl = 600` s (04).
- **07:** media endpoint exposure is "on the manager surface"; the id is the credential;
  ids never logged.
- **11 A8:** "deleted at first fetch or TTL (60 s), whichever first — that is the redefined
  'ephemeral'; same exposure + token rule as all surfaces".

## Proposed pin (needs contract sign-off)

1. **Upstream shape.** The manager serves `GET /media/<id>` as HTTP/1.1 on its own
   surface addressing: over the unix socket in local mode (single request per connection,
   `Connection: close`), over its TCP bind otherwise (instance-token rule of that surface
   applies). Response: `200` with `Content-Type` and `Content-Length` (the artifact is a
   file, size known — no chunked responses), body = raw bytes; `404` with the canonical
   envelope (`media_expired`) as JSON. No `Range` support: a `Range` request is answered
   with the full body (ignored), not `206`.
2. **Bridge relay.** On `GET /media/<id>`: same `Host`/`Origin` rules as `POST /mcp`
   (Host required, Origin must match), **no credential gate** (the id is the credential),
   one upstream request, status/headers/body streamed through unchanged (no buffering:
   read/write chunks so the manager's open-fd concurrent-reader rule works). `404` is
   passed through as-is. Manager unreachable → `502` with the canonical envelope
   (`manager_unreachable`). Unknown paths/other methods keep the pinned
   `network bridge serves POST /mcp only`? — no: `/media/*` is a second target, so the
   target check needs its own shape (`GET /media/<id>` only; everything else 404/405 as
   today).
3. **Ids never logged.** The bridge has no request logging today; the pass-through must
   not add any (no path in errors beyond the pinned envelopes, no debug prints).
4. **TTL discrepancy (blocking question).** 05/04 say `media.ttl = 600` s default; 11 A8
   says 60 s. One of them is the typo — which value is canonical? (The relay itself is
   TTL-agnostic; the manager enforces it.)

## Test plan (fake manager, no real media store needed)

- `200` round-trip: exact bytes, `Content-Type`/`Content-Length` preserved, binary-safe.
- `404 media_expired` passed through with its envelope body.
- No credentials required; wrong `Host`/`Origin` refused by the pinned rules.
- Manager unreachable → `502` + `manager_unreachable` envelope.
- Large body streamed (a multi-MiB fake artifact, chunked reads on the wire — no full
  buffering; assert first bytes arrive before the upstream finishes).
- `Range: bytes=0-1` ignored → full `200` body.
- `GET /media/` (empty id), `POST /media/<id>`, `GET /media/<id>/extra` → pinned 404/405.
- Ids never appear in stderr.

## Gates

No test-count change expected beyond the new `mcp_listen` cases; the standard
`fmt · clippy -D warnings · unit · daemon · doctor · fixtures · mcp · mcp_listen` line
applies, plus the live battery raw beside this note.
