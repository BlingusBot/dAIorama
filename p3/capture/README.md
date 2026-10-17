# P3-CAPTURE — design note (runtime side)

Scope: the two capture tools that close the observe-then-act loop — `screenshot` and
`clip_video` — implemented on the manager's dispatch path and mapped to MCP content types by
the bridge. `clip_audio` stays out (named v1 cut, 01).

This is the tools-only slice: capture commands are executable and injectable, tests drive them
with fixture commands, and the real compositor binaries are an environment concern (P1 image)
that changes no code here.

## Where a capture runs

The manager spawns a per-call command with the **session's runtime environment** — the same
`XDG_RUNTIME_DIR` / `WAYLAND_DISPLAY` / `HYPRLAND_INSTANCE_SIGNATURE` the spawner handed the
compositor — so the capture sees exactly the session's output, never the manager's host session.
Commands are configured (DEFAULT pins, 08):

```ron
capture: (
  screenshot_command: "grim -t png -",        // stdout: one PNG
  clip_command: "wf-recorder --no-damage -f -", // stdout: webm (stopped with SIGINT)
  timeout_s: 10,                               // per call, both commands
  screenshot_max_bytes: 16777216,              // 16 MiB PNG; larger -> capture_too_large
),
```

Config keys live with `media.*` in 08's manager-side block (one new `capture.*` table). A test
sets both commands to a fixture script; no test needs a compositor.

## Tool shapes

### `screenshot` — `session_id`, optional `monitor` (Hyprland output name)

Manager runs the screenshot command, reads stdout (bounded), and returns a **typed payload**:

```json
{"capture": "image", "mimeType": "image/png", "data_b64": "...", "bytes": 123456,
 "width": 1920, "height": 1080}
```

The bridge maps it to an MCP `image` content block (`data`, `mimeType`) plus
`structuredContent` with `bytes`/`width`/`height` (the base64 is not duplicated in structured
content). `width`/`height` come from the PNG header (no image crate: a 24-byte parse).

### `clip_video` — `session_id`, `duration_s` (1..=30, default 5)

Manager runs the clip command for `duration_s` (SIGINT to finalize), reads the file/stdout,
and **mints the artifact** through `MediaStore::mint(session, "video/webm", bytes, audit.store_clips)`:

```json
{"capture": "media", "media_id": "<32 hex>", "mimeType": "video/webm",
 "bytes": 654321, "duration_s": 5}
```

The bridge maps it to a `resource_link` content block whose URI is **built at the edge**, on the
surface that served the call:

- `--listen` (network): `http://<Host header>/media/<media_id>` — the client's own Host, so a
  fronting proxy or a non-loopback bind keeps working; the id is the credential (A8).
- stdio: **open question Q1** below.

`structuredContent` carries `{media_id, bytes, duration_s, expires_in_s}` (never the URI alone,
so a harness can key on the id).

## Audit

- `audit.store_clips` → the clip's bytes are persisted by `mint(..., store_clips=true)` —
  one artifact, one capture.
- `audit.store_screenshots` → the PNG is persisted with `MediaStore::persist_screenshot(
  session, bytes)` — writes `<data>/media/<session>/<id>` (0600) **without** inserting a URL
  entry: a screenshot is not fetch-once media and never becomes a capability. Same 128 MiB
  per-session cap, oldest-first, same retention (08).

## Errors (tool errors, never protocol errors)

| case | code | message (pinned in fixtures) |
|---|---|---|
| session not visible to the scope | `session_not_found` | existing pinned string |
| command exits non-zero | `internal` | `capture failed: <command> exited <code>` |
| command exceeds `timeout_s` | `internal` | `capture failed: <command> timed out after {n}s` |
| PNG larger than `screenshot_max_bytes` | `bad_request` | `capture too large: {bytes} bytes (max {max})` |
| stdout empty / not a PNG | `internal` | `capture failed: no output` / `capture failed: not a PNG` |
| `duration_s` out of range | `bad_request` | `duration_s must be between 1 and 30` |

All four ride the existing `isError: true` tool result; the envelope is in `structuredContent`
as today.

## Tests

1. **Manager unit tests** (fixture command = `/bin/sh -c` writing a fixed PNG / webm):
   image payload shape, PNG width/height parse, `capture_too_large`, timeout, non-zero exit,
   empty output, `clip_video` mints and returns a 32-hex id, `store_clips` persists + no
   resurrection, `store_screenshots` persists a screenshot with **no** URL entry, scope
   isolation (`session_not_found` for a stranger).
2. **Bridge mapping tests**: image payload → `image` block; media payload → `resource_link`
   with `http://<Host>/media/<id>` (network) and the stdio policy; both keep structuredContent.
3. **One live leg**: real binary, fixture capture command, `screenshot` → image block bytes
   equal the fixture PNG; `clip_video` → id → fetch through `--listen` `/media/<id>` returns
   the fixture webm; second fetch `404 media_expired`.

## Open questions before implementation

- **Q1 (stdio resource_link policy).** The manager mints, but a stdio bridge has no HTTP
  surface for the caller to fetch. Recommending **(a)**: add `daiorama media fetch <id> --out
  <path>` (peer-credentialed on the manager socket, no new credential), and the stdio bridge
  returns `resource_link {uri: "daiorama://media/<id>"}` plus one text block naming the
  command. Alternatives: (b) `clip_video` over stdio is a tool error ("needs `--listen`"),
  (c) require `audit.store_clips` and return a `file://` URI. (a) keeps stdio usable and is
  ~80 lines; (b) is simplest but dead-ends embedded agents.
- **Q2 (config surface).** `capture.screenshot_command` / `capture.clip_command` /
  `capture.timeout_s` / `capture.screenshot_max_bytes` as above — or fixed commands with no
  config? Recommend config: the command is compositor-dependent (grim/wf-recorder today,
  something else for a future compositor).
- **Q3 (cap behavior).** `screenshot_max_bytes` over-budget → `bad_request` (no downscale).
  Recommend that; a downscale flag can come later with a region param.

On ack I implement the manager handlers + bridge mapping + the fixture-command tests, then the
live leg, and hand the head back.
