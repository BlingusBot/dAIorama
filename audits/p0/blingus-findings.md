# P0 audit pass — BlingusBot (independent) — FINAL, for reconciliation

status: **final for reconcile** — own-lens pass + three independent subagent
waves, cross-verified, deduplicated, and merged. Each finding keeps a source
tag (own / sys / ctr / sec = wave lens) so nothing drops silently; raw wave
outputs are committed verbatim under `waves/`.
audited: `spec/v1` @ `f4e4cd1`
method: full read of 00–11 + `90-appendix`; live reference-box probes
(`evidence/`, raw outputs + repro scripts); three independent lens waves
(systems/feasibility, contract/consistency, security/trust) run as isolated
subagents and merged after (a) dedup and (b) targeted re-verification of
load-bearing claims (below).
re-verified on merge (not taken on the waves' word): Fedora's Hyprland
package is orphaned/absent for f43+ (packages.fedoraproject.org shows f41/f42
only; Fedora discussion + devel list confirm retirement; COPRs are the live
path); the MCP 2026-07-28 changelog (fetched) confirms per-request `_meta`
versioning, `server/discover`, and `UnsupportedProtocolVersionError` (-32022);
all reference-box claims come from our own committed probe outputs.

Severity key (as `90-appendix/design-audit.md`): **blocker** = cannot build as
written without guessing / breaks in normal use · **major** = causes rework or
user-visible inconsistency · **minor** = polish.

**Summary: 39 findings — 5 blockers, 27 major, 7 minor.**
Suggested reconcile format per item: `F# → adopt / reject / counter + replacement text`.

Source map (raw → merged): own F1→F1, F2→F28, F3→F26, F4→F6, F5→F8, F6→F31,
F7→F23, F8→F25, F9→F16, F10→F22, F11→F16 · sys F1→F1, F2→F7, F3→F8, F4→F9,
F5→F39, F6→F6, F7→F4, F8→F2, F9→F3, F10→F31, F11→F12, F12→F10, F13→F28,
F14→F5 · ctr F1→F13, F2→F14, F3→F15, F4→F11, F5→F22, F6→F12, F7→F21, F8→F16,
F9→F17, F10→F18, F11→F19, F12→F20, F13→F23, F14→F24 · sec F1→F26, F2→F27,
F3→F28, F4→F29, F5→F30, F6→F31, F7→F32, F8→F33, F9→F34, F10→F35, F11→F36,
F12→F37, F13→F38.

---

## A. Renderer & spike chain

### F1 — blocker — CI lane (llvmpipe) vs refuse-at-creation; re-verification has no mechanism
**Cite:** 06 — *"if the active renderer is detected as software (llvmpipe or equivalent), creation refuses"*; 09 — *"a CI lane running protocol-level tests against the headless backend under software rendering"*; 06 D6 — *"re-verified at first browser launch"*.
**Problem:** C7's mandated CI lane creates sessions the creation path unconditionally refuses; no config key, env, or flag can express "software is fine for tests" — the "phrasing tension resolved" paragraph resolves the policy, not the mechanism. Second half of D6 has no executor either: the browser is a compositor grandchild the manager holds no handle to, and no automation protocol may ask it (by design). Hyprland's own software path is not guaranteed up front (`WLR_RENDERER_ALLOW_SOFTWARE` caveat, upstream #3516), so the CI substrate itself needs a decision.
**Disposition:** one manager-side config key `renderer.allow_software` (default off, config-only — never a runtime flag); state the re-verification mechanism explicitly (e.g. session-owned launcher wrapper capturing the browser's own GL string into a manager-readable channel, and/or rendered-probe-page readback via screencopy) and add it to the spike exit criteria; reword 03 "no software-only fallback path" → "no *supported* software-only path". *(src: sys F1 + own F1)*

### F2 — major — Headless path ordering is inverted for containers
**Cite:** 03 — *"try Hyprland's dedicated virtual-output feature first; if flaky in practice, the render-only DRM path (AQ_NO_KMS_REQUIREMENT=1) is the documented fallback"*.
**Problem:** in a render-node-only container these are not alternatives: Hyprland by default only selects nodes with display outputs and ignores render nodes; `AQ_NO_KMS_REQUIREMENT=1` is the *prerequisite* that makes the render node selectable, and `hyprctl output create headless` is a post-start action layered on top. "Trying the feature first" means Hyprland never starts and the spike misattributes the failure.
**Disposition:** reword 03: render-only init (AQ_NO_KMS_REQUIREMENT=1, AQ_DRM_DEVICES where needed) = container prerequisite; virtual-output creation = the output/resolution mechanism on top; spike verifies the combination + the wayvnc resize path. *(src: sys F8)*

### F3 — major — Hyprland install/version matrix is dead on Fedora (verified); range values unpinned
**Cite:** 04 — *"Hyprland not found. Install it: {exact command for the detected package manager}, or use the container image."*; 09 — *"presence + version range"*.
**Problem:** on the target distro the pinned refusal would print a `dnf install hyprland` that cannot succeed — Fedora's package is orphaned and absent for f43/f44 (only f41/f42 builds exist; COPRs are the live path) — and the version-range check has no distro artifact to compare. Separately the `{min}-{max}` constants are never pinned anywhere.
**Disposition:** make install-command resolution a maintained table in the release manifest (distro → package/COPR → version, with "not packaged / out of range" rows routing to the image or pinned source); pin the initial range values at spike exit and name what the mismatch string says when the distro package is missing. *(src: sys F9 + own F7; spot-verified externally)*

### F4 — major — GPU recipe table has no NVIDIA-on-nouveau path; restate vendor-agnostically
**Cite:** 09 — *"NVIDIA — CDI (--device nvidia.com/gpu=all), host requires Container Toolkit ≥ 1.18"*; 03 — *"a single config value: a render-node path or a CDI device identifier"*.
**Problem:** the reference GPU (Quadro K2200, GM107) runs nouveau with no proprietary module and no toolkit — the CDI row cannot apply, and the render-node row is labelled "AMD/Intel", so `doctor`'s family dispatch has no rule for the box the project is built against. The generic render-node recipe is the correct, portable answer here and yields a real Mesa renderer (fingerprint check passes).
**Disposition:** restate recipes vendor-agnostically ("any `renderD*` → render-node recipe; CDI = the NVIDIA-proprietary alternative only"); add the nouveau case to `doctor`'s matrix. *(src: sys F7)*

### F5 — minor — Session compositor config supply + dialect unspecified
**Cite:** 09 — *"startup checks Hyprland's IPC surface against a known-compatible range"*.
**Problem:** nothing says how per-session config (resolution/scale/env) reaches the compositor (generated file vs `hyprctl keyword`), and the version gate covers IPC only while upstream config dialects changed recently — a generated config is a launch-time failure surface the IPC check won't catch.
**Disposition:** one sentence in 06 (generated per-session config under the runtime dir, written before exec); extend 09's gate to pin the supported config dialect. *(src: sys F14)*

## B. Containment & lifecycle

### F6 — blocker — C6's fail-loud has no error surface; the reference box fails it by default
**Cite:** 06 — *"If unavailable: fail loud with the exact next step by default; limits.mode = off is the explicit, recorded downgrade"*; 03 — *"Crash and close paths run cgroup.kill … the completion guarantee"*.
**Problem (verified live):** the reference agent container mounts cgroupfs read-only and cannot create sub-cgroups (observed; mkdir → EROFS), so with defaults the flagship "embedded" shape cannot start — yet 05 has no containment code, 04 has no string, 09's compose says nothing about the cgroup mount, and 03's loser-retry rule is undefined for a winner that dies at the delegation check. Additionally, `limits.mode = off` — the documented downgrade — silently forfeits the cgroup.kill completion path (PDEATHSIG covers direct children only), conflating "no limits" with "no cleanup guarantee".
**Disposition:** pin code `limits_unavailable` + 04 string naming both fixes (delegated/writable cgroupfs wiring, or `limits.mode = off` as recorded downgrade **with its cleanup caveat stated**); add the cgroup-mount line to 09's snippets; state check placement (manager startup; `doctor` reports it) and the loser outcome (same error; no loop). *(src: sys F6 + own F4)*

### F7 — major — The cgroup.kill "completion guarantee" has no executor on manager death
**Cite:** 03 — *"Crash and close paths run cgroup.kill on the session's cgroup — the completion guarantee"* + *"Nothing is reconciled; nothing could be"*.
**Problem:** cgroup.kill is code; on manager SIGKILL/OOM no code runs — Hyprland/wayvnc die via PDEATHSIG, but everything Hyprland exec'd is only *usually* cleaned by socket teardown; orphaned trees keep session cgroups, and the new manager reconciles nothing. Also unstated: PDEATHSIG binds to the *forking thread* (async-runtime foot-gun), and the recommended bare-host `systemd-run --user … -p Delegate=yes` path is a *scope*, which does not sweep its tree when the manager dies.
**Disposition:** mandate session cgroups under an instance-owned subtree + a **startup sweep** (registry can't be reconciled; cgroups can — say so); bless a systemd *service* unit (KillMode=control-group) as recommended supervision, note scopes don't give this; add the fork-thread constraint. *(src: sys F2)*

### F8 — major — Dead-session sweep and history retention both key off `close`, which nothing performs by default
**Cite:** 06 — *"the reaper closes them on its next pass (reaper.sweep_dead = on by default when the reaper is enabled)"*; 04 — reaper *off*; retention *"7 days after close"*.
**Problem:** with defaults, dead entries are closed by nobody — the zombie class B3 warned about reopens; registry and per-session history segregate for the manager's whole life, and retention keyed "after close" never triggers for sessions that never close.
**Disposition:** split the key — `reaper.sweep_dead = on` as its own default (sweeping a dead entry changes nothing operators rely on), independent of `reaper.idle`; key history retention to session *end* (dead/reaped/closed). *(src: sys F3 + own F5)*

### F9 — major — Hot classification starts managers for commands that cannot need one
**Cite:** 04 — hot = *"new, close, control, view, drain, daemon end"*.
**Problem:** `daemon end` on a quiet box starts a manager to end it; `close <id>` on a fresh box starts one, fails, and *leaves it running* — an accidental daemon, exactly what principle 2 forbids. No-manager is an honest, already-pinned answer.
**Disposition:** hot = `new` only; move the rest to cold; keep the startup line for `new` alone. *(src: sys F4)*

### F10 — major — Session IDs must not be reusable across restarts (cached-ID wrong-target hazard)
**Cite:** 02/06 — *"IDs are stable and opaque"*; 06 — view addresses solved staleness with generations; sessions have no equivalent.
**Problem:** nothing forbids a per-manager counter; agents cache IDs; the registry empties on restart; history retains old IDs — a stale cached ID could address a brand-new, different desktop.
**Disposition:** mandate globally-unique opaque IDs (UUIDv4/ULID) or add a manager birth-generation to session records with a distinct error on foreign-generation IDs. *(src: sys F12)*

### F11 — major — `session_closed`/`session_reaped` need a tombstone rule that is never stated
**Cite:** 05 — *"reaped ≠ closed ≠ dead — distinct codes"*; 06 — closed = *"registry entry removed. Final."*
**Problem:** a removed entry is indistinguishable from a never-existing ID unless closed IDs are remembered — retention, bound, and restart behavior unstated; the reap path (which call returns `session_reaped`, when) is also unstated.
**Disposition:** one sentence: closed/reaped IDs remembered for N hours (or manager lifetime), inside → the distinct code, outside → `session_not_found`; state the reap path explicitly. *(src: ctr F4)*

### F12 — major — Idempotency dedup, key format, and pagination mechanics under-specified
**Cite:** 05 — *"deduplicates within a bounded window (10 minutes or the session's lifetime, whichever is shorter)"* + retry-once rule; *"accept a `cursor` and return `next_cursor`"*.
**Problem:** key format/length/uniqueness domain unpinned; dedup-store persistence unstated — a manager restart inside the retry window duplicates the desktop the key exists to prevent (the exact failure the justification names); "session's lifetime" has no defined edges; cursors: opacity/stability/expiry unstated, no CLI pagination story.
**Disposition:** pin key shape (≤64 chars, unique per scope+operation); state dedup is manager-lifetime, lost on restart (or persist it — name the file); define the create window; cursors opaque + bounded validity + `bad_request` on invalid. *(src: sys F11 + ctr F6)*

## C. Contract surfaces

### F13 — blocker — `control` has no action vocabulary or argument grammar
**Cite:** 04 — *"daiorama control <session> <action> [args] — same action set as the MCP control surface"*; 05 — control-tool names *"pinned as each category lands"*; 11 A1 — *"Openness is now limited to internal struct names, file paths, and algorithms."*
**Problem:** the referenced surface contains category names only; `[args]` has no grammar anywhere. CLI parser, every control tool's inputSchema, scope-manifest validation ("load errors listing the valid names" — which don't exist), `list_capabilities`, and fixtures all depend on the missing vocabulary. Freezing this as-is freezes an unimplementable command.
**Disposition:** pin the argument grammar + the first category's action/argument names at freeze (additive-only after), or remove `control` from the frozen tree and state the landing rule. *(src: ctr F1)*

### F14 — major — Version handshake unpinned at both ends; MCP revision never stated
**Cite:** 05 — *"Both sides declare {protocol_version, min_supported}"*; *"managers reject out-of-range handshakes at connect"*; 04 `version` prints core+protocol only.
**Problem:** no transport named for the handshake, though "reject at connect" presumes one; `min_supported` surfaces nowhere operator-visible; edge cases (missing/empty/inverted ranges, prereleases) undefined; and the spec makes revision-relative MCP claims without pinning which MCP revision the bridge implements. The current revision (2026-07-28) moved to per-request `_meta` versioning + `server/discover` + `UnsupportedProtocolVersionError` (-32022) — "reject at connect" is legacy-era (re-verified at the changelog).
**Disposition:** state supported MCP revision(s) + era handling; define the dAIorama handshake as a pinned first call or MCP metadata; surface `min_supported` in `version`/`status`/readiness; map `UnsupportedProtocolVersionError`; add the omitted-`min_supported` default. *(src: ctr F2; spot-verified externally)*

### F15 — major — Error envelope is not total: four pinned refusals have no code
**Cite:** 05 — *"Every failure is a stable envelope"*; 04's string list.
**Problem:** `token required for {addr}`, `refusing to bind`, `Hyprland not found`, Hyprland out-of-range have no codes; `no manager running` has no machine shape; `bad_request` has no usage rule; most codes have no pinned message while 02 makes messages contract — consumers fall back to prose matching on exactly the auth/dependency paths A6 claimed to close.
**Disposition:** add the four codes or state process-level refusals are outside the envelope by design; rule for `bad_request`; JSON shape for `no manager running`. *(src: ctr F3)*

### F16 — major — Machine-readable output shapes unpinned (`--json`, carrier on MCP)
**Cite:** 04 — *"Read commands accept --json"*; 05 — *"MCP tool errors carry this envelope 1:1"*.
**Problem:** no field names for status/list/detail/errors; "read commands" undefined; cold-answer shape unstated; and MCP's carrier for tool errors (protocol error vs `isError: true` + `structuredContent`) is not stated for the string-code envelope — both mechanisms exist and the choice is load-bearing for harnesses.
**Disposition:** pin one JSON key set per read command; `--json` errors emit `{code,message,hint}` verbatim; state the MCP carrier explicitly (recommended: tool-execution error + structuredContent; JSON-RPC protocol errors reserved for malformed MCP). *(src: ctr F8 + own F9)*

### F17 — major — 04's pinned surface has four gaps
**Cite:** 04 — dashboard serve in no cold/hot/neither row; 09 has `dashboard install --version` not in 04; `mcp --manager` duplicates global `--remote`; history is advertised as queryable but has no read surface.
**Problem:** each gap is a guess that yields incompatible configs; A1's remedy was precisely "no guesses".
**Disposition:** classify `dashboard serve` ("neither"); add `--version` to 04; delete `--manager` or define it; pin the history-read surface or declare dashboard-phase-only. *(src: ctr F9)*

### F18 — major — Readiness call vs `status`: two field sets claimed to be one contract
**Cite:** 05 readiness — *"state + component versions (binary, protocol, dashboard module)"*; 04 `status` — state/mode/pid/socket/versions/counts/drain; 10 — *"One readiness contract"*.
**Problem:** neither is a superset; readiness lacks drain/pid/mode/counts, status lacks module version; the call's surface/route unnamed — watchers and healthchecks code against one of two shapes.
**Disposition:** declare `status --json` the contract and readiness its RPC form (superset), or state readiness ⊆ status; name the call. *(src: ctr F10)*

### F19 — major — Runtime-dir fallback unpinned; container equivalents drop `<instance>`
**Cite:** 04 — socket = `$XDG_RUNTIME_DIR/daiorama/<instance>/manager.sock`; 08 — fallback *"creates a fallback equivalent"*; container equivalents `/config, /data, /run/daiorama`.
**Problem:** CLI and manager must compute the same socket path; with `XDG_RUNTIME_DIR` unset (the ordinary container case!) two implementations can pick different fallbacks; the container equivalence sentence also drops the instance segment C8 exists for.
**Disposition:** pin the fallback (e.g. `$TMPDIR/daiorama-<uid>/<instance>/manager.sock`, 0700) and write container equivalents with `<instance>`. *(src: ctr F11)*

### F20 — major — Strings are not single-sourced; code messages unpinned
**Cite:** 02 — *"strings are written once … (04)"*; 05/09 carry two machine-relevant strings not in 04; 04 freezes "final wording" at sign-off.
**Problem:** string-matchers grepping 04 miss the scope-required and module-not-installed refusals; which prose is contract is undecidable.
**Disposition:** move both strings into 04 with codes; pin a `message` per code or amend 02 to "codes + 04 strings are contract; `message` is non-normative". *(src: ctr F12)*

### F21 — major — `config set` pickup timing/class semantics unstated
**Cite:** 08 — *"restart-only except where reloadable"*; 04 — local `config set` *"operates on the files directly"*.
**Problem:** an agent setting a key on a live manager gets no signal the value is inert until reload/restart (contrast token rotate's mandated plain statement); "cold" mis-describes `config reload` (it RPCs a running manager) — the classes are really never-starts vs may-start.
**Disposition:** "local `config set` writes the file; running manager keeps old value until `config reload`/restart; `config set` prints that fact"; redefine the classes; state reload never starts a manager. *(src: ctr F7)*

### F22 — major — `list` default-scope contradiction; `sessions.visibility` is instance-global
**Cite:** 04 — *"Default: the caller's own scope's sessions; --all for everything (operator view)"*; 07 — *"CLI reads are operator-wide"*; `sessions.visibility` sits in instance core config.
**Problem:** for the CLI, "own scope" is already operator-wide, so `list` vs `--all` is contractually ambiguous; and one scope's `sessions.visibility = all` grant flips broader visibility for **all** scopes — contradicting per-scope intent.
**Disposition:** state resolved semantics (CLI list = operator-wide; define or drop `--all`); move visibility into scope files or state the global consequence explicitly. *(src: ctr F5 + own F10)*

### F23 — minor — Values referenced but never pinned
**Cite:** 04 — drain `--timeout` (no default; unbounded `--wait`?); `{gmax}`/global ceiling has no key name; media TTL has a value but no key; `{min}-{max}` range; enum/duration value grammar; 09 manifest names no signature scheme.
**Disposition:** add the timeout default; name keys (`sessions.max_global`, `media.ttl`, quota fields); state enum/duration forms; pin range at spike exit; name the manifest signature scheme + verifier. *(src: ctr F13 + own F7)*

### F24 — minor — View-operation error contract gaps
**Cite:** 04 `view [--stop]`; 05 `stop_view`; 06 kill-on-session-death.
**Problem:** no code for view-start failure (wayvnc unusable, port range exhausted, front bind refused); `stop` with no view / after view death undefined; expired-media refetch has no stated response.
**Disposition:** pin `view_unavailable` (or `internal`); stop-with-no-view = idempotent success; dead session ⇒ `session_dead`; expired media ⇒ stated 404 shape. *(src: ctr F14)*

### F25 — minor — Detached-mode diagnostics have no sink on a bare host
**Cite:** 10 — *"stderr → journald / docker logs. No bespoke log files."*; 04 `daemon start` — *"detached"*.
**Problem:** detached with no supervisor = stderr to nowhere, in the mode meant to survive exit; "no bespoke log files" forbids the obvious fix.
**Disposition:** pin the sanctioned detachment (`systemd-run --user`/service unit; `daemon start` = dev convenience, terminal-scoped stderr) or allow one deliberate log destination. *(src: own F8)*

## D. Network, auth, views, media

### F26 — blocker — MCP wire presentation of credentials is unpinned (both hops)
**Cite:** 07 — *"the connection presents scope name + scope secret, validated by the manager on the same presentation path as the instance token"*; 09 — flagship *"published port"* shape.
**Problem:** nothing defines how anything is presented over the network MCP transport: stdio is env-only; Streamable HTTP has no static-secret channel in-spec beyond client-configurable headers; two hops are unpinned (harness→bridge, bridge→manager); 401/error behavior undefined (OAuth-discovery storms). The flagship shape cannot be built without this decision.
**Disposition:** pin request headers (e.g. `Daiorama-Token` / `Daiorama-Scope` / `Daiorama-Scope-Token`), validated per request by the manager (bridge = presentation relay), MCP-level errors not OAuth challenges; state the no-custom-header fallback (container-local stdio). *(src: sec F1 + own F3)*

### F27 — blocker — Operator identity over TCP undefined; the instance token is shared by all agents
**Cite:** 07 — *"Reserved scope operator: CLI reads are operator-wide"*; *"TCP bind requires instance token, always"*; *"a scope without a secret cannot be used over the network"*.
**Problem:** every agent holds the instance token; if token ⇒ operator, every agent gets operator-wide reads (defeating B8/A4); if not, the CLI has no defined operator path over TCP (operator scope has no stated secret). The two-layer model only coheres if identity ≠ transport token — as written it is unspecified on the shape scope-auth exists for.
**Disposition:** state: instance token is transport-only, confers no scope; CLI must present the `operator` scope secret over TCP (define it, ship a starter `scopes/operator.ron`); `scope_auth_required` when absent. *(src: sec F2)*

### F28 — major — View-front credential: a name, not a flow
**Cite:** 07 — *"checks a token-derived credential (cookie or one-time code …)"*; 05 — `get_view` *"returns its address either way"*.
**Problem:** issuance path, derivation (must be HMAC over instance token so the token itself isn't handed to callers — unstated), coverage (page only? assets? WS upgrade?), multiplicity ("cookie or one-time" are opposite semantics; dashboard + agent + human = multi-consumer), Origin/CSWSH validation, and noVNC client-supplied host/port params (open-relay risk) — all unstated. The A3 "real enforcer" is unimplementable without inventing the flow.
**Disposition:** pin one mechanism: per-view bearer `HMAC(instance token, view_id||generation)`, multi-use, dies with the generation; returned as a query param by `view`/`get_view` (never echoed elsewhere); required on **every** request incl. WS upgrade with `Origin` validation; server-side WS target; drop one-time codes in v1. *(src: sec F3 + sys F13 + own F2)*

### F29 — major — View address/port model breaks in the central shape
**Cite:** 06 — port allocated *"at view start … from view.port_base"*; 05 — *"the realtime consumer then talks to the returned address directly"*; 07 — front default *"loopback"*.
**Problem:** a view is two listeners (front + wayvnc) but one port is modeled; and in the flagship shape the returned loopback address is unreachable from the caller's container — the "talk to it directly" seam only works same-container, and nothing says what to do otherwise (the view-front-as-open-proxy question).
**Disposition:** registry stores front + vnc ports; for network callers, either bridge-proxy `/view/<session>/…` (despite relay being a named non-goal, this is view-only) or state plainly that the front must be explicitly exposed and how the credential travels. *(src: sec F4)*

### F30 — major — Observation gate is instance-wide; scope isolation stops at the tool boundary
**Cite:** 05 — *"token-gated beyond loopback … not a separate per-session credential"*; 07/A4 scope motivation.
**Problem:** any admitted caller can view any session's stream / fetch any known media id across scopes — the two most sensitive surfaces silently negate per-scope isolation. Accept explicitly or fix.
**Disposition:** add the one-sentence consequence to 07 ("observation is instance-scoped, not scope-scoped") or mint front/media credentials over (scope, id) with the front validating the scope claim. *(src: sec F5)*

### F31 — major — Media endpoint: fetch auth, address family, lifetime edges
**Cite:** 05 — *"deleted at first successful fetch or after TTL (default 60 s)"*; *"same address family as its RPC surface"*.
**Problem:** a `resource_link` fetch is out-of-band (no MCP headers ride along) → 401 for generic fetchers or an unspecified tokenized URL; "manager surface" address is unreachable in the flagship shape (the bridge exists because it is); fetch-once races/resume/undefined "successful"; id entropy unstated; 60 s vs agentic fetch latency; persisted copy vs fetchable copy diverge.
**Disposition:** pin: single-use TTL-bound capability URL minted **on the surface that served the call** (bridge proxies `/media` for network consumers); delete after response completes; `media_expired` envelope on miss; first-complete-wins concurrency rule; 128-bit ids; revisit TTL default. *(src: sec F6 + sys F10 + own F6)*

### F32 — major — Interop identity probe spoofable; no attach verb; credential ordering unpinned
**Cite:** 07 — *"probed for protocol version + instance ID before any RPC"*; *"Attachment is explicit, never a side effect"*.
**Problem:** the probe is unauthenticated by construction (routing aid, not trust), but nothing states where instance IDs live, who validates, ordering vs token presentation (harvest risk), or what "explicit attachment" *is* — no attach/pin verb exists in 04.
**Disposition:** probe strictly tokenless before any credential; instance-id file under `<data>/<instance>/`; TOFU pin on first `--remote` attach with a re-pin path; state the trust decision is the credential + operator choice. *(src: sec F7)*

### F33 — major — Bind-mount interop puts "cooperative local" scopes on the shared shape
**Cite:** 07 — *"local unix socket: scope claims are cooperative"* vs *"the shared/central shape … is where scope auth is real, and that is the network path"*.
**Problem:** the runtime-dir bind-mount gives a *different container* the tokenless, filesystem-gated socket with cooperative claims — including `operator` — on the very shape designated authenticated. Never reconciled.
**Disposition:** state the mount's trust consequence in the interop section (one sentence + ops note), require secrets for `operator`/`dashboard` even on unix, or scope the pattern to non-shared instances. *(src: sec F8)*

### F34 — major — Dashboard exposure rule self-referential; credential flows unnamed
**Cite:** 07 — dashboard row *"same rule as everything else"*; 10 — per-connection tokens in dashboard config.
**Problem:** the dashboard is multi-manager and has no single instance token; its own listener credential (if exposed) is undefined; exposing it = lateral access to N managers; the `dashboard` scope secret's home and the broad-read key are unnamed.
**Disposition:** pick: (a) v1 dashboard refuses non-loopback binds (remote = SSH/overlay), or (b) own-token lifecycle for the dashboard listener + per-connection `dashboard` scope secret; name the broad-read key. *(src: sec F9)*

### F35 — major — B9 rate limiting: mechanism must land (minimal candidate attached)
**Cite:** 07/11 — *"must-decide-before-ship for shared shapes"*.
**Problem:** code exists, trigger doesn't; on the shared shape the stealth rationale has no enforcement, and retry rules don't touch tight loops.
**Disposition:** per-session token bucket over control-class calls (input class; reads exempt); scope-file config `limits.rate { per_sec, burst, mode = pace|reject }`; default `pace` for shared shapes (pacing = human-plausible profile), `reject` ⇒ `rate_limited` + `retry_after_ms` hint. *(src: sec F10)*

### F36 — minor — Persisted media vs text-history retention contract
**Cite:** 05 — persisted *"under the history retention rules"*; 08 — line-per-entry text, 8×4 MiB segments.
**Problem:** one PNG/clip exceeds the text segment budget → rotation corruption, silent drops, or contradiction; persisted-media cap/eviction unstated.
**Disposition:** give persisted media its own contract in 08 (`<data>/media/`, own cap, oldest-first eviction, same 7-day post-close); state the segment budget is text-only. *(src: sec F11)*

### F37 — minor — Token hygiene details
**Cite:** 07 — value *"printed once at that startup"*; 04 `token show`.
**Problem:** print-once via stderr lands in persistent logs (same exposure class the spec avoids for argv); `token show` unclassified vs `--remote`; post-rotate the file holds a not-yet-effective value; control-call args not explicitly excluded from history.
**Disposition:** print path + "run token show" only; pin `token show|rotate` local-only (refused with `--remote`); rotation-window clause; one sentence on history contents. *(src: sec F12)*

### F38 — minor — Posture language + Origin validation
**Cite:** 07 — *"a real enforcer"* vs posture paragraph; media row circular; no Origin/Host validation stated for front/dashboard.
**Problem:** "real enforcer" invites quoting against the posture; media row cannot be implemented as written for bridge-reached callers (see F31); browser-facing surfaces need Origin/Host rules (MCP mandates them for the bridge).
**Disposition:** reword; fix the media row; one line: all manager-adjacent HTTP surfaces validate Origin/Host, loopback default. *(src: sec F13)*

## E. Packaging

### F39 — major — 09 re-introduces the single "VNC port" 06 declares dead
**Cite:** 09 — *"one GPU line, MCP port, VNC port, optional volume"*; 06 — *"the old single-'VNC port' compose assumption is dead"*.
**Problem:** views allocate one port each from 6080 upward; a single published port serves one view — wrong for `--view` defaults and the shared shape; multi-instance hosts also collide on the same base.
**Disposition:** publish an explicit range (`6080-6099`) or state the required range = base + max concurrent views; note per-instance `view.port_base` for multi-instance hosts. *(src: sys F5)*

---

## Ledger resolution check (merged verdicts)

Prior-audit items (A1–A8, B1–B10, C1–C8, D1–D6) were independently re-checked
by all three waves + the own pass. Verdict: most items genuinely resolved and
internally consistent. Residual mis-resolutions:
- **A1** → F13/F17/F23 (surface gaps under the "nothing open" rule);
- **A2** → F21 (pickup semantics);
- **A6** → F15/F16/F20 (envelope not total/machine shapes/strings);
- **A7** → F14 (handshake era);
- **A8** → F31 (reachability/auth);
- **B2** → F39 (VNC-port relic); **B3** → F8 (sweep default);
- **B4** → F7 (executor hole); **B5** → F9 (hot set);
- **C6** → F6 (failure surface); **C7/D6** → F1 (CI lane + re-verify).
- A3/A4/A5/B7/B8/D3 are resolved **as rules/enumerations**; the residual gaps
  are mechanism-level (F26–F34). No other silent re-introductions found.

**Verified OK highlights** (checked against sources, no finding): rootless
virtual-input story (Hyprland ships both wlroots virtual-input protocols; no
uinput/root needed); wayvnc = RFB-over-WS only, multi-client, front required
by construction; MCP content types incl. `resource_link` (no video type);
`cgroup.kill` present on the reference kernel (5.14+); delegation defaults as
06 describes (`systemd-run -p Delegate=yes` correct bare-host path); PDEATHSIG
mechanics as 03/06 describe (thread nuance → F7); rootless device recipes
(`--device` = bind-mount rootless; keep-groups = the group-mapping workaround);
all pinned ports ≥1024 (no capability needed under `--cap-drop=ALL`); A3/A5
surface rules hold as written.

## Needs runtime test (merged spike list)

1. Headless compositor on the target: `AQ_NO_KMS_REQUIREMENT=1` (+`AQ_DRM_DEVICES`) start; output creation; resolution/scale; wayvnc capture + resize path (prior audit wayvnc#326).
2. Software substrate for the CI lane (can the chosen build run llvmpipe at all) + the F1 override end-to-end.
3. In-container cgroup delegation wiring (podman invocation yielding writable/delegated cgroupfs; create + limits + `cgroup.kill`); F6 string vs the current ro-cgroupfs reality.
4. Manager-death cleanup (SIGKILL → what survives; startup sweep against stale cgroups; service-unit KillMode claim).
5. Renderer re-verification mechanism (no CDP) — wrapper/log-channel or screencopy readback.
6. In-binary screencopy client against the chosen Hyprland/Aquamarine build.
7. View credential + media handoff E2E (browser cookie path + raw WS consumer over non-loopback); `/media/<id>` edge cases (concurrent fetches, interrupted download, TTL).
8. Input grant policy assertion (virtual-input protocols on the pinned build).
9. Idempotency across restart; cursor stability; concurrent `get_view`; readiness healthcheck; MCP era vs actual harness matrix.

## Box answers — reference hardware for P1 routing (raw evidence in `evidence/`)

Checked live 2026-10-15 (UTC):

- **Compute:** Xeon W-2133 — 12 threads. **~125 GiB RAM** (not 64G — correction for the lane notes).
- **GPU:** NVIDIA **Quadro K2200** (GM107GL, PCI `10DE:13BA`, Maxwell). Host driver = **nouveau**; no proprietary module, no Container Toolkit → **no CDI route here**; no Vulkan (pre-Turing); GL/EGL via Mesa `nvc0`.
- **Render node:** `/dev/dri/renderD128`, mode **0666** (world-rw); card1 = 0660 +ACL, no ACL for our uid → `host` backend stays unavailable to this user (consistent with B6).
- **Rootless podman device passthrough: VERIFIED.** Full `devices` entry via the podman REST create API; ran with `cap_drop: ["ALL"]`; `renderD128` opened `O_RDWR`; DRM ioctl → `nouveau` 1.4.2.
- **GL rendering from the container: VERIFIED (GLES level).** Surfaceless EGL 1.5 (Mesa 25.2.8) → context OK; `GL_RENDERER = NV117` (nouveau gallium — real GPU, not llvmpipe); GLES 3.2. The GBM-platform EGL route crashed in our probe scripts; `eglinfo`'s GBM attempt failed init without an explicit device — spike must pin the EGL platform the headless backend uses.
- **In-container cgroupfs: read-only by default** (our pod and sibling containers). Host `user@1001.service` delegates `cpu io memory pids` → wiring is available, not default.
- **Host runtime:** Fedora-family (Aurora) 44, kernel 7.1.4, rootless podman 5.8.4 (crun 1.28), cgroup v2.

**Routing implication:** this box carries the generic render-node chain leg (rootless podman → render node → headless Hyprland → input → capture → wayvnc → noVNC → real WebGL string) — exercisable on real GPU via Mesa/nouveau, **not** a test of the NVIDIA-proprietary CDI path (that leg belongs on the peer's GTX 1060 / driver 550 box). The llvmpipe CI lane can run anywhere; this box has headroom.

**API mechanics note (cross-verification):** podman 5.8.4 REST create applies `devices` correctly with full `LinuxDevice` fields — but `inspect` does **not** reflect it (`HostConfig.Devices` stays `[]`); verify inside the container, never via inspect.

---

## Appendix — planning counters (for lane hop 2/6)

- **P0 exit** = reconciled freeze commit **+** an explicit "open-items-admitted-into-build" list with decision deadlines (B9 rate limit → before P3; Hyprland range + GPU recipes → P1 exit).
- **P1** splits by hardware and risk: (L1) generic render-node chain on *our* box; (L2) NVIDIA/CDI + nested-container delegation on the peer box; (L3) llvmpipe CI harness — either/both. Exit = chain proven + renderer string captured from an actual browser + committed harness with evidence files.
- **P2/P3 overlap:** contract-first only — pull the fake-manager + MCP conformance fixtures up to the P1/P2 boundary; then parallel per component.
- **P4/P5** unchanged.
- **Workflow refinements:** (1) durable progress/thread index lives **in the repo**, threads are read-when-on pointers; (2) cross-verification standard = commands + raw outputs committed per claim (this pass models it); resume pack = branch · head SHA · last green · next step · open questions.
