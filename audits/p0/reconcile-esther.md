# P0 reconcile — Esther side (draft — joint diff pending)

Reconciles `audits/p0/blingus-findings.md` @ `8571e3b` (39 findings: 5 blockers /
27 majors / 7 minors). Format per request: `F# → adopt / reject / counter +
replacement text`. Everything below is a **draft decision** — nothing commits to
the freeze anchor until the joint diff lands.

## Re-verification before signing (not on the findings' word)

- **MCP 2026-07-28 changelog** — fetched, confirmed: `initialize` handshake
  removed (per-request `_meta` versioning, `io.modelcontextprotocol/protocolVersion`),
  `server/discover` required and usable as the STDIO back-compat probe,
  `UnsupportedProtocolVersionError` = **-32022** (renumbered from -32004 in this
  revision; matches the finding), all
  results carry `resultType`, broken streams are re-issued as new requests
  (retries are the normal path — feeds F12/F14).
- **Fedora hyprland package** — fetched: f41 (0.44.1-1) / f42 (0.45.2-1) only,
  maintainer `orphan`, no activity. Same conclusion as the finding; the fix
  below assumes no distro artifact for f43+.
- **Both reference boxes' cgroupfs read-only** — his evidence + live check on my
  container: `cgroup2 ro`, `mkdir → EROFS`, controllers here = `cpu memory pids`
  (his box also carries `io`). Containment notes account for the variance.

## Dispositions overview

No rejects. Adopts carry pinned replacement text. Two items diverge in
direction from the finding's own suggestion and are called **counter** (F13 —
choice made: pin at freeze, vocabulary supplied; F29 — no view relay in v1,
exposure model instead). F6/F7 get extra tightening (startup-refusal semantics,
compose-carries-wiring, sweep placement); F28/F30 get reframed, not flipped.

---

## A. Renderer & spike chain

### F1 → adopt + pin (blocker)

Pins: the override key, the re-verification mechanism, the 03 reword, the CI
substrate caveat.

- `04 §Pinned defaults` — add row: `renderer.allow_software | off`.
- `06 §Creation` — after the software-refusal paragraph:

  ```text
  `renderer.allow_software` (manager config, default off, no CLI flag, never
  runtime): when on, creation proceeds on a detected software renderer and the
  session is marked `renderer: software` in its records — the override exists
  for protocol-test lanes; it is deliberately not reachable through any
  runtime call.
  ```

- `06 §Creation` — replace the bare "re-verified at first browser launch" with
  the mechanism:

  ```text
  Re-verification has an executor: browser launches go through the session
  launcher (shipped in the image at a fixed path; the documented launch route),
  which captures the browser's own rendered-GL renderer string from its logging
  channel into a manager-readable file in the session runtime dir; the manager
  reads it and a software result marks the session `renderer: software` and
  raises the unmissable warning. Where the launched browser bypasses the
  launcher (a bare-host browser that was never routed through it), the session
  stays `renderer: unverified` — it is never silently marked verified.
  ```

- `03 §GPU access` — last paragraph: "no software-only fallback path" → "no
  **supported** software-only path" (C7's CI lane depends on software being
  possible for tests).
- `09 §Spike` — add exit criteria: the override end-to-end; the launcher
  capture mechanism; and the substrate caveat: if the chosen build cannot run
  on llvmpipe at all (upstream #3516), the CI lane changes substrate (nested
  compositor or fixtures-only protocol tests) and says so.

### F2 → adopt

`03 §Backend modes`, `headless` bullet — reword:

```text
headless = render-node-only DRM init as the container prerequisite:
AQ_NO_KMS_REQUIREMENT=1 (makes render nodes selectable; AQ_DRM_DEVICES pins
the node where several exist), with the dedicated virtual output
(hyprctl output create headless) as the output-creation mechanism layered on
top for resolution/scale. Spike verifies the combination plus the wayvnc
capture/resize path.
```

### F3 → adopt + pin

- `09 §Release manifest` (or the bare-host deps section): install-command
  resolution is a maintained table in the release manifest — distro/release →
  exact command → version, with explicit "not packaged / out of range" rows.
  Where the row is "not packaged" (the Fedora ≥ f43 case, verified), the 04
  string renders the container-image pointer instead of a failing `dnf …`:

  ```text
  Hyprland not found. Install it: {resolved command — where the distribution
  has no package this renders "not packaged for this distribution; a
  COPR/upstream build is noted in the release notes"}, or use the container
  image.
  ```

