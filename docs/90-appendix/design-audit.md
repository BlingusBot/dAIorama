# dAIorama — design audit

*Audited 2026-10-05. Method: four independent audit lenses over the full document — systems/contracts, modularity & packaging, ergonomics, internal-consistency sweep — findings merged and de-duplicated, plus spot-checks of the document's load-bearing external claims against live sources. Severity keys: **blocker** = cannot build as written without guessing, or breaks in normal use; **major** = causes rework or user-visible inconsistency; **minor** = polish.*

---

## Verdict

The load-bearing seams survive review well. Manager-as-only-daemon, the stateless bridge, view-as-one-mechanism, the lock-guarded lazy start, PDEATHSIG ownership, deny-by-default manifests, and the SSD-wear and threat-model reasoning are all sound — and the doc's habit of recording *why* is what made this audit possible at all. What is not yet buildable without guessing clusters in four places: **(1)** the external contract (names, flags, defaults, ports, error codes, per-key config authority); **(2)** network identity (bridge port, view-port token enforcement, scope authentication); **(3)** lifecycle edge policies (reaper semantics, view state ownership, hang handling, descendant cleanup); **(4)** module/release mechanics (`dashboard install`, version matrix, cgroup preconditions).

One structural recommendation underlies half the list: the doc needs a normative **Interfaces appendix** (CLI tree, flags, defaults, ports, env vars, error codes, version rule, scope transport, per-key config authority, filesystem layout) — and §11 needs to be split into *must-decide-before-build* vs *open-for-implementation*.

---

## Blockers — decide before build

### A1. The doc's own contract rule is contradicted by §11
The intro says command names, flags, defaults, and component contracts are exactly what is *not* left open; §11 says exact command names are intentionally left open. A builder cannot implement a CLI contract from a doc that disclaims it. Concrete unnamed pieces the rest of the doc already relies on:
- **The bridge entry point is never named** — no `daiorama <cmd>` appears for the MCP bridge, though every harness config and §9's compose snippets must invoke it.
- **`end` vs `end --force`**: plain `end` behavior is never stated (refuse while sessions live? presumably — but undefined), only `--force` appears.
- **Stop-verb drift**: close (sessions), end (daemon), stop view, "stop session" — one concept, four spellings.
- **Ports**: manager port, bridge MCP port, view port allocation — no defaults; §9's compose "a VNC port" cannot serve multiple viewed sessions as written.
- **Resolution/DPI**: called creation parameters "not left to whatever the backend defaults to" (fingerprint rationale) — yet given neither flag names nor default values.
- **Env var**: "`DAIORAMA_TOKEN` or similar" — not a name.

**Fix:** pin all of the above in a normative appendix now; leave only internal struct names, file paths, and algorithms open in §11.

