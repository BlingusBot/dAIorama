# 05 — MCP bridge and tool surface

## The bridge

Entry point: **`daiorama mcp`** — one entry, two modes; this is the command
every harness config points at (pinned).

- **stdio** (default): spawned per connection by a harness, ephemeral, dies
  with the connection. Scope arrives from the spawn environment:
  `DAIORAMA_SCOPE` (+ `DAIORAMA_SCOPE_TOKEN` if that scope has a secret); a
  bridge started without a scope refuses to run: `scope required: set
  DAIORAMA_SCOPE`.
- **network service** (`--listen <addr>`): one long-lived bridge, many
  connections. Scope is authenticated **per connection** — the connection
  presents a scope name and, where the scope has a secret, that secret; the
  manager validates it on the same presentation path as the instance token.
  No anonymous calls; no default scope. Scopes without a secret are usable via
  stdio only (07 states the model; rationale in 11).
- **Stateless w.r.t. sessions, always.** Bridge crash or harness restart costs
  nothing; sessions belong to the manager.
- **Reconnect, don't die:** bridge→manager and dashboard→manager connections
  reconnect with backoff on drops and surface the shared `manager unreachable
  at {addr}` state (04), never a raw connection error. On reconnect nothing
  needs re-syncing — session state structurally cannot be lost unless the
  manager itself died (03).

## Tool exposure model

Two questions, kept separate in design and code:

1. **Internal capability registry** — what dAIorama can do. Grows toward the
   full useful Hyprland IPC range: window management, input, clipboard, screen
   observation, query, program launch. No artificial cap once the plumbing exists.
2. **Exposed manifest** — what a specific connected caller may call. Per-scope,
   configurable by category or individual tool. Default: **deny-by-default** —
   nothing exposed until a scope's config grants it.

**One named exception to deny-by-default:** the always-on session-lifecycle set
— `create_session`, `close_session`, `list_sessions`, `session_status`, and
`list_capabilities` — scoped to the caller's own sessions by the normal scope
rules. Without it, an agent with nothing granted cannot even create the session
it needs (bootstrapping is not desktop control). Everything that touches or
observes a running session stays deny-by-default, including `get_view` and
`stop_view`.

**`list_capabilities`** (read-only, always-on): what the caller's scope may
call (grantable categories/tools it holds), plus its effective quotas and
audit flags. A fresh agent can discover what *could* be granted without string
matching error prose.

**Names are additive-only** (agents cache them; A7). Plain snake_case; no theme
words (02). Core names pinned here: the lifecycle set above, `get_view`,
`stop_view`, `screenshot`, `clip_video` (plus `clip_audio` when built).
Remaining control-tool names are pinned as each category lands; they may only
be added, never renamed.

**Starter essential set to build first:** window management, input, screen
capture (non-optional — it closes the observe-then-act loop), query, program
launch. Decorations, animation curves, monitor arrangement, dynamic
multi-monitor/resolution changes: later expansion, non-blocking.

## Views over MCP

- **`get_view`** (control manifest, deny-by-default): session-scoped,
  idempotent — starts a view if none is running (same on-demand mechanism as
  `--view`, PDEATHSIG child), returns its address either way. A discrete MCP
  call hands off to a standing stream: the realtime consumer then talks to the
  returned address directly and never touches MCP again for perception (05
  keeps this seam; continuous streams are not tool calls).
- **`stop_view`** (control manifest): stops the view only. Session unaffected
  (06).
- The returned address is the manager-owned front (07): token-gated beyond
  loopback, same rule as everything else — not a separate per-session credential.
- Audio has no standing stream yet (named v1 cut, 01). If built, same shape.

## Capture, media, and the content-type rule

Visual/audio data uses MCP's standard content types — no custom datatype:

- `screenshot` → an `image` content block (base64 + `mimeType`) in the result.
- `clip_audio` (later) → an `audio` block, same way.
- `clip_video` → a **resource link** to a manager-served media endpoint. The
  MCP spec has no video content type; a fetchable URI is the spec's own shape
  for artifacts too large to inline.

**Media endpoint (A8)**: the manager serves captured media on the same address
family as its RPC surface, path `/media/<opaque-id>`, under the same
exposure + token rule as every other surface (07). Lifetime, precisely:
the artifact is deleted at **first successful fetch or after TTL (default
60 s), whichever comes first** — that is what "ephemeral" now means. When a
scope's audit config enables persistence (`audit.store_clips`,
`audit.store_screenshots`), it is the same already-captured artifact that is
additionally persisted to the data dir — never a second capture — retained
under the history retention rules (08).

## Errors (machine-usable, A6)

Every failure is a stable envelope, identical over MCP and RPC:

```
{ "code": "<stable_code>", "message": "<human text, plain>", "hint": "<next action, optional>" }
```

MCP tool errors carry this envelope 1:1; **codes are a versioned API surface**
(additive-only, like tool names). Canonical codes:

- Session: `session_not_found`, `session_reaped`, `session_closed`, `session_dead`
  (reaped ≠ closed ≠ dead — distinct codes, distinct states, 06)
- Identity: `no_session_specified`, `scope_unknown`, `scope_auth_required`
- Permissions/limits: `tool_not_granted`, `quota_exceeded` (message carries scope + global numbers), `rate_limited`, `draining`
- Creation: `renderer_refused`, `backend_busy` (host exclusivity, 03)
- Protocol: `version_incompatible` (message carries both version pairs), `bad_request`, `internal`
- Transport (client-side presentation): `manager_unreachable` — the shared string in 04.

`hint` is the corrective step where one exists (e.g. "create a session first",
"close a session or raise the scope quota").

## List, history, and retries

- **Pagination**: `list_sessions`, history reads, and `list_capabilities`
  accept a `cursor` and return `next_cursor` — no unbounded result sets.
- **Idempotency**: `create_session` and control calls accept an
  `idempotency_key`. The manager deduplicates within a bounded window (10
  minutes or the session's lifetime, whichever is shorter) and returns the
  original result for duplicates. Documented retry rule: on
  `manager_unreachable` or a timeout, a client may retry **once** with the
  same key; without a key, retries are at-least-once and may duplicate (the
  reason the key exists — flaky connections are explicitly in the design, 03).

## Version handshake (A7)

- Both sides declare `{protocol_version: <semver>, min_supported: <semver>}`.
- **Comparison rule**: a connection is allowed iff
  `max(client.min_supported, manager.min_supported) <= min(client.protocol_version, manager.protocol_version)`
  — i.e. the supported ranges must overlap. Otherwise: `version_incompatible`,
  message carries both pairs (string in 04).
- Protocol version is its own semver, separate from the binary version.
- **The server cannot detect a newer client's unsupported call** under
  additive-only growth — only the client can pre-check. Clients pre-check
  before calling anything outside their known surface; managers reject
  out-of-range handshakes at connect.
- Additive-only applies to the RPC surface, error codes, and tool names.
- The dashboard never rejects a manager on version: it degrades per-manager
  via capability detection (10); a small compat table ships with release notes
  (09).
- Readiness: the manager exposes a lightweight readiness call (distinct from a
  full session list) returning state + component versions (binary, protocol,
  dashboard module) — the thing operators and the dashboard watch (C3).
