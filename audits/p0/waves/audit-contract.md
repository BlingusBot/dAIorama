# dAIorama — adversarial contract/consistency audit (wave: audit-contract)

*Lens: the machine-facing contract surface — 04 CLI, 05 MCP bridge/tools/errors, 02 glossary,
08 config/files, 10 operations — read against all of 00–11 and 90-appendix/design-audit.md.*
*Repo read-only (no edits under /home/agentuser/src/dAIorama); this file is the only write.*

Method: line-level cross-reference of every pinned name, flag, default and error string; independent
re-verification of every 2026-10-05 audit item (A1–A8, B1–B10, C1–C8, D1–D6) against the numbered
files; external spot-checks where 05 makes revision-relative MCP claims (sources inline).

Verdict: the ledger's "every item has a numbered home" claim holds for lifecycle, ownership, packaging
and ergonomics items — but **not** for the contract items. A1, A2, A6 and A7 are mis-resolved in
residual (F1/F9/F13, F7, F3/F4/F6/F8/F12, F2). Counts: **1 blocker, 11 major, 2 minor.**

---

## Findings

### F1 — `control` has no action vocabulary and no argument grammar — **blocker**
**Citation** — 04 "Commands/Sessions": "`daiorama control <session> <action> [args]` — one control
action, same action set as the MCP control surface (05)."; 05 "Tool exposure model": "Remaining
control-tool names are pinned as each category lands; they may only be added, never renamed.";
11 A1: "Openness is now limited to internal struct names, file paths, and algorithms."
**Problem** — the referenced "MCP control surface" contains no action names, only category names
(window management, input, screen capture, query, program launch — 05:57). `[args]` has no grammar
anywhere in the set. A1's own resolution forbids leaving a machine surface open, so this is a silent
re-introduction of the stance A1 rejected.
**Build impact** — the CLI parser, every MCP control tool's `inputSchema`, scope-manifest validation
(08:91 wants load errors "listing the valid names" — which do not exist), `list_capabilities`, and
the conformance fixtures (09:104) all depend on a vocabulary that is not in the spec. `control` is
hot-classified and in the frozen tree, so freeze would freeze an unimplementable command.
**Disposition** — before freeze pin (a) the argument grammar as one shape (e.g.
`daiorama control <session> <action> [--arg k=v ...]` or `[--json '<object>']`) and (b) the first
category's tool/action names and argument names, keeping additive-only for later categories. If that
is genuinely post-freeze, remove `control` from the pinned tree and say the first category's landing
pins its own names and grammar.