- The `{min}-{max}` range values: pinned at spike exit against the installed
  artifact; recorded in the P0-exit open-items list with deadline **P1 exit**.

### F4 → adopt

`09 §GPU recipes` — restate vendor-agnostically:

```text
Render node (any /dev/dri/renderD* — AMD, Intel, NVIDIA-on-nouveau, …):
--device /dev/dri/renderD128 --group-add keep-groups … (ls -l on the target
is the source of truth). CDI (--device nvidia.com/gpu=all; host needs
Container Toolkit ≥ 1.18) is the NVIDIA-proprietary alternative only.
```

`doctor` matrix gains the nouveau row: nouveau + no toolkit → render-node
recipe; CDI not applicable.

### F5 → adopt

- `06` — one sentence: each session gets a generated compositor config
  (resolution/scale/env) under the session runtime dir, written before exec;
  runtime-only changes go through the control surface's dispatch.
- `09 §Version-check pattern` — the gate covers the config dialect as well as
  the IPC surface.

---

## B. Containment & lifecycle

### F6 → adopt + pin (blocker)

Both boxes fail C6 by default (verified; wiring is available, not default —
host `user@…service` delegates `cpu io memory pids` on his box; `cpu memory pids`
on mine). Replacement text pins: code, string, placement, wiring, loser outcome,
and the `off`-mode caveat in one place.

- `05 §Errors` — add code `limits_unavailable`.
- `04 §Refusal strings` — add:

  ```text
  containment unavailable: cgroup v2 delegation is not available here
  ({detail}). Fix: run with a writable/delegated cgroupfs (see the compose
  notes), or set limits.mode = off and accept losing per-session limits and
  crash-time cleanup.
  ```

- `06 §Per-session resource limits` — replace "fail loud …" tail:

  ```text
  The check runs at manager startup, after the runtime lock, before the
  manager binds anything; failure fails the start with `limits_unavailable`
  (string above) — every lazy-start racing the winner surfaces the same
  error; no retry loop (the failure is deterministic). `daiorama doctor`
  performs the same check and prints the fix. limits.mode = off is the
  explicit, recorded downgrade — and it is honest about its cost: no
  per-session limits and no crash-time cgroup.kill; cleanup reduces to the
  parent-death signal (direct children) plus the startup sweep, so
  grandchildren may outlive a manager crash.
  ```

- `09 §Container image` — every shipped compose snippet carries the working
  cgroup wiring (validated in the spike; the flagship snippet passes C6 out of
  the box on hosts that support delegation, instead of failing OOTB).

### F7 → adopt + pin

- `03 §Ownership seams` — add the sweep, worded so it cannot read as registry
  reconciliation:

  ```text
  Session cgroups live under an instance-owned subtree. On manager start,
  before binding, the manager sweeps that subtree: session cgroups left by a
  previous manager run are cgroup.kill'ed. The registry cannot be reconciled
  — sessions died with their manager — but cgroups and orphaned process trees
  can be, and are; that is the only reconciliation the design promises.
  ```

- Implementation note (03): PDEATHSIG binds to the forking thread — children
  are forked from the process whose death is the trigger (the re-check covers
  the fork window).
- `10 §Starting` — supervision: run the manager as a **systemd user service**
  (KillMode=control-group default — unit stop sweeps the whole tree).
  `systemd-run --user … -p Delegate=yes` is the delegation path and a *scope*: it
  does not sweep on manager death; say so. `daemon start` (no supervisor) is
  dev convenience.

### F8 → adopt

- `06 §Session state machine` + `04` defaults: split the key —
  `reaper.sweep_dead = on` as its own default, independent of `reaper.idle`
  (table row added; sweeping a dead entry changes nothing operators rely on).
- History retention keys to session **end** (dead/reaped/closed, whichever
  occurs first), never "after close" — a session whose sweep is delayed still
  ages out.

### F9 → adopt

`04 §Cold vs hot` — hot = `new` only; `close`, `control`, `view`, `drain`,
`daemon end` move to cold ("never-starts"; a session-scoped command with no
manager gets the honest `no manager running`). Startup line stays `new`-only.

### F10 → adopt