### A2. Config has no per-key authority; `config set remote.default` cannot work as written
§4.2 folds all config into manager RPC ("a remote config change and a remote session creation are the same kind of call"), but §6 makes `remote.default` a client-side routing value the CLI must read *before it can locate any manager*, set "with the ordinary `config set` command." Read strictly, `config set` writes the manager's copy and the caller's routing default never changes; read the other way, config commands are not uniformly RPC and a builder must guess which keys live where. Same wall for `config edit` against a remote manager (its file can't open in your `$EDITOR`) and for `config setup`. And there is **no reload rule anywhere**: does a running manager re-read core config or `scopes/` after an external edit, and when? Scope files add a third editing mode with its own semantics.

**Fix:** normative per-key authority table (client-local vs manager-side), enforced in the command contract; define remote `config edit` (fetch–edit–push or explicit refusal); state reload semantics explicitly (restart-only, an explicit `config reload` RPC, or per-call reads for scopes).

### A3. Network surfaces: the flagship one is missing from the rule, and the view-port gate has no enforcer
§6 enumerates network-reachable things but (a) **omits the MCP bridge in network-service mode**, which §9 calls "the default, flagship shape" (published MCP port) — no stated bind default, no stated auth, and it necessarily reaches managers (possibly remote ones, possibly token-bearing); (b) gives the dashboard no concrete rule; (c) says the view port is gated "by the same auth token" while **naming no component that can enforce it** — wayvnc implements no such token, and a browser cannot send an Authorization header to a VNC/noVNC endpoint; (d) gates on bind address, which is fragile once anything proxies or publishes.

**Fix:** make the gate request-time (unix socket stays tokenless); add bridge + dashboard to the enumeration; name the component that fronts the view port as a manager-owned static+WebSocket front that serves the noVNC page, checks a token-derived credential (cookie or one-time code), and proxies to loopback wayvnc. This also closes §11's wayvnc item — confirmed in the field: wayvnc serves RFB-over-WebSocket frames only; the noVNC page needs a companion server regardless, so the gate belongs in front of both.

### A4. Scope is a self-asserted identity
Per-scope quotas and manifests are the stated harm reduction for "several differently-trusted agents on one box." But the scope is a string the caller claims per call; the network gate is one manager-wide secret; nothing authenticates the claim. On the shared/central shape, any caller can act as any other scope — evading its own quota, reading sessions visibility rules were meant to hide. "Differently-trusted agents" cannot be told apart at all.

**Fix:** pick one model — per-scope secret validated by the manager (same presentation path as the manager token), or explicitly document scope as a cooperative label and soften the trust language accordingly. Pin scope transport per bridge mode (stdio: config/env at start; network: per-connection authentication, not a claimed string).

### A5. Token lifecycle contradicts itself, and rotation silently drops every session
§6 says a token "is generated automatically on first use if none is set," and also that the manager "refuses to start bound non-locally without a token configured" — a builder cannot implement both. No path, no `token show`/`rotate` command, no client-side missing-token error text. And rotation "just regenerating the file and restarting the manager" — restarting the manager drops every running session via PDEATHSIG; §6's "no running-state to migrate" misses that.

**Fix:** pick one rule (e.g. generate on first non-loopback bind, print path + value once); name the file path; add `daiorama token show|rotate`; specify the client refusal text; state the rotation-drops-sessions consequence (or support live reload).

### A6. No machine-usable error or denial contract
Deny-by-default is the agent's entire world, yet the only specified errors are prose ("clear error"). Nothing lets a consumer distinguish: session-not-found vs reaped vs closed; tool-not-granted vs quota (scope) vs global ceiling; rate-limited; manager-unreachable. Nothing lets a fresh agent discover what *could* be granted. Consumers will invent string matching against messages designed to be human-readable. Also absent from the RPC surface: pagination for list/history, and idempotency for `new`/`control` — with §4.2 explicitly inviting flaky connections, an at-least-once retry of `new` duplicates desktops, and of input calls double-types.

**Fix:** stable `{code, message, hint}` error envelope, mapped 1:1 onto MCP tool errors, codes treated as versioned API; `list_capabilities` read-only tool inside the always-on lifecycle set (grantable tools + effective quotas/audit flags); reaped distinct from closed; pagination + idempotency keys with a documented retry rule.

### A7. The version handshake isn't implementable as written, and rejection is the wrong policy for the dashboard
"Additive-only" plus "reject a client that declares an incompatible version" defines neither the comparison rule nor who checks what — and under additive growth the manager *cannot* detect a newer client's unsupported call; only the client can pre-check. A museum aggregating managers at different versions must degrade per manager, not reject them; binary vs protocol version, and whether MCP tool *names* are additive-only (agents cache them), are unstated.

**Fix:** protocol semver + `min_supported` in the handshake with the exact comparison rule; capability discovery beyond liveness; machine-readable version error carrying both versions; dashboard degrades per-manager via capability detection; publish a small compat table; declare tool names additive-only.

### A8. `clip_video` resource link contradicts ephemeral-by-default capture
A fetchable URI needs the artifact to outlive the call; "exists only long enough to be served as that call's response, then discarded" says it doesn't. No server, port, TTL, or auth is stated — an unenumerated listener, unlike the carefully normed manager and view ports.

**Fix:** specify a manager-served media-fetch endpoint with an explicit TTL / fetch-once rule, under the same exposure+token rule as everything else, with its interaction with `audit.store_clips` stated; redefine "ephemeral" as "deleted at TTL."

---

## Major — seams & lifecycle

### B1. The idle reaper's "activity" reaps exactly the sessions in live use
§5's intended perception pattern is a realtime consumer that "never touches MCP again" while watching the VNC stream; §4.4 lets humans watch. Both send zero control calls — so a ~1h "no control calls" window kills sessions that are actively being watched. Also unstated: whether the reaper is on-by-default (the "teardown is always explicit" rule plus the lean principle suggest opt-in; a stated 1h default suggests on), whether query/dashboard calls refresh the timer, and what a reap looks like (notice? log? difference from closed?).
**Fix:** activity = any session-scoped RPC *or* attached view *or* compositor input events; exempt watched sessions from the control-call timer (separate view TTL if wanted); surface idle-since in session detail; state default on/off; distinguish reaped from closed.

### B2. View state has no registry owner; port allocation unstated
§4.1's registry covers sessions/IDs/status only. Nothing owns view state: running, address, port, generation. Consequences: if view ports are ephemeral, every cached `get_view` address goes stale after a view crash; concurrent `get_view` calls can fork two wayvncs; Hyprland death says nothing about killing the view (a lingering wayvnc keeps its port while the session is "dead"); §9's single "VNC port" in compose can't serve several views.
**Fix:** registry view struct {state, address, generation}; allocate the port at session creation (or return a generation token so stale addresses are detectable); per-session lock around view start; explicit kill-on-session-death; define port allocation strategy for multiple viewed sessions.

### B3. Hang ≠ crash: the health-check failure action is undefined
"A failed health check gets the same treatment as a crash" doesn't transfer — a crash ends processes; a hang leaves Hyprland *and its whole app tree* running, holding GPU and memory. Undefined: does the manager TERM→SIGKILL the tree? Are children reaped or merely flagged? Does a dead session free its quota slot / cgroup, and when? Do dead sessions count against `max_sessions` and the global ceiling? This is precisely where zombie accumulation happens.
**Fix:** explicit state machine (running → degraded → dead → closed) and the health-failure action (kill the tree via its cgroup, mark dead, quota freed only on close); dead sessions stay listable but don't count; state who closes them (reaper or operator).

### B4. PDEATHSIG covers direct children only; state the guarantee precisely and extend it
Hyprland and wayvnc are direct children — the kernel guarantee holds for them. Everything Hyprland execs is a grandchild: browsers and their process trees are *not* covered (Wayland clients usually exit when the compositor dies; "usually" is not a guarantee). The per-session cgroup §9 already builds is the right cleanup unit — make crash/close paths `cgroup.kill` it, with PDEATHSIG as the belt to that suspenders. Also worth documenting: the fork→prctl window (child must re-check `getppid()` after setting it), and which signal is configured (anything but SIGKILL can be ignored by a wedged compositor).
**Fix:** one paragraph in §4.1 stating scope + the cgroup-kill completion + the prctl-after-fork check.

### B5. Lazy start: command classes, `--remote` behavior, and mode determinism
"The first command that needs one" never says which commands need one — must `list`/`status`/`config get` start a daemon to answer "nothing exists"? (Lean principle says classify cold vs hot commands and never start for cold ones.) "Can't find one reachable" doesn't exclude an explicit `--remote`: a dead remote must **never** trigger a local start. The attach-vs-background guess has unstated consequences (foreground supervisor? SIGHUP on shell exit?) and no override/reporting. And "O_EXCL or flock" are not equivalent — flock is kernel-released on crash; an O_EXCL lockfile goes stale and needs recovery; the loser's "waits briefly" needs a bound + retry-once if the winner died pre-bind.
**Fix:** cold/hot command classification; never auto-start on explicit `--remote`; deterministic mode with a report line ("started manager, mode, pid, socket") and a config pin; flock + bounded retry semantics spelled out.

### B6. `host` backend: contradicting "never needs the display node", and exclusive at the wrong granularity
§3.3's host mode *is* the DRM/KMS path — §3.4's blanket "dAIorama never needs the display node" must be scoped to headless/nested, with host documented as the exception (needs a seat / display node; rootless story unproven there; effectively bare-metal only, since containers lack the display node). And exclusivity is **device-global, not per-manager**: two managers on one host — or a host manager plus a container with the host device — each pass a per-manager check, then fight over the same connector; concurrent `new --backend=host` calls also race.
**Fix:** scope the claim; gate on the device itself (flock the card node) plus serialized session creation; define the second-request error naming the incumbent session.

### B7. §7.2 interop needs semantics, not just "falls out for free"
Two different discovered sockets are collapsed into one behavior: the manager's own (adopt = talk to it — fine) and Hyprland's (adopt = unclear: whose session is it? no child, no PDEATHSIG, no ownership). And if a container CLI finds the *host* manager, session RPCs execute on the host — desktops run outside the container and its isolation boundary, inverting the deployment's purpose. Nothing distinguishes "my socket" from "a foreign socket" before adopting.
**Fix:** scope the trick to CLI/RPC discovery (talking to a foreign, compatible manager — stated feature); identity-probe any discovered socket (protocol version + instance ID) before adopting; make attachment explicit, never a side effect of session creation; document the boundary-inversion hazard as the reason for the guard.

### B8. History and dashboard scoping
Own-sessions-only is specified for sessions; history entries carry origin but **no scope field**, and the dashboard's whole point is cross-session, cross-manager reads. Nothing says whether the visibility rules apply to history or how the dashboard gets a (deliberately broad) read grant.
**Fix:** add scope to history records; history reads obey the same scoping rule; the dashboard is a named scope with an explicit, documented broad-read grant — never default.

### B9. Rate limiting: keep, but put it where decisions live
§4.3 correctly records the intent (control-call rate is itself a detection signal) — but the mechanism isn't on §11's list of open items. Since it cuts directly against the project's reason to exist, leave §11 with an explicit entry and treat it as must-decide-before-ship for central/shared shapes, not generic abuse boilerplate.

### B10. Manager→dashboard reporting contradicts "zero dashboard-aware code"
Dashboard-initiated polling is genuinely "just another client." Manager-dials-dashboard-URL is a *second mechanism*: manager config, an outbound client, reconnect/backoff, an event protocol (message set, cadence, catch-up after outage, dedupe), and sink auth — how does a museum hold N managers' tokens? As written, "no special coupling" is false.
**Fix:** ship v1 as dashboard-initiated pull only; if NAT-case reporting is needed, spec it explicitly as its own mechanism (config key, protocol, auth) or a separate optional relay module — don't deny it in prose.

---

## Major — modularity, packaging & release

### C1. `dashboard install` is a name, not a mechanism
Unspecified: what gets installed (release asset per target triple), from where, version match to the core binary (or latest?), install path/PATH behavior, checksums, update/rollback, offline path, and the **container case** — a runtime download evaporates on container recreate unless on a documented volume or shipped as an image variant. Note also: the download-shrink rationale is weaker than stated, since the core binary already ships noVNC assets for `--view`.
**Fix:** define asset naming per triple, version-match default, atomic activate/revert, checksum, `--from` for offline, container path documented.

### C2. A release manifest is missing
"A static Rust binary" (singular) vs "the CLI and bridge binaries" (plural, independently upgradable): a builder can't tell whether the bridge is a subcommand or a sibling artifact. A stdio bridge must exist **where the harness runs** — an agent's own container — which is an unnamed client-only artifact. The Homebrew tap implies macOS; manager subcommands are Linux-only — state the client-only scope. Image: no tag scheme, digest pinning, or variant story.
**Fix:** signed per-release manifest (artifacts × triples × checksums); one multiplatform core binary + separate dashboard module + client-only install mode; macOS = client scope; pin snippets by digest.

### C3. Upgrade operations: give operators a procedure
"Updating a shared instance drops what's running; avoid while agents are active" is a warning, not a procedure. No drain path, no version visibility (component versions don't appear in the readiness call), no documented sequence for bare-host or compose users; clients have nothing to display on skew.
**Fix:** expose versions in readiness; add drain (reject new sessions, report remaining); document upgrade sequences; state client-side skew display.

### C4. GPU detection cannot run inside an unconfigured container
§3.4's bundled detection step, if run in a container without passthrough, sees no device and no `nvidia-smi` — it cannot diagnose anything; AMD/Intel needs host state a container can't apply (render group membership, SELinux boolean, udev-dependent permissions). The "one-line" compose snippet is CDI-only.
**Fix:** a host-runnable check that prints the exact next step per GPU family; per-family compose snippets (render-node recipe vs CDI) listing host prerequisites; in-container startup only verifies the actual renderer and fails loud.

### C5. Bare-host requirements exceed Hyprland + wayvnc, and the capture decision is unmade
§8 detects Hyprland and wayvnc, but the design also needs: **screen capture** — undecided whether an in-binary screencopy client or an external tool (grim etc.); this decides whether the "static binary" claim survives. And **a browser**: whose WebGL gets checked (3.4), whose profile 9 persists — the provider is never named or scoped. Also: wayvnc version check, noVNC assets, clipboard.
**Fix:** publish a dependency + version-check table per shape; decide capture in-process (screencopy protocol client); name who supplies the browser (and where it lives in the container image).

### C6. cgroups v2 per-session containment: preconditions unstated, fallback unstated, bare-host unaddressed
"Rootless Podman already supports delegating" hides real preconditions: unified cgroup v2 hierarchy; systemd-based delegation (`memory`+`tasks` delegated by default, `cpu` usually not; distro variance — some delegate nothing by default); creating sub-cgroups inside a container needs cgroupfs rw, ownership, and sometimes a SELinux boolean; a bare (non-container) manager can't create limits in its own populated cgroup without a delegated scope. If any precondition fails, the promised containment silently doesn't exist.
**Fix:** state preconditions; detect effective delegation at startup and fail loud or document a degraded mode; define the bare-host path (`systemd-run --user … -p Delegate=yes` scope) or scope the feature to containers; add to the spike.

### C7. Testing seams: make the harness the spike's exit criterion
The P0 spike should *leave behind* the repeatable harness §11 demands: a committed container script; protocol-level tests under software rendering as a CI lane; a separate manual/GPU lane; fake-manager + MCP conformance fixtures; a renderer check. Resolve the phrasing tension: "no software-only fallback" vs "CI under software rendering" — state that llvmpipe is functional enough for protocol tests but never acceptable for real sessions (fingerprint + performance); that distinction is exactly why §3.4's fail-loud check exists.

### C8. Filesystem contract table missing
Categories are named (data dir, token file, runtime fallback, `scopes/`, module target, dashboard config) but no paths. Compose mounts, backups, §7.2's bind-mount, and the read-only config mount all need exact paths; and it's unclear what's per-instance vs per-user — several managers on one host would collide token/history paths.
**Fix:** a stable layout table (config, data — instance-discriminated history/token/modules —, runtime fallback + socket naming, container equivalents, dashboard config), referenced by compose, backups, and dashboard install.

---

## Major — ergonomics

### D1. First-run and refusals are under-specified
`config setup`'s contents are undefined; the GPU detection command is unnamed; nothing says where users learn `--view`/`--backend`/resolution flags (`new --help`? the wizard?). Refusal texts are uneven: llvmpipe, missing token, Hyprland version mismatch each get a promise of clarity but no content.
**Fix:** define setup's sequence; name the detection command; surface creation options in `new --help`; write the actual refusal strings (including the compat range in the version error).

### D2. Start-mode visibility
Three paths to one process (serve / daemon start / lazy) with no way to see which is active or switch; `daemon status` fields undefined; "manager unreachable" is specified for bridge and dashboard but not the CLI. Also plain `end` vs `end --force` (see A1).
**Fix:** define `status` output (mode, pid, socket, versions); allow attach/detach or explicitly drop; one shared reachability/refusal string across CLI, bridge, dashboard.

### D3. Dashboard operator flows
Only `dashboard connect <address>` exists: no connection CRUD, no token story for aggregating N managers (env vars don't scale; define storage, 0600, rotation), no stated scope the dashboard queries as. "Take over" is promised once and never defined (input-arbitration state: agent/human/shared with explicit transitions). Rename never says label vs ID (must be label-only; IDs stay stable).
**Fix:** connection list/add/remove; dashboard config for tokens; named operator scope; define take-over and rename semantics; surface view-down vs session-dead in the UI.

### D4. Scope ergonomics: unreadable, unscriptable, unvalidated
get/set deliberately excludes scopes; "config edit-style access" never says which file. Operators on the shared instance can't script, diff, review, or read effective quotas/manifests — and under deny-by-default a typo in a granted tool name silently shrinks capability (the soggy failure §2 forbids).
**Fix:** `config scope list|get`, `config effective`, `config validate`; version scope files with startup validation; unknown tool/category name = load error listing valid names; `setup`/scope-add emit commented starter files.

### D5. Pin history defaults; RON choice for scopes needs its rationale
"A handful of rotated segments" and "short grace period" are placeholders for user-visible disk behavior — pin them (e.g. segments × cap, retention). The RON choice has no recorded rationale, and the files most likely hand-edited (scope manifests) are where RON diverges most from non-Rust expectations — either record the reason or use TOML for hand-edited files.

### D6. llvmpipe handling is an unmade fork
"Refuse to create the session **or** surface an unmissable warning" is two different products, and the detectors fire at different times — a compositor-side check can pass while a browser still reports software rendering. Since quiet degradation is the one failure the doc says must never happen, the response is load-bearing.
**Fix:** write the decision — refuse at creation on a software renderer; if unverifiable, warn at creation and re-verify at first browser launch, marking the session; quote the user-visible message.

---

## Minor / nits

- **Theme register**: error/status strings should stay plain and stable (operators grep them); theme nouns only in dashboard chrome; add a one-page term map — and resolve "daiorama"-as-session-noun colliding with the product name.
- **§7.1 duplication**: the entrypoint that pre-creates the runtime dir should call the binary's own ensure step (or be dropped), so the shipped image exercises the same code path as the "real fix."
- **macOS tap scope**: state that it covers client subcommands only, or note iOS/Mac is client-only.
- **Stop-view**: get_view is specified; stop-view exists only as a dashboard button — say whether it's an MCP tool and in which manifest.
- **"DAIORAMA_TOKEN or similar"**: pin the exact name (it appears in every harness config).
- **Homebrew tap** (repeat of C2 minor): keep.

---

## What holds

- Manager-is-the-only-daemon; no hidden second process (settles the remote/config ambiguity at the right place).
- Stateless bridge: harness restart costs nothing; sessions belong to the manager.
- View creation, on-demand view, and `get_view` really are one forked-child mechanism, stated once — no duplicate build.
- Lazy-start double-start race closed by an atomic lock at the right seam ("exactly one manager per runtime directory").
- PDEATHSIG + explicitly-empty-registry-after-restart is a clean, kernel-backed ownership story (scope caveat: B4).
- Deny-by-default manifest + the named always-on session-lifecycle exception are internally coherent — and the exception is genuinely necessary for bootstrapping.
- `--remote` = one meaning, two address shapes (host:port or unix path) — one mechanism, not two features.
- Scopes split out of the dot-path config with an honest rationale; runtime-dir fix single-sourced; history's buffered-write/rotation reasoning (write pattern, not format, is the wear driver); wayvnc-down ≠ session-down; §8's startup version-check pattern; missing-Hyprland refusal prints the exact per-package-manager install command.

---

## External claims — spot-checked (with sources)

- **Hyprland virtual input protocols**: CONFIRMED. `zwp_virtual_keyboard_manager_v1` and `zwlr_virtual_pointer_manager_v1` are supported by Hyprland, with in-tree implementations — the rootless input-injection story holds. (wayland.app protocol listings; Hyprland source: `src/protocols/VirtualKeyboard.cpp`, `VirtualPointer.cpp`.)
- **wayvnc websockets**: CONFIRMED data-channel-only. WebSocket support since 0.7.0 (`-w`); socket forms `ws:`, `ws-unix:`, `ws-fd:`; it frames RFB over WebSocket and does **not** serve the noVNC HTML/JS page — the standard pattern is a small web server in front serving noVNC (`vnc_lite.html`) and proxying the WS. Auth interop is fiddly (wayvnc's TLS security type is noVNC-hostile; `relax_encryption` is the common workaround) — reinforcing A3's fix: put the token gate in the front. (wayvnc manpage/`wayvnc.scd`; noVNC issue #1824; wayvnc discussion #353; raspberrypi/bookworm-feedback #5.)
- **Headless paths**: both real, both with sharp edges — supporting §3.3's "resolve during building" hedge. `hyprctl output create headless`: current black-screen reports (Hyprland discussion #12690, 0.52.2), clients-not-starting-without-monitor reports (forum.hypr.land), and wayvnc resizing a headless output trips a Hyprland protocol error (wayvnc issue #326) — test the resize path for `--view`. `AQ_NO_KMS_REQUIREMENT=1`: wiki-documented for render-only GPUs; Hyprland then auto-initializes a HEADLESS-0 output (1920x1080@60). (wiki.hypr.land Virtual GPU page + env-var reference; aquamarine PR #255/release notes.)
- **NVIDIA CDI**: CONFIRMED as described — since Container Toolkit 1.18 a systemd unit auto-generates/refreshes the CDI spec (toolkit install/upgrade, driver install/upgrade, reboot); `--device nvidia.com/gpu=all` is the documented invocation. (NVIDIA Container Toolkit docs: CDI support; 1.18.0 release notes.)
- **MCP content types**: CONFIRMED — tool results support `text`, `image`, `audio`, `resource_link`, embedded resources; there is no video type, so `clip_video` → resource link is the spec's own shape. (MCP spec 2025-06-18, Server/Tools.)
- **cgroups v2 rootless delegation**: NOT as settled as §9 implies — see C6; preconditions confirmed real, several open rough edges in-container. (systemd `CGROUP_DELEGATION`; podman discussion #20295.)
- **Bonus finding**: a real-world llvmpipe fallback incident (Hyprland running software-rendered when a renderer node was discarded) is exactly the silent failure §3.4 fears — the fail-loud renderer check is well-founded, not paranoia. Worth a line in the doc tying §3.4 and §11's CI lane together.

---

## Suggested revision order (single editing pass)

1. Normative Interfaces appendix: CLI tree + flags + defaults, env vars, ports, error codes, version rule, scope transport, per-key config authority, filesystem layout.
2. Split §11 into *must-decide-before-build* (CLI surface, clip_video lifetime, scope delivery, dashboard scope, reaper default, token/ports, static-server decision, rate limit) vs *open-for-implementation* (headless-path choice, GPU recipe details, rotation/backoff internals).
3. Lifecycle state-machine paragraph: session + view states, kill paths (`cgroup.kill`), quota accounting, reaper semantics.
4. Trust boundaries: §6 enumeration fixed + request-time gate; scope model decision; token lifecycle.
5. Dashboard direction decision (pull-only for v1) + `dashboard install` mechanism + release manifest.
6. GPU/cgroup/bare-host preconditions + spike exit criteria (committed harness).
7. Pin remaining defaults (resolution/DPI, ports, rotation/retention, exact env name).
