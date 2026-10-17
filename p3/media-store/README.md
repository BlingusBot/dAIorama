# P3 W2a — manager-side media store + `/media/<id>` endpoint

Design note before implementation. The slice is deliberately env-independent: the media store
lives in the manager process, is exercised by in-process tests with a temp data dir, and gets
exactly one live leg through the real network relay against a **test-minted** id. Capture
tools (the production mint callers) are the next slice and can be blocked by missing capture
binaries without holding this one.

## What the frozen docs pin

- **05 §Capture (A8):** the manager mints the `clip_video` resource link as a single-use,
  TTL-bounded capability URL on the surface that served the call; the 128-bit opaque id is the
  credential; ids are never written to history or logs; after the response body completes the
  artifact is deleted (an open fd keeps serving concurrent readers; first-complete-wins, no
  Range/resume); later fetches get `404` + `media_expired`; TTL `media.ttl = 600` s.
- **08:** persisted artifacts live in `<data root>/media/`, cap 128 MiB per session,
  oldest-first eviction, same retention as history; `audit.store_clips` /
  `audit.store_screenshots` persist the **already-captured** artifact (never a second capture)
  and persistence never resurrects the URL.
- **11 A8 / media pass-through decision:** the manager surface serves the URL; the bridge
  relays it unvalidated.

## Design (to pin)

1. **Store location and lifetime.** One `MediaStore` per manager instance, in memory:
   `id → Entry { content_type, created_at, session_id, scope, source, consumed }`. A manager
   restart drops every live entry — URLs die, fetches answer `404 media_expired` (the registry
   was never promised durable; only `audit.store_*` artifacts persist).
2. **Id and mint API.** `MediaId` = `uuid::Uuid::new_v4().simple()` — 32 lowercase hex, the
   same CSPRNG source the token/scope-secret generators use. `MediaStore::mint(session_id,
   scope, content_type, bytes, store_clips) -> MediaId` is an in-process API only: capture
   tools call it when they land; this slice tests it directly. **No RPC method, no CLI verb, no
   public mint surface.**
3. **Endpoint on the manager surface.** The manager's unix socket now accepts two protocols,
   distinguished by the first line: a line starting `GET /media/` is HTTP; anything else is a
   JSON-RPC frame as today (handshake first, unchanged). HTTP shape:
   - `GET /media/<32 lowercase hex>` → `200` + `Content-Type` + `Content-Length` (bytes from
     memory, or from the persisted file when the entry was restored/persisted), `Connection:
     close`. No chunked, no `Range` (full body; a `Range` request is ignored).
   - Unknown, expired, or already-consumed id → `404` + canonical envelope
     (`media_expired`) as JSON — the three cases are indistinguishable (same non-leak rule as
     `session_not_found`).
   - Any other method on `/media/...` → `405` + `Allow: GET`; any other HTTP-looking line →
     `404` + envelope (`bad_request`, "unknown target").
4. **Fetch-once.** A `GET` marks the entry consumed and serves from an open handle; the entry
   is removed from the map when the body write **completes** (first-complete-wins). Concurrent
   readers that already hold a handle keep being served; a `GET` after removal answers
   `media_expired`. A stalled write keeps the handle alive until the connection ends.
5. **TTL.** `config.media.ttl` (default 600 s) is read live at fetch/mint time (a reloaded
   config applies); entries older than the TTL are dropped during access (lazy sweep on fetch
   and mint — no background timer needed for v1).
6. **`audit.store_clips` persistence.** When the minting scope has `audit.store_clips = true`,
   `mint` also writes the same bytes to `<data root>/media/<id>` (0600; the data root itself is
   0700) — one artifact, no second capture. The persisted file is independent of the URL
   lifetime: consuming or expiring the URL never deletes it and never makes it pageable again.
   Retention: persisted bytes are tracked per session; over the 128 MiB per-session cap the
   oldest files are evicted first (08). `audit.store_screenshots` uses the same path when the
   screenshot tool lands.
7. **No ids anywhere.** No id in history records, logs, or error strings; the store never
   prints one. Tests assert absent ids in the manager's stderr for the live leg.

## Test plan

In-process (temp data dir, injected clock/cap where needed):

- mint → GET → exact bytes + `Content-Type`; second GET → `404 media_expired`.
- two handles opened before the first completes: both read the full body; a third GET after
  completion → `404`.
- TTL: `media.ttl = 1 s` (or injected now) → GET after expiry → `404`, entry gone.
- `store_clips` on: persisted file exists at `<data>/media/<id>` with the same bytes; after the
  URL is consumed the file remains and the URL still answers `404` ("never resurrects").
- `store_clips` off: no file.
- eviction: small injected cap → oldest persisted artifact goes first, newest survives.
- id shape: 32 lowercase hex; distinct across mints.
- no id in captured stderr on every refusal path.

One live leg (no compositor, no capture binaries): in-process manager on a temp root + the real
`daiorama mcp --listen` binary → GET `/media/<id>` through the relay returns the exact bytes;
a second GET through the relay answers `404 media_expired`; with `store_clips` on, the file is
present in the data dir and the URL still 404s. (The relay's own 13-case battery already covers
the bridge side.)

## Docs to touch

05 gets the manager-endpoint paragraph (HTTP over the manager socket, fetch-once/TTL semantics,
the indistinguishable-404 rule); 08 stays as-is unless the retention detail needs a line; 11
gets the decision block (store lifetime, in-process mint only, no resurrection).

## Gates

Standard line; expected growth is `unit` + the new store tests and, if the live leg lands in
`tests/`, an `mcp_listen`-style count stays unchanged (the relay is not modified). No `src/mcp`
changes are anticipated beyond none at all — the endpoint is manager-side.