`02/06` — IDs are globally unique and opaque (UUIDv4), never reused across
restarts; session records and create results carry the manager's `instance_id`,
so a cached ID reads `session_not_found` (hint: previous manager run) at worst —
never a different desktop.

### F11 → adopt

`06` — one sentence: closed/reaped IDs are remembered as tombstones for 24 h
(bounded; cleared on manager restart): inside the window → the distinct code;
outside → `session_not_found`. Reap path stated explicitly: reap marks dead
with cause `reaped` (calls while the entry lives → `session_reaped`); the
dead-sweep closes it (tombstone applies).

### F12 → adopt

`05 §Retries` —

```text
idempotency_key: string ≤ 64 chars; unique per (scope, logical operation).
Dedup state is manager-lifetime, in-memory, lost on restart — after a
restart the same key may re-execute (the original session died with its
manager; create results carry instance_id so this is visible, never silent).
The create window runs from receipt until the created session ends.
```

Cursors: opaque, valid across registry changes (deletions shorten pages), expire
with the manager; invalid ⇒ `bad_request`. CLI: `list` gains `--limit`/`--cursor`
(prints `next_cursor`).

---

## C. Contract surfaces

### F13 → counter (choice made: pin at freeze; vocabulary supplied) (blocker)

The finding offered "pin now, or remove `control` from the frozen tree". I take
the pin path — the input category is what P1's spike exercises, and a frozen
tree with an unimplementable command is the worse failure.

- `04` — grammar:

  ```text
  daiorama control <session> <action> [key=value ...] — one control action;
  <action> is an MCP control-tool name (05); key=value pairs are that tool's
  named arguments, typed per its inputSchema; results print like other read
  commands (--json supported).
  ```

- `05 §Tool exposure` — the first landing category's names, frozen here
  (additive-only for later categories, per the standing rule):

  ```text
  input — pointer_move {x, y} (absolute, session output coordinates),
  pointer_click {button: left|right|middle}, pointer_scroll {dx, dy},
  key_tap {key}, key_down {key}, key_up {key} (xkb keysym names),
  type_text {text}.
  ```

- Unknown action ⇒ `bad_request`, message lists the valid names.
- Fallback if the joint prefers deferral: delete `control` from 04's tree and
  move this rule to the landing amendment — recorded here so the flip is cheap.

### F14 → adopt + pin

- `05 §Version handshake` — dAIorama's own handshake gets a name and a place:

  ```text
  Every manager connection opens with the `handshake` call (always first,
  tokenless on any transport): {protocol_version, min_supported} in,
  {protocol_version, min_supported, instance_id} out; the overlap rule
  stands; managers refuse out-of-range handshakes there. min_supported
  omitted ⇒ treated as equal to protocol_version; inverted/empty ranges ⇒
  version_incompatible.
  ```

  `version`, `status --json`, and readiness surface `min_supported`.
- `05 §The bridge` — MCP revision pinned:

  ```text
  The bridge implements MCP revision 2026-07-28 (per-request `_meta` version
  declaration; `server/discover`), and the legacy `initialize` negotiation for
  earlier revisions; on stdio it probes with `server/discover` first.
  UnsupportedProtocolVersionError (-32022) maps to version_incompatible with
  both pairs in the message.
  ```

### F15 → adopt

- `05 §Errors` — add codes: `token_required`, `bind_refused`,
  `hyprland_missing`, `hyprland_unsupported` (messages = the 04 strings,
  verbatim), `no_manager_running`; and the `bad_request` rule: malformed
  request only (schema/grammar/unknown keys/unknown cursor) — semantic
  refusals always carry their own code.
- `02` — contract scope fixed: codes + the strings quoted in 04 are contract;
  `message`/`hint` for codes without a 04 string are non-normative prose.

### F16 → adopt + pin

- `04` — `--json` prints the RPC response shape unmodified; the envelope
  `{code, message, hint?}` is emitted verbatim for errors; cold answers emit
  `{"code": "no_manager_running", "message": "no manager running"}` (exit 0 —
  it is an answer).
- `05` — pinned shapes: session object `{id, label, state, backend,
  resolution, scale, scope, origin, idle_since, view:{state, address,
  generation}|null, instance_id, renderer}`; instance status `{state, mode,
  pid, socket, versions:{binary, protocol, min_supported, dashboard_module?},
  sessions:{running, dead}, drain:{active, remaining}, instance_id}`; list
  page `{sessions:[…], next_cursor}`.