### F2 — A7's handshake is unpinned at both ends, and the MCP revision the bridge implements is never stated — **major**
**Citation** — 05 "Version handshake (A7)": "Both sides declare `{protocol_version: <semver>,
min_supported: <semver>}`." / "managers reject out-of-range handshakes at connect."; 04: "`daiorama
version` — core version + protocol version (skew display, C3)."; 05: "The MCP spec has no video
content type".
**Problem** — (i) no transport is stated for the handshake: nothing says it is a named first RPC or
MCP metadata, yet "reject at connect" presumes one; (ii) `min_supported` is surfaced by no operator
surface (04 `version`, 04 `status`, 05 readiness all omit it), so 05's "Clients pre-check before
calling anything outside their known surface" has nothing to pre-check against except the mismatch
string after the fact; (iii) edges undefined — missing/empty `min_supported`, inverted ranges,
prerelease ordering; (iv) the spec makes revision-relative MCP claims but never pins which MCP
revision `daiorama mcp` implements, or how the dAIorama semver handshake relates to MCP's own
version axis. External check: content types for MCP 2025-06-18 are text/image/audio/resource_link
(no video; 05's claim holds there), but the **current** MCP revision 2026-07-28 removed the
`initialize` handshake for stateless per-request `_meta` version declaration
(`io.modelcontextprotocol/protocolVersion`), added `server/discover`, and defines
`UnsupportedProtocolVersionError` (JSON-RPC -32022) — so "reject at connect" is legacy-era
behaviour and a bridge built from this spec must guess which era it targets.
(https://modelcontextprotocol.io/specification/2025-06-18/server/tools ;
https://modelcontextprotocol.io/specification/2026-07-28/changelog ;
https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
**Build impact** — the bridge is the one entry every harness config points at (04:52); the MCP era
decides handshake code, error mapping and `tools/list` shape, and modern-era harness failures land
outside any code the spec maps.
**Disposition** — state the supported MCP revision(s) plus era detection (stdio `server/discover`
probe, legacy `initialize` fallback) and map `UnsupportedProtocolVersionError` →
`version_incompatible`; define the dAIorama handshake as a pinned first call (name it) or state it
rides MCP metadata; add `min_supported` to `version`/`status --json`/readiness; add "min_supported
omitted ⇒ treated as equal to protocol_version".

### F3 — A6's "every failure is a stable envelope" is not total: four pinned refusal strings have no code — **major**
**Citation** — 04 "Refusal and error strings": "`token required for {addr}: set DAIORAMA_TOKEN …`",
"`Hyprland not found. Install it: {exact command for the detected package manager}…`"; 05 "Errors":
"Every failure is a stable envelope, identical over MCP and RPC".
**Problem** — 7 of 04's 11 strings map to codes in 05:107–113; four do not: `token required for
{addr}` (the network client's first auth failure), `refusing to bind {addr}: …`, `Hyprland not
found`, `Hyprland {found} is outside the supported range`. `no manager running` (04:100) is "an
answer, not an error" with no machine shape, and `bad_request` is listed but never given a usage
rule. Conversely most codes have no pinned `message`, while 02:32 makes messages part of the
machine contract.
**Build impact** — consumers switching on `code` — the point of A6 — must fall back to prose
matching on exactly the auth/dependency paths; A6's rejected "prose errors" outcome is partially
re-introduced.
**Disposition** — pick one and write it: add codes (`token_required`, `bind_refused`,
`hyprland_missing`, `hyprland_unsupported`) to 05's canonical list, or state in 05 that
process-level refusals are outside the envelope by design; give `bad_request` a usage rule; give
`no manager running` a stated JSON form.

### F4 — `session_closed` / `session_reaped` require a tombstone rule that is never stated — **major**
**Citation** — 05: "Session: `session_not_found`, `session_reaped`, `session_closed`,
`session_dead` (reaped ≠ closed ≠ dead — distinct codes, distinct states, 06)"; 06: "**closed** —
registry entry removed. Final."; 03: "the session registry always starts empty after the manager
restarts."
**Problem** — a removed entry cannot be told apart from a never-existing ID, so `session_closed` is
returnable only if closed IDs are remembered: retention, bound and restart behaviour are unstated.
Same for `session_reaped` — the FSM has no `reaped` state, and 06:99 ("the session ends with cause
`reaped` (error code `session_reaped`)") never says which call returns it.
**Build impact** — a builder either drops the distinction (breaking A6's promise and scripts that
separate closed/reaped from not-found) or invents an unbounded tombstone map in a long-lived
manager; tests cannot be written against undefined retention.
**Disposition** — one sentence in 06: "IDs of closed/reaped sessions are remembered for <N hours /
until manager restart>; calls inside the window return `session_closed`/`session_reaped`, outside it
`session_not_found`", plus the explicit reap path (reap marks the entry dead with cause; the sweep
closes it; calls while the entry lives return `session_reaped`).

### F5 — `list` default scope: 04 and 07 contradict each other — **major**
**Citation** — 04: "Default: the caller's own scope's sessions; `--all` for everything (operator
view)."; 07: "Reserved scope `operator`: sessions created from the CLI carry it; CLI reads are
operator-wide."
**Problem** — the CLI caller's scope is `operator`, whose reads are operator-wide, so either the
default already returns everything (`--all` undefined/redundant) or 07's sentence is wrong.
Second-order: `sessions.visibility = all` is described as how one MCP caller gets broader visibility
(07), but 08:36 puts it in the instance core config — one global switch — so one scope's grant would
flip for all scopes.
**Build impact** — the manager-side filter, the CLI default, `--all`, and history reads (same
scoping rule, 06:109) all key off this; agent visibility is script-visible and safety-relevant.
**Disposition** — decide explicitly: "CLI caller is the `operator` scope; `list` defaults to
operator-scope sessions; `--all` means <state it or drop the flag>"; move `sessions.visibility` into
scope files or state plainly that it is instance-global with that consequence.

### F6 — Retry/idempotency and pagination mechanics are under-specified — **major**
**Citation** — 05: "The manager deduplicates within a bounded window (10 minutes or the session's
lifetime, whichever is shorter) and returns the original result for duplicates."; 05:
"`list_sessions`, history reads, and `list_capabilities` accept a `cursor` and return `next_cursor`".
**Problem** — `idempotency_key` format/length/type and uniqueness domain (per scope? per session?
global) are unpinned; dedup-store persistence is unstated (03's empty-registry-after-restart plus
08's path table imply in-memory ⇒ a manager restart inside the retry window duplicates the desktop
the key exists to prevent — and MCP's current revision requires re-issuing requests after a broken
stream, so retries are the normal path); for `create_session` "the session's lifetime" has no
defined start/end; cursors: opacity, stability across registry mutations, expiry — all unstated, and
no CLI pagination story.
**Build impact** — the one-retry rule and the fake-manager/conformance fixtures (09:104) assert
nothing definite; every test encodes a guess.
**Disposition** — pin key = string ≤64 chars, unique per (scope, logical operation); state "dedup
state is manager-lifetime, lost on restart — clients must not retry across a manager restart" (or
persist it and name the file in 08); define the create_session window; add "cursors are opaque,
valid for N minutes and across registry changes; invalid cursor ⇒ `bad_request`".

### F7 — `config set` against a running manager: pickup timing and output contract unstated — **major**
**Citation** — 08: "**Core config: restart-only except where reloadable.** `config reload` (or
manager restart) re-reads the file."; 04: "Against `--remote`: RPC to that manager. Local, no manager
needed: operates on the files directly (cold)."
**Problem** — (i) nothing pins what a local `config set` says about when it applies — contrast
04:49, where `token rotate` must "state plainly" its timing; an agent setting `reaper.idle` on a
live manager gets no signal the value is inert until `config reload`/restart; (ii) "restart-only
except where reloadable" is resolved only by the next sentence (bind+token restart-only,
"everything else re-reads cleanly"); (iii) `config reload` is classified cold although it RPCs a
running manager and answers `no manager running` otherwise — the table's definition ("Cold = answers
from files/void") does not describe it; cold/hot is really never-starts / may-start.
**Build impact** — scripts that `config set` then act on the new value silently get old behaviour;
a builder must guess file-write vs RPC plus whether to emit a reload hint.
**Disposition** — add: "A local `config set` writes the file; a running manager keeps the old value
until `config reload` (or restart); `config set` prints that fact." Redefine the classes as
*may-start (hot)* / *never-starts (cold)* and note `config reload` needs a running manager to take
effect and never starts one.

### F8 — Machine-readable output shapes are unpinned (`--json`, cold answers, the MCP error carrier) — **major**
**Citation** — 04: "Read commands accept `--json`."; 05: "MCP tool errors carry this envelope 1:1".
**Problem** — no `--json` field names are pinned for `status`/`list`/session detail/errors (the
prose field lists in 04:26/35 are not a schema); which commands count as "read commands" is
undefined (`config get`? `token show`?); the cold answer `no manager running` has no stated machine
shape; and the envelope's carriage on MCP is unspecified — MCP defines two mechanisms (protocol
errors with numeric JSON-RPC codes; tool-execution errors with `isError: true`), and dAIorama's
string codes map to neither. MCP 2025-11-25 (SEP-1303) clarified validation errors belong in
tool-execution errors, so the choice is load-bearing.
(https://modelcontextprotocol.io/specification/2025-06-18/server/tools ;
https://modelcontextprotocol.io/specification/2025-11-25/changelog)
**Build impact** — the CLI is "a thin RPC call" (03) and C7's fixtures need concrete shapes;
harnesses will parse whatever the implementer invents.
**Disposition** — pin one JSON key set (session object, status object, error object); state that
`--json` errors emit `{code,message,hint}` verbatim and cold commands emit a stated shape; state the
MCP carrier — `isError: true` + `structuredContent` = the envelope (plus a text block), JSON-RPC
protocol errors reserved for malformed MCP requests.

### F9 — 04's pinned surface has four gaps (dashboard serve, install `--version`, `--manager`, history reads) — **major**
**Citation** — 04: "`daiorama dashboard serve` — serve the dashboard (module must be installed).";
04: "`daiorama mcp [--listen <addr>] [--manager <addr>]`"; 04: "`--remote <addr>` … One flag, one
meaning, everywhere."
**Problem** — (a) `dashboard serve` is in the command tree but in none of the cold/hot/neither rows
(04:86–88), so whether it may lazily start a manager is undecidable; (b) `dashboard install
--version <v>` exists in 09:34 and not in 04:57, so 04 is not the single pinned surface A1 promises;
(c) `--manager` on `mcp` duplicates the global `--remote` meaning for the one command every harness
config points at — either `--remote` does not apply there ("everywhere" false) or two spellings
exist for one meaning (the flag form of A1's stop-verb drift); (d) history is advertised as a
queryable record (03:28, 05:120, 10:72) but neither a CLI command nor an MCP tool name reads it.
**Build impact** — each gap is a guess that yields incompatible harness configs/scripts; A1's remedy
was precisely "no guesses".
**Disposition** — add `dashboard serve` to the "neither" row; add `--version` to 04:57; delete
`--manager` (make `--remote` the one flag) or define the difference; pin the history-read surface or
state it is dashboard-phase-only.

### F10 — Readiness call vs `status`: two field sets, claimed to be one contract — **major**
**Citation** — 05: "the manager exposes a lightweight readiness call … returning state + component
versions (binary, protocol, dashboard module)"; 04: "state, mode (attached/background), pid, socket
path, core + protocol versions, session counts, drain state."; 10: "poll the readiness call (05);
humans use `daiorama status` (04). One readiness contract, not per-integrator probing."
**Problem** — the field sets differ (readiness has no drain state, pid, mode, counts; status has no
dashboard-module version); nothing states their relation or the readiness call's surface/method
name; upgrade automation watching readiness cannot see drain state.
**Build impact** — watchers, container healthchecks and the dashboard status pane code against one
of two shapes; mismatch is a runtime surprise.
**Disposition** — declare `status --json` the readiness contract and the readiness call its RPC form
(superset = 04:35 fields + dashboard module version), or enumerate both lists and state
readiness ⊆ status; name the call/route.

### F11 — Runtime-dir fallback path is unpinned; the container equivalent drops `<instance>` — **major**
**Citation** — 04: "manager socket | `$XDG_RUNTIME_DIR/daiorama/<instance>/manager.sock`"; 08: "it
creates a fallback equivalent (same permission model) before anything else"; 08: "container
equivalents | `/config`, `/data`, `/run/daiorama`".
**Problem** — the fallback is deliberately unnamed, but the CLI must compute the same path to
connect; with `XDG_RUNTIME_DIR` unset — the ordinary container case this product targets — the
pinned default is unusable and two implementations can choose different fallbacks. The container
equivalent also omits the `<instance>` segment C8's instance-discrimination exists for.
**Build impact** — the socket path is the core local contract; "works from the manager but not the
CLI" is the classic failure of an unpinned path.
**Disposition** — pin the fallback (e.g. `$TMPDIR/daiorama-<uid>/<instance>/manager.sock`, 0700) and
write the container equivalent with the instance segment (`/run/daiorama/<instance>/manager.sock`).

### F12 — 02's "strings are written once, in 04" is violated; code `message`s are mostly unpinned — **major**
**Citation** — 02: "Error and status strings are written once, quoted in this spec (04), and reused
verbatim by CLI, bridge, and dashboard"; 05: "refuses to run: `scope required: set
DAIORAMA_SCOPE`."; 09: "`daiorama dashboard serve` refuses with a clear \"module not installed, run
dashboard install\" when absent."
**Problem** — two pinned-looking strings live only in 05/09, so 04's list is not the single source
and string-matchers will miss them; and while 02 makes `message` part of the contract, only 04's 11
strings are pinned — the other canonical codes carry free-text messages. Since 04:112 freezes
"final wording" at sign-off, which strings are frozen must be decidable.
**Build impact** — an agent grepping the canonical file for the module/scope refusals finds nothing;
a builder cannot tell which prose is contract.
**Disposition** — add both strings to 04's list with codes (reuse `scope_auth_required` for the
missing-scope case or add `scope_required`; add `module_not_installed`), and either pin a `message`
per code or amend 02 to "codes plus the strings in 04 are contract; `message` is explicitly
non-normative".

### F13 — Values referenced but never pinned — **minor**
**Citation** — 04: "`--wait` blocks until zero running, bounded by `--timeout <secs>`."; 04:
"session limit reached: scope '{scope}' {n}/{max}; global {g}/{gmax}."; 04: "Hyprland {found} is
outside the supported range {min}-{max}."
**Problem** — (a) no default for `--timeout`, nor whether `--wait` without it blocks forever; (b)
the global session ceiling has no key name (`gmax` is a placeholder; `max_sessions` at 06:52 never
becomes a defined key or location); (c) no key name for media TTL (04:75 pins the value; 08:36 names
a `media.*` family); (d) the Hyprland compatible range is unpinned while its error string freezes at
sign-off; (e) enum/duration value grammar unstated (`manager.start_mode = auto|attached|background`,
`limits.mode = off`, `reaper.idle`) — RON string vs bare symbol, duration units — yet `config set
<key> <value>` is a string-matched surface; (f) 09's "Signed per-release manifest" names no
signature scheme or verification rule, though install verifies "checksum from the manifest".
**Build impact** — scripts cannot bound `drain --wait`; invented key names become de-facto
contract; frozen strings interpolate unpinned constants.
**Disposition** — add the timeout default to 04's table; name the keys (`sessions.max_global`,
`media.ttl`, quota-profile fields) and the enum/duration value forms; state the Hyprland range is
fixed at spike exit (09) or pin it; name the manifest signature scheme and its verifier.

### F14 — View-operation error contract gaps — **minor**
**Citation** — 04: "`daiorama view <session> [--stop]` — print the view address (starting the view
if needed…), or stop it."; 05: "`stop_view` … stops the view only. Session unaffected (06)."
**Problem** — no code/string for view-start failure (wayvnc unusable, `view.port_base` range
exhausted, front bind refused); no stated result for `view --stop`/`stop_view` with no view running
or `stop_view` on a dead session whose view was killed with it (06:83); a re-fetch of a media
artifact after first-fetch/TTL deletion (05:88) has no stated response. (Front credential mechanics
are the security wave's item — cross-reference, not duplicated here.)
**Build impact** — the dashboard ("view down ≠ session dead", 10:100) and CLI must return these
outcomes; agents hit this on the first `get_view` against a broken session, where A6 promised an
envelope.
**Disposition** — pin: start failure ⇒ named code (`view_unavailable`) + message, or `internal`;
`--stop`/`stop_view` with no view ⇒ success (idempotent), stated; dead session ⇒ `session_dead`;
expired media ⇒ a stated 404 shape / `bad_request`.

---

## Verified OK (ledger items A1–A8, B1–B10, C1–C8, D1–D6)

- **V1 — A3**: complete surface enumeration including the previously missing bridge, and a named
  view-front enforcer (07:10–17, 29–35); request-time gate rule. External check consistent: wayvnc
  serves no noVNC page, a companion front is required by construction
  (https://github.com/novnc/noVNC; appendix sources).
- **V2 — A4**: scope model closed and consistent across 05/07 (stdio = env at spawn; network =
  per-connection name + secret; unix = cooperative and documented; no-secret scopes stdio-only).
- **V3 — A5**: one token rule, both halves of the old contradiction gone (generate on first
  non-loopback bind; refuse only when generation impossible); 0600 file; print-once + `token show`;
  rotate's next-start and drops-sessions consequences stated in 04/07/10.
- **V4 — A8**: media endpoint `/media/<id>`, first-fetch-or-60 s, same exposure rule,
  `audit.store_clips` persists the same artifact (05:86–94; 07 table row).
- **V5 — B1, B3, B4**: reaper activity definition + watched exemption + idle-since + reap≠close
  (06:87–103; caveat F4); FSM + `cgroup.kill` + no auto-restart + dead-doesn't-count, with
  11:119–128 recording the reconciliation of the audit's two clauses; PDEATHSIG
  direct-children-only + `getppid()` re-check + non-ignorable signal (03:37–45).
- **V6 — B2, B5, B6, B7**: view registry `{state,address,port,generation}` + per-session lock +
  kill-on-session-death (06:62–85); cold/hot table with every command classified except
  `dashboard serve` (F9) + flock + bounded wait + retry-once (04:78–92, 10:15–18); device-global host
  exclusivity with `backend_busy` naming the incumbent (03:89–97, 06:20–24); interop identity probe
  + explicit attachment + non-adoptable compositor sockets (07:87–100).
- **V7 — B8, B9, B10**: history records carry scope, same read scoping, dashboard a named
  never-default scope (06:105–112); B9 correctly left **open** with a must-decide-before-ship home
  (07:102–109, 11:166–170) — tracked, not omitted; pull-only v1 with the false "no coupling" denial
  removed (03:27–33, 11:172–179).
- **V8 — C1, C2, C6, C7**: dashboard-install mechanism (per-triple asset, version-matched,
  checksum, atomic symlink, deterministic revert, `--from`, container case) and release manifest (one
  core binary incl. bridge, separate module, client-only mode, macOS scope, digest pinning) — with
  `--version` flagged in F9; cgroup preconditions + fail-loud + `limits.mode = off` (06:114–131);
  spike-harness exit criteria (09:98–118).
- **V9 — C3, C4, C5, C8, D1, D2, D3, D4, D5, D6**: upgrade procedures + drain + skew display
  (10:33–49); host-runnable `doctor` (10:53–55, 09:91–96); deps table + in-binary capture + named
  browser (09:75–89); exact path table (08:9–18; caveat F11); first-run sequence (10:51–65); status
  fields + `end`/`--force` + one reachability string (04:34–35; 11:251–255); dashboard flows incl.
  take-over/rename/view-down (10:86–101); scope ergonomics + validation (08:86–99); history defaults
  + RON rationale (04:73, 11:271–279); llvmpipe fork decided (06:11–18, 04:103).
- **V10 — A1, A2, A6, A7**: substantially executed but **mis-resolved in residual** — A1 →
  F1/F9/F13; A2 → F7; A6 → F3/F4/F6/F8/F12; A7 → F2. No other rejected alternative is silently
  re-introduced (checked all *Rejected* lines in 11 against 01–10).

## Needs runtime test

- **Idempotency dedup across manager restart** — create a session, kill the manager inside the
  window, retry with the same key; assert no duplicate desktop (or document the duplicate).
- **Cursor stability** — page `list_sessions`, close a session between pages, resume; assert the
  rule once F6 is written.
- **Concurrent `get_view` / stale-address detection** — two simultaneous starts (one view only,
  loser gets winner's address); view crash then cached address ⇒ generation bump.
- **Readiness vs `daiorama status --json`** — field sets and the published route once F10 is
  resolved; run a container healthcheck against it.
- **MCP era negotiation** — against the harnesses actually in use (modern 2026-07-28 per-request
  `_meta` vs legacy `initialize`); determines F2's support matrix.
- *(Already tracked in 11's open items — spike chain, headless path, GPU recipes, cgroup delegation —
  not re-listed.)*