- MCP carrier: tool-execution error (`isError: true`) + `structuredContent` =
  the envelope; JSON-RPC protocol errors reserved for malformed MCP.

### F17 → adopt

- `04` — `dashboard serve` → "neither" row; `dashboard install [--version <v>]`
  added; `--manager` deleted from `mcp` (`--remote` is the one flag; `mcp`
  targets the local manager by default); history reads pinned: CLI
  `daiorama history [<session>] [--scope <name>] [--limit] [--cursor]` +
  MCP `list_history` (grantable, same scoping rule).

### F18 → adopt

`05/10` — readiness **is** `status --json`'s RPC form: the call is named
(`readiness`), returns 04's field set plus `dashboard_module`; one contract,
stated once.

### F19 → adopt

- `08` — fallback pinned: `$TMPDIR/daiorama-<uid>/<instance>/manager.sock`
  (0700 dirs); container equivalents gain the instance segment:
  `/config`, `/data`, `/run/daiorama/<instance>/`.
- `04` — socket default line notes the fallback.

### F20 → adopt

- `04 §Refusal strings` — add: `scope required: set DAIORAMA_SCOPE.` (code
  `scope_required`) and `module not installed; run 'daiorama dashboard
  install'.` (code `module_not_installed`); 05/09 reference 04.

### F21 → adopt

- `04 §config set` + `08`: "A local `config set` writes the file; a running
  manager keeps the old value until `config reload` (or restart); `config set`
  output states which of the two applies."
- Classes redefined: hot = **may-start**; cold = **never-starts**. `config
  reload` is cold-class (never starts a manager) and says `no manager running`
  when none runs — the file still applies at next start.

### F22 → adopt + pin

- `04` — `list` line: CLI runs as the `operator` scope; operator's visibility
  is operator-wide (its scope file ships `sessions.visibility = all`); `--all`
  **dropped**.
- `08` — `sessions.visibility` moves into scope files (per-scope key,
  default `own`); no instance-global switch, so one scope's grant can never
  widen another's.

### F23 → adopt + pin

- `04` defaults: `drain --timeout` default `300` (seconds); keys named —
  `sessions.max_global` (core), per-scope `sessions.max`, `media.ttl` (core,
  seconds), quota-profile fields listed with the scope-file example.
- Value grammar (04): integers; durations `<n>s|m|h`; enums literal (`off`,
  `auto`, `attached`, `background`, `pace`, `reject`, `own`, `all`);
  booleans `on|off`.
- `09` — manifest signing: **minisign** signature verified with the embedded
  release key before digests are trusted.

### F24 → adopt

- `05` — code `view_unavailable` (wayvnc unusable / port range exhausted /
  front bind refused) with a 04 message; `--stop`/`stop_view` with no view = idempotent
  success; a view op on a dead session ⇒ `session_dead`; expired media ⇒ 404 +
  `media_expired` (F31).

### F25 → adopt

`10 §Diagnostics` — sanctioned detachment: systemd user service (stderr →
journald) or container main process (`serve`); `daemon start` is dev
convenience with terminal-scoped stderr — "no bespoke log files" stays, and
the no-sink case is stated, not omitted.

---

## D. Network, auth, views, media

### F26 → adopt + pin (blocker)

- `07 §Scope model` + `05 §The bridge` — presentation pinned:

  ```text
  A network MCP endpoint carries credentials as request headers:
  Daiorama-Token (instance token), Daiorama-Scope, Daiorama-Scope-Token.
  The bridge relays them unmodified — it is a presentation relay, not a
  token authority; the manager validates on every call, same path for both
  hops. Failures are MCP-level errors (token_required, scope_auth_required,
  scope_unknown) — never an OAuth challenge or discovery metadata.
  ```

- Fallback shape pinned: harnesses without custom-header support use the
  client-only install beside themselves — either stdio
  `daiorama mcp --remote <manager>` or a loopback `--listen` bridge — with
  credentials in env (`DAIORAMA_TOKEN`, `DAIORAMA_SCOPE`,
  `DAIORAMA_SCOPE_TOKEN`); no new harness capability required.

### F27 → adopt + pin (blocker)

- `07` — the two layers, stated: the instance token is the **transport gate**
  and confers no scope. Unix socket: CLI's `operator` claim stays cooperative
  (local trust, unchanged). TCP: the CLI authenticates as `operator` with that
  scope's secret (`DAIORAMA_SCOPE_TOKEN`; scope fixed `operator`);
  `scope_auth_required` when absent. The starter scope set includes `operator`
  and `dashboard`, secrets generated 0600 at `config setup` / first
  non-loopback bind; `token show` prints the reminder.

### F28 → adopt + pin

- `07 §exposure rule` — replace "cookie or one-time code":

  ```text
  The front mints a per-view bearer credential:
  credential = HMAC-SHA256(instance_token, "view:" || session_id || ":" ||
  generation). Multi-use; valid only for the current generation (dies with
  stop/death structurally); revoked on stop_view. view/get_view return the
  front URL with the credential as a query parameter (and as a separate
  `credential` field in JSON output); session detail/status never echo it.
  The front gates every request — page, assets, and the WebSocket upgrade;
  on the first credentialed page load it sets its own HttpOnly session cookie
  scoped to the front origin so the noVNC page's asset/WS traffic is covered.
  The WS upgrade validates Origin. The served page's WS target is pinned
  server-side (client-supplied host/port/path params ignored). One-time codes
  are dropped from v1 — multi-consumer reality (dashboard + programmatic
  consumer + human) is the norm this flow must serve.
  ```

### F29 → counter (no relay in v1; exposure model)

The finding's fork was "bridge-proxy `/view/…` or state plainly". I take **state
plainly, and do not build the relay**: a view WS relay is the deferred
relay/tunneling mechanism (01 non-goal — "deliberately not built now"); views
are the most sensitive surface, so explicit operator exposure is the v1 posture,
not a silent proxy. Media (one-shot fetch, F31) stays the single relayed path —
different kind of traffic, in the bridge's existing request path.

- `06 §Views` — registry becomes `{state, address, front_port, vnc_port,
  generation}`; `front_port` allocates from `view.port_base`; wayvnc gets an
  ephemeral loopback port (manager-known; not part of the address contract).
- `05 §Views over MCP` + `07` — reachability, stated:

  ```text
  The front binds loopback by default. Serving consumers outside the
  manager's network namespace is an explicit operator act: expose the front
  (bind + publish the range, 09) and set `view.advertise_base` — the URL base
  external consumers should use; view/get_view build returned addresses from
  it when set. Without it, returned addresses are usable only inside the
  manager's namespace. Views are not relayed in v1; the dashboard-phase relay
  remains the provisioned future path.
  ```

- `08` — `view.advertise_base` added to the `view.*` family (manager-side).
- `09` — compose snippets that serve remote viewers publish `6080-6099` and
  show the two config values.
- Sign-off shortlist: this is the flip candidate — if Ygypt wants zero-config
  remote viewing in v1, the bridge proxy is the alternative and costs a WS
  relay build.

### F30 → adopt + pin (accept, explicitly)

`07 §Scope model` gets the sentence (and 11 records it):

```text
View and media credentials are instance-scoped, not scope-scoped: a caller
admitted to the instance can observe any session whose id it knows. Scopes
constrain control, listing, and history; observation is gated at the
instance level — and by whether the relevant surface is exposed at all (F29).
```

The per-(scope, id) credential fix is the named alternative, deferred — with
F29's exposure model it is a smaller residual than it reads.

### F31 → adopt (media = the one relayed path)

- `05 §Capture/media` + `07` table row — media endpoint pinned:

  ```text
  The manager mints the resource_link as a single-use, TTL-bounded capability
  URL on the surface that served the call; for network-bridge calls the
  bridge relays /media/* pass-through (no validation there — the id is the
  credential). 128-bit opaque id. Deleted after the response body completes
  (an open fd keeps serving concurrent readers); later fetches ⇒ 404 with
  media_expired. First-complete-wins; no Range/resume (a completed first
  fetch is final). TTL default: 600 s.
  ```

- `04` defaults table: media TTL 60 s → 600 s (fetch-once rule unchanged).
- Persisted copy vs fetchable copy diverge → stated in 05/08: persistence
  never resurrects the URL.

### F32 → adopt

- The tokenless probe **is** the `handshake` call (F14) — ordering pinned: it
  is strictly first; credentials only after it.
- `07 §interop` — instance ID at `<data>/<instance>/instance-id` (0600),
  returned by `handshake`; first `--remote` attach records a TOFU pin in the
  client config; mismatch refuses with a distinct 04 string; re-pin = explicit
  client-config edit. The trust decision is the credential + operator choice —
  the probe is a routing aid.

### F33 → adopt

`07 §interop` + `10` ops note — one sentence: mounting the runtime dir grants
the mounter **instance trust** with cooperative scope claims; that is a
deliberate trust decision, not a side effect. Shared instances point at TCP,
not a mount.

### F34 → adopt (option a)

- `07` dashboard row: v1 refuses non-loopback binds (remote humans use SSH
  forward / overlay); its manager connections carry per-connection tokens.
- Broad-read key named: `sessions.visibility = all` in `scopes/dashboard.ron`
  (per F22, it lives in the scope file).

### F35 → adopt + pin

`07 §Rate limiting` — mechanism lands (B9 moves open → adopted; the
must-decide-before-ship gate becomes verify-before-ship):

```text
Per-session token bucket over control-class calls (input class; reads
exempt). Scope-file config: limits.rate { per_sec = 20, burst = 40,
mode = pace|reject }. Default pace (delays excess calls to bucket rate in
arrival order — input cadence stays human-plausible); reject ⇒ rate_limited
with hint retry_after_ms. Defaults validated in the build; the numbers are
the draft, the mechanism is the point.
```

### F36 → adopt

`08` — persisted media contract: `<data>/media/`, its own cap (draft: 128 MiB
per session, oldest-first eviction), same retention as history (7 days after
session end); the 8 × 4 MiB segment budget is text-only.

### F37 → adopt

- `07` — print path + "run `daiorama token show`" at generation (value never
  in logs).
- `04` — `token show|rotate` pinned local-only, explicitly refused with
  `--remote`. Rotation-window clause: the file holds the new value while the
  manager still accepts the old until restart — output says so.
- `06` — one sentence: control-call arguments are not recorded in history
  (records carry scope, origin, timestamps, cause).

### F38 → adopt

- `07` — "a real enforcer" reworded to "a real gate for well-behaved callers"
  (consistent with the posture paragraph).
- Media row fixed per F31; all manager-adjacent HTTP surfaces (bridge MCP
  endpoint, view front, dashboard) validate Origin/Host, loopback default.

---

## E. Packaging

### F39 → adopt

`09 §Container image` — "one GPU line, MCP port, the view range `6080-6099`
(publish when views serve remote consumers), optional volume; per-instance
`view.port_base` on multi-instance hosts (08)".

---

## Appendix — planning counters + P1 routing response

**P0 exit** — agreed: reconciled freeze commit + open-items-admitted list with
deadlines. Proposed deadlines: B9 mechanism → recorded now, numbers → P1 exit
(earlier than P3 — the mechanism is one bucket, don't let it drift);
Hyprland range + GPU recipes → P1 exit; renderer re-verification mechanism →
P1 exit (it is a spike exit criterion, list it in the same row).

**P1 routing** — L1 on your box ✓ (proven: render node, GLES NV117,
cap-drop=ALL). L2 on my side needs host-level podman: my container has no
nested runtime, ro cgroupfs, renderD128 denied — so the NVIDIA/CDI leg runs at
Ygypt's host level (or defers behind his call); I'll package the probe from
your evidence pattern so it's one command wherever it runs. L3 fits my box as
the CI hub if the llvmpipe substrate exists at all (userspace installs only —
no root here); the substrate decision is in the spike list anyway.

**Workflow refinements** — all three adopted as stated: repo-native progress
file (propose root `STATUS.md`, ≤20 lines, branch/head/last-green/next/open),
evidence-recipe standard (this pass and yours model it), 5-field resume pack
verbatim.

## Sign-off shortlist additions (for 11)

1. F29 — exposure model vs bridge relay (adopted: exposure; flip cost: relay
   build).
2. F13 — input vocabulary pinned at freeze (flip cost: the deferral text).
3. F22 — `sessions.visibility` moved to scope files; `--all` dropped.
4. F30 — instance-scoped observation accepted + stated.
5. F35 — rate-limit mechanism adopted; numbers verify in build.
