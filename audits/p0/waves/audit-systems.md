# dAIorama — systems / feasibility audit (wave: audit-systems)

Lens: architecture, session/view lifecycle, containment, GPU/rendering, the P0 spike chain, packaging/image claims.
Scope read (read-only): `docs/00-index.md` … `docs/11-decisions.md`, `docs/90-appendix/design-audit.md`, `docs/90-appendix/daiorama-design.md`.
Adversarial but cited: every external claim was checked against live sources (URLs inline). Nothing in this file is a style opinion.

Reference environment used for feasibility checks (observed, not assumed):
- Agent container on the target host: kernel `7.1.4-200.fc44.x86_64`; `/sys/fs/cgroup` mounted `cgroup2 (ro,nosuid,nodev,noexec,relatime,nosymfollow,seclabel,nsdelegate)`; `mkdir /sys/fs/cgroup/x` → `Read-only file system`; `cgroup.controllers` = `cpu io memory pids`; no `/dev/dri` inside the agent container. Host GPU: NVIDIA `10DE:13BA` (Quadro K2200, GM107), nouveau, no Container Toolkit.
- Fedora 44 packages checked: `wayvnc` 0.9.1-4.fc44 present; `hyprland` **absent** for f43/f44 (package orphaned, last build f42 0.45.2) — see F9.

---

## Findings

### F1 — Renderer policy is not implementable as written: the mandated llvmpipe CI lane hits an unconditional refusal, and the promised browser-launch re-verification has no mechanism
**Severity: blocker**
**Citation (06 — Sessions and views, §Creation):** "if the active renderer is detected as software (`llvmpipe` or equivalent), creation **refuses**: `renderer_refused` with the string in 04."
**Citation (09 — Packaging, §Testing seams = spike exit criteria):** "a CI lane running protocol-level tests against the `headless` backend under software rendering (correctness only)".
**Problem:** two normative texts require incompatible things: a CI lane that creates real sessions on the `headless` backend under software rendering, and a creation path that refuses exactly that. 09's "phrasing tension resolved" paragraph resolves the *policy* question (llvmpipe fine for tests, never for real sessions) but no mechanism exists to express it: no config key, no env var, no flag anywhere in 00–11 can turn the refusal off. Separately, the second half of D6 — "it is re-verified at first browser launch, and a software result then marks the session `renderer: software`" (06) — has no executor: the browser is a grandchild launched by the compositor's own `exec` (01), the manager holds no handle to it, and by design there is no automation protocol to ask it (no CDP). Additionally, Hyprland's own software path is not guaranteed: upstream has said software rendering is "not planned" if `WLR_RENDERER_ALLOW_SOFTWARE=1` doesn't work (hyprwm/Hyprland#3516), so the CI lane may not even be runnable on modern Aquamarine.
**Why it matters for build:** the spike's exit criteria (C7) are the P0 gate for the whole project; as written the CI lane cannot create a session, and the renderer re-verification — the one check the spec calls the failure that must never happen quietly — has no defined implementation to build.
**Proposed disposition:** (a) add one manager-side config key, `renderer.allow_software` (default `false`; one-nesting-level dot-path per 08; listed in 04's defaults table; entered in 11) — CI sets it, agents cannot (no CLI flag, so it can't be a runtime route around the fingerprint policy); (b) state the re-verification mechanism explicitly — e.g. re-read the compositor's GL renderer via the same health query 03 already defines, plus a session-owned launcher wrapper that captures the browser's own rendered-GL string from its logging channel into a manager-readable file — and add that mechanism to the spike exit criteria; (c) reword 03's "the compositor has no software-only fallback path" to "no *supported* software-only path", since C7 depends on software rendering being possible.

### F2 — The `cgroup.kill` "completion guarantee" has no executor when the manager itself dies; nothing is allowed to reconcile the leftovers
**Severity: major**
**Citation (03 — Architecture, §Ownership seams):** "Crash and close paths run `cgroup.kill` on the session's cgroup — the completion guarantee — with the parent-death signal as the belt to that suspenders."
**Citation (03, §Ownership seams):** "Empty after restart. … Nothing is reconciled; nothing could be."
**Problem:** `cgroup.kill` is code — it runs only while the manager is alive. On a manager SIGKILL/OOM (exactly the case PDEATHSIG exists for), no code runs: Hyprland and wayvnc die (direct children, PDEATHSIG SIGKILL — man7 PR_SET_PDEATHSIG(2const)), but everything Hyprland exec'd (browsers and their trees, the whole reason for the cgroup path, per the audit's B4) is only *usually* killed by wayland socket teardown. Orphaned trees keep their session cgroup; the new manager, by rule, reconciles nothing, so they leak GPU and memory forever on a non-systemd bare host. Two further mechanics the spec doesn't state: (i) PDEATHSIG's "parent" is the *thread* that forked (man7), so a Rust async manager forking children from a worker thread that later exits kills sessions spuriously; (ii) the bare-host path 06 recommends, `systemd-run --user … -p Delegate=yes`, is a *scope* — systemd.scope(5) is explicit that "scope units have no 'main' process" and their lifecycle tracks "the existence of at least one process", so a dead manager does **not** stop the scope or sweep the tree.
**Why it matters for build:** the containment/cleanup story is stated as a kernel-backed guarantee; as written it holds only for clean-ish failures. Orphaned desktops holding the render node directly break the flagship "shared instance" economics and the "no zombies" promise (B3).
**Proposed disposition:** (a) mandate per-session cgroup paths under an instance-owned delegated subtree and spec a **startup sweep** in 03 (the registry cannot be reconciled; cgroups and orphans can — say so); (b) bless a systemd *service* unit (KillMode=control-group default kills "all remaining processes in the control group" on unit stop — systemd.kill(5)) as the recommended bare-host supervision, and note scopes do not give this; (c) add the fork-thread constraint as an implementation note; (d) runtime-verify (spike list).

### F3 — Dead-state housekeeping and history retention both key off `close`, which nothing performs with the reaper off by default
**Severity: major**
**Citation (06 — Sessions and views, §Session state machine):** "the reaper closes them on its next pass (`reaper.sweep_dead = on` by default when the reaper is enabled)".
**Citation (04 — CLI surface, §Pinned defaults):** "idle reaper | off (06, rationale in 11)" and "history | 8 segments × 4 MiB per session; retention 7 days after close (08)".
**Problem:** with defaults, the reaper is off, so dead sessions are closed by nobody: B3's zombie concern ("The dead state is precisely where zombies would accumulate") reopens under the default configuration — registry entries, and their per-session history segments, accrete for the manager's whole life. Retention is defined "after close", so a session that is never closed (manager killed mid-life, dead session never swept) is never deleted either: at 8 × 4 MiB the disk cost is bounded per session but unbounded in count — ~32 MiB per abandoned session on a long-lived shared instance.
**Why it matters for build:** the sweep is the only mechanism that makes "dead ≠ closed" workable; gating it behind an off-by-default feature recreates the exact accumulation the audit flagged.
**Proposed disposition:** split the key: `reaper.sweep_dead = on` as its own default (dead sessions hold no live resources — sweeping one changes nothing an operator relies on), independent of `reaper.idle`; key history retention to session *end* (dead/reaped/closed) plus the existing segment cap, and record both in 11.

### F4 — Hot-command classification lazily starts a manager for commands that cannot need one
**Severity: major**
**Citation (04 — CLI surface, §Cold vs hot):** "‘Hot’ = may lazily start a local manager when none is reachable." — table row: "| hot | `new`, `close`, `control`, `view`, `drain`, `daemon end` |".
**Problem:** these five non-`new` commands all act on state that cannot exist without a manager. With this classification, `daiorama daemon end` on a quiet box starts a manager in order to end it; `close <id>` on a fresh box starts a manager, fails `session_not_found`, and leaves the manager running — an accidental daemon, the exact thing 01 principle 2 ("inert, not … accidentally running things") and the cold-class rationale forbid.
**Why it matters for build:** this is a behavioral contract (agents and scripts string-match startup lines); building it as written produces real, visible absurdity. A session-scoped command with no manager has an honest answer already pinned: `no manager running`.
**Proposed disposition:** hot = `new` only. Move `close`, `control`, `view`, `drain`, `daemon end` to cold. Keep the pinned startup line (`started manager (mode=…, pid=…, socket=…)`) for `new` alone.

### F5 — 09 re-introduces the single "VNC port" that 06 declares dead
**Severity: major**
**Citation (09 — Packaging, §Container image):** "Common-case compose stays genuinely simple: one GPU line, MCP port, VNC port, optional volume."
**Citation (06 — Sessions and views, §Views):** "Several viewed sessions coexist on consecutive ports; the old single-"VNC port" compose assumption is dead."
**Problem:** views get one front port each, allocated upward from `view.port_base` (6080, 04); a compose snippet publishing one "VNC port" serves exactly one view. This is the same defect A1 fixed ("§9's compose 'a VNC port' cannot serve multiple viewed sessions as written") and B2's fix explicitly buried — it survives in 09. It also silently breaks multi-instance hosts (08 supports several instances; each allocates from the same 6080 base).
**Why it matters for build:** 09's snippets "are the documentation" (09) — this one is wrong for the default `headless`-with-`--view` shape and for the shared/central shape.
**Proposed disposition:** replace the line with an explicit range mapping (`-p 127.0.0.1:6080-6099:6080-6099`) or state the required published range equal to `view.port_base` + max concurrent views; add a per-instance `view.port_base` note to 08 for multi-instance hosts.

### F6 — C6's "fail loud" containment check has no error code, no string, and no container-side fix in the packaging docs — and the reference environment fails it by default
**Severity: blocker**
**Citation (06 — Sessions and views, §Per-session resource limits):** "If unavailable: fail loud with the exact next step by default; `limits.mode = off` is the explicit, recorded downgrade".
**Problem (verified):** the precondition failure is real and current — the reference agent container mounts `cgroup2 (ro,…,nsdelegate)` and cannot create sub-cgroups (observed; `mkdir` → `Read-only file system`; controllers present: `cpu io memory pids`). So with defaults, the manager's startup delegation check fails, and the first-listed deployment shape (01: "embedded in a single agent's own container") cannot start at all. Yet: 05's canonical code list has **no** containment/limits-availability code; 04's pinned strings have none either, though A6/D2 promise that every failure is greppable and stable; 09's image guidance ("one GPU line, MCP port, VNC port, optional volume") says nothing about the cgroup mount, so the flagship compose is OOTB broken on the target box; and 03's lazy-start loser rule ("retrying once if the winner died pre-bind") is undefined for the case that actually occurs here — the winner dies *after* winning but at the delegation check (retry loop? same error?).
**Why it matters for build:** the failure surface for the most likely startup failure on the real host is unpinned; a builder must invent the code, the string, the check placement (manager startup vs session creation), and the retry outcome.
**Proposed disposition:** pin code `limits_unavailable` + a 04 string naming both fixes (container invocation with a writable/delegated cgroupfs; or `limits.mode = off` as the recorded downgrade); state the check runs at manager startup and `doctor` reports the same in-container; add the cgroup-mount line to 09's snippets; state the loser outcome (surface the same error; do not loop).

### F7 — The GPU recipe table has no path for NVIDIA-on-nouveau; the reference GPU is exactly that
**Severity: major**
**Citation (09 — Packaging, §Container image):** "NVIDIA — CDI (`--device nvidia.com/gpu=all`), host requires Container Toolkit ≥ 1.18 (auto-refreshing CDI spec)."
**Citation (03 — Architecture, §GPU access):** "Container passthrough specifics, per GPU family, live in 09." (consequence: "no hardcoded device path … a single config value: a render-node path **or** a CDI device identifier.")
**Problem:** the target GPU (10DE:13BA / Quadro K2200, GM107) is bound to nouveau with no proprietary module and no Container Toolkit; CDI-with-NVIDIA is built around the proprietary driver stack (NVIDIA Container Toolkit CDI docs), and NVIDIA's own guidance is to blacklist nouveau where the container stack is used. So the CDI row cannot apply, and the render-node row is labelled "AMD/Intel" — meaning `doctor`'s family dispatch has no rule for the box the project is being built against, even though the generic render-node recipe (`--device /dev/dri/renderD128` + keep-groups) is the correct and portable answer (it also yields a real Mesa GL_RENDERER, so the fingerprint check passes).
**Why it matters for build:** 03 makes the recipe table normative-by-reference ("live in 09"); a builder implementing family detection per that table has to invent the nouveau case.
**Proposed disposition:** restate the recipes vendor-agnostically: "any `renderD*` render node → render-node recipe"; keep CDI as the NVIDIA-*proprietary* alternative only; add an explicit nouveau row to `doctor`'s detection matrix (nouveau ⇒ render-node recipe, CDI not applicable).

### F8 — The headless path ordering is inverted for the container shape: the render-only env is a prerequisite, not a fallback
**Severity: major**
**Citation (03 — Architecture, §Backend modes):** "try Hyprland's dedicated virtual-output feature first; if flaky in practice, the render-only DRM path (`AQ_NO_KMS_REQUIREMENT=1`) is the documented fallback under the same CLI name."
**Problem:** the two paths are not alternatives in the flagship (container, render-node-only) shape. Hyprland "requires by default that your graphics card has at least one display output" and "will, by default, only select nodes on the system with display outputs and ignore the render node"; the wiki's documented fix is `AQ_NO_KMS_REQUIREMENT=1`, which makes the render node a *selectable* node (wiki.hypr.land Virtual-GPU). And `hyprctl output create headless` is a **post-start** action (wiki Using-hyprctl; forum evidence: Hyprland starts without a monitor and "sits happily … until I run `hyprctl -i 0 output create headless`", while window-open-without-monitor historically crashed — later patched upstream). So in a container the compositor does not start at all without the env; "trying the dedicated feature first" means Hyprland never comes up and the spike misattributes the failure to the virtual-output feature.
**Why it matters for build:** this is the ordering of the P0 spike's central step; getting it inverted burns the spike's budget and mislabels the fallback.
**Proposed disposition:** reword 03: `headless` = render-only DRM init (`AQ_NO_KMS_REQUIREMENT=1`; AQ_DRM_DEVICES where multiple nodes) as the container prerequisite; the dedicated virtual output (`hyprctl output create headless`) is the output-creation mechanism layered on top, used to pin resolution/scale per session; spike verifies the combination plus the wayvnc resize path (the prior audit's wayvnc#326 note).

### F9 — Fedora's Hyprland package is orphaned and absent for f43/f44; the pinned "exact install command" refusal cannot be honoured on the target distro
**Severity: major**
**Citation (04 — CLI surface, §Refusal and error strings):** "`Hyprland not found. Install it: {exact command for the detected package manager}, or use the container image.`"
**Citation (09 — Packaging, §Bare-host dependencies):** "missing: exact install command for the detected package manager + point at the image; out of range: named-mismatch error".
**Problem (verified):** packages.fedoraproject.org lists hyprland only for Fedora 42 (0.45.2-1.fc42) and 41 (0.44.1-1.fc41), maintainer **orphan** — no f43/f44 builds at all (wayvnc is fine: 0.9.1-4.fc44). On the target distro the refusal would print a `dnf install hyprland` that fails, and the version-range check has no in-repo build to compare (upstream is 0.5x; distro artifact is 0.45-era). A bare-host Fedora 44 user is therefore pointed at a dead end while 09's table promises a command.
**Why it matters for build:** this is a pinned operator-visible string and a named dependency-matrix cell; both are wrong for a distro the project runs on.
**Proposed disposition:** make the install-command resolution a maintained table in the release manifest (distro/release → package → version, with a "not packaged / out of range" row routing to the container image or a pinned upstream source), and state in 09 what the named-mismatch error says when the distro package is missing. Add to the pre-freeze checklist: re-check the table at each release.

### F10 — `clip_video`'s resource_link is unfetchable in the flagship network-bridge shape as specified
**Severity: major**
**Citation (05 — MCP bridge, §Capture, media, and the content-type rule):** "the manager serves captured media on the same address family as its RPC surface, path `/media/<opaque-id>`, under the same exposure + token rule as every other surface (07)."
**Problem:** the manager's default RPC surface is a tokenless **unix socket** (07 table) inside dAIorama's container. The flagship caller (09: "published port — agent's container → dAIorama's container") is a harness in *another* container, talking MCP to a **bridge**, holding scope credentials — not the instance token — and MCP resource fetching has no standardized credential channel. Nothing in 05/07 says who serves `/media/<id>` to that harness, what scheme a unix-socket "address family" URI has, or how the instance token is presented on the fetch. A8's lifetime rule is precise; its reachability is not.
**Why it matters for build:** `clip_video` is a core, pinned tool (05 §Tool exposure, "screen capture (non-optional)"); the profile of the returned resource_link is a caller contract that cannot be implemented as written.
**Proposed disposition:** decide and pin, per transport: bridge-proxied media (URI base = the bridge's listen address; the bridge presents the instance token to the manager — consistent with the bridge being the harness's network entry), or a one-time credential embedded in the URL minted by the manager; state the URI scheme per shape (http for TCP, explicit for the unix-socket case) in 05 and add the endpoint to 07's surface table as its own row when it rides the bridge.

### F11 — Idempotency dedup state vs manager restart is undefined
**Severity: major**
**Citation (05 — MCP bridge, §List, history, and retries):** "The manager deduplicates within a bounded window (10 minutes or the session's lifetime, whichever is shorter) and returns the original result for duplicates."
**Citation (03 — Architecture, §Ownership seams):** "the session registry always starts empty after the manager restarts. Nothing is reconciled".
**Problem:** nothing states where dedup state lives or what happens across a restart — and the documented retry rule ("on `manager_unreachable` or a timeout, a client may retry **once** with the same key") is precisely the path where a manager restart intervenes. After a restart: the dedup table (if in-memory) is gone, the original session is dead by design, and a same-key retry either silently creates a **second** desktop or fails in a way no clause defines. A6's rationale — "an at-least-once retry of `new` duplicates desktops" — is then unmet in the exact scenario that justification exists for.
**Why it matters for build:** duplicating desktops on retry is a user-visible, resource-costly bug class the design claims to close; the builder currently has to guess the restart semantics.
**Proposed disposition:** add one sentence to 05: dedup state is per-manager-lifetime, in memory, lost on restart; after a restart the key MAY be re-executed because the original session died with the manager — and therefore `create_session` results carry the manager's instance ID/generation (see F12) so a client can tell "retry deduplicated" from "new desktop created".

### F12 — Session IDs are not required to be unique across restarts; a cached ID can silently address a different desktop
**Severity: major**
**Citation (02 — Glossary / 06 §Creation):** "IDs are stable and opaque; labels are mutable and never identifiers."
**Citation (06 — Sessions and views, §Views):** "so a cached address from before a view crash is detectable as stale (generation moved) instead of silently dead."
**Problem:** the spec solved exactly this class of staleness for view *addresses* (generation), for view *state* (registry), and for protocol (*handshake*), but not for session IDs: nothing forbids a per-manager counter. Agents cache IDs (A7's own premise), the registry is empty after every restart, and history retains old sessions' IDs — so a stale cached ID can hit a brand-new, different session, and a control call lands on the wrong desktop. That is a wrong-target hazard, not an ergonomics nit.
**Why it matters for build:** ID generation is a one-line decision every builder must make; making it wrong is silent and dangerous, and it interacts with the empty-registry rule, the retry rule (F11), and history.
**Proposed disposition:** mandate globally unique opaque IDs (UUIDv4/ULID) in 02/06, or keep any scheme but add a manager instance-birth generation to session records and to the MCP/RPC session detail, with `session_not_found` (hint: "session from a previous manager run") when a client presents a foreign-generation ID. Record in 11.

### F13 — View-credential issuance is unspecified, yet the caller contract (the returned address) depends on it
**Severity: major**
**Citation (07 — Network surfaces, §The exposure rule):** "checks a token-derived credential (cookie or one-time code — a browser cannot send an Authorization header to a VNC endpoint, and wayvnc implements no such token), and proxies to a loopback wayvnc."
**Citation (05 — MCP bridge, §Views over MCP):** "starts a view if none is running …, returns its address either way."
**Problem:** "cookie **or** one-time code" is two different front APIs and two different caller workflows. Nothing says who mints the credential, when, with what TTL/use-count, whether `get_view`'s returned address already carries it (an agent's non-browser consumer cannot set cookies; a one-time code must then be in the URL or in an explicit field), or how the dashboard (a different process, possibly a different host) obtains one for its embedded widget. As written, `get_view` returns an address that a non-loopback caller may not be able to use, and no other call can obtain a usable one.
**Why it matters for build:** `get_view` is a pinned tool with a pinned seam ("a discrete MCP call hands off to a standing stream"); the handoff cannot be implemented without choosing the credential shape.
**Proposed disposition:** pin one mechanism: manager mints a single-use, short-TTL code, `get_view`/`view` return it as an explicit field alongside the address (never logging it), the front consumes it on first request and sets its own session cookie for browsers; dashboard gets one the same way through its manager connection. Add to the spike (noVNC end-to-end in a browser; raw WebSocket consumer without cookies).

### F14 — How a session's Hyprland config is supplied (and in which dialect) is unspecified, just as the dialect is changing upstream
**Severity: minor**
**Citation (09 — Packaging, §Version-check pattern):** "startup checks Hyprland's IPC surface against a known-compatible range and fails with a specific, named error".
**Problem:** resolution/scale are creation parameters (03/04) and the session env (AQ_*, XDG_RUNTIME_DIR, WAYLAND_DISPLAY) must reach the compositor, but no file says how: a generated per-session config, `hyprctl keyword` after start, or both. The version gate as written covers the *IPC surface* only, while upstream changed the config dialect recently ("Since Hyprland 0.55, hyprlang is deprecated in favor of lua" — wiki.hypr.land); a generated config is a launch-time failure surface that the IPC-surface check will not catch.
**Why it matters for build:** every session start writes (or doesn't write) this file; the choice interacts with the version-compat range and the "no hardcoded anything" spirit of 03.
**Proposed disposition:** one sentence in 06: each session gets a generated compositor config under its runtime dir, written by the manager before exec; extend 09's version gate to pin the supported config dialect as well as the IPC surface.

---

## Audit-item resolution check (A1–A8, B1–B10, C1–C8, D1–D6)

Method: each item's audit text re-read against the numbered files, then against the numbered file's own downstream claims.

| item | verdict | note |
|---|---|---|
| A1 | resolved | full tree/flags/defaults/strings in 04; bridge entry named; ports pinned. (09 later re-breaks the port part by re-publishing a singular "VNC port" — F5.) |
| A2 | resolved | per-key authority table + reload semantics; `config set remote.default` works by rule. |
| A3 | resolved | 07 is a complete surface table with one request-time gate; front named. (Credential mechanics left open — F13.) |
| A4 | resolved | network = name+secret per connection; unix socket cooperative, documented as such. |
| A5 | resolved | generate-on-first-non-loopback-bind, single rule, rotation consequence stated. |
| A6 | **gap** | envelope/codes/pagination/keys present, but the C6 failure has no code or string — the contract's own "every failure is a stable envelope" is unmet (F6). |
| A7 | resolved | overlap rule formula correct; additive-only; dashboard degrades. |
| A8 | **gap** | lifetime/TTL/persistence precise; fetch path unreachable for the flagship caller (F10). |
| B1 | resolved | activity redefined (RPC ∨ view ∨ input), watched-sessions exempt, default off recorded with recommendation. |
| B2 | **re-introduced** | 06 correct (per-view ports, generation, lock, kill-on-death), but 09 resurrects the single "VNC port" compose claim (F5). |
| B3 | resolved-with-gap | FSM + quota reading are right; the sweep that makes them work is gated on the off-by-default reaper (F3). |
| B4 | **overclaim** | scope stated correctly for direct children, but the cgroup.kill "completion" cannot execute on manager death and nothing may reconcile (F2). |
| B5 | resolved-with-defect | cold/hot, flock, report line all present; the hot set itself now starts daemons for commands that can't need one (F4). |
| B6 | resolved | display-node claim scoped; device-global exclusivity via card-node flock; `backend_busy` string pinned. |
| B7 | resolved | probe-before-adoption; foreign compositor sockets refused; boundary-inversion documented as the reason. |
| B8 | resolved | scope on history records; dashboard = named scope with explicit broad-read grant. |
| B9 | resolved (open, tracked) | requirement + `rate_limited` code + must-decide-before-ship marker in 11. Correctly left open. |
| B10 | resolved | pull-only v1; dial-out demoted to a separate future mechanism; "no special coupling" denial removed. |
| C1 | resolved | asset/version/checksum/atomic-swap/offline/container-path all specified. |
| C2 | resolved | signed manifest, one core binary + separate module, client-only mode, digest pinning. |
| C3 | resolved | drain + procedures per shape + versions in readiness + skew display. |
| C4 | resolved | host-runnable doctor; in-container verifies renderer only. |
| C5 | resolved-with-gap | capture decided in-binary; browser provider named; the "bare-host: the user's own" browser and the version-gate table are thin for Fedora (F9). |
| C6 | **gap** | preconditions stated honestly (and confirmed real on the reference box), but the failure path has no code/string and no container-side fix in 09 (F6). |
| C7 | **not implementable** | CI lane on llvmpipe contradicts the unconditional creation refusal; no override exists (F1). |
| C8 | resolved | exact paths, instance-discriminated, container equivalents, single-sourced runtime-dir ensure. |
| D1 | resolved | first-run sequence + written strings + `new --help` surfacing. |
| D2 | resolved | status fields, end vs `--force`, one shared reachability string. |
| D3 | resolved | connection CRUD, 0600 token store, `dashboard` scope, take-over defined, rename label-only. |
| D4 | resolved | scope list/get/validate + effective; unknown names are load errors listing valid names. |
| D5 | resolved | history defaults pinned; RON rationale recorded with a stated TOML fallback at sign-off. |
| D6 | **gap** | the fork was decided, but neither half is fully implementable: no software override for the CI lane, no re-verification mechanism (F1). |

No ledger↔normative mismatches were found beyond the above (11's entries accurately describe what 01–10 say; nothing was silently re-derived). No other silent re-introductions spotted.

---

## Verified OK (checked against sources; no finding)

1. **Rootless virtual-input story.** Hyprland advertises `zwp_virtual_keyboard_manager_v1` (wayland.app lists Hyprland 0.52.1) and wlroots-family `zwlr_virtual_pointer_manager_v1`; two independent Hyprland injectors drive pointer+keyboard with no `/dev/uinput` and no root (github.com/sophronesis/xdg-desktop-portal-hyprland-input; github.com/gfhdhytghd/hypr-kdeconnect-fix), and a commercial RDP server reports both protocols "confirmed working live" on Hyprland (lamco.ai RDP docs). Caveat recorded in the spike list: the keyboard protocol reserves an `unauthorized` error, so the exact grant policy must be asserted on the chosen build.
2. **wayvnc realities.** RFB-over-WebSocket only (`ws:`, `ws-unix:`, `ws-fd:` address forms since 0.7; frames ws, never serves the noVNC HTML) — the manager-owned front is required and 07's gate-in-front design is right (man.archlinux.org/man/wayvnc.1; phoronix.com WayVNC 0.7). wayvnc does have its own VNC-level auth (`enable_auth`, TLS/PAM), but no HTTP token — 07's phrasing survives.
3. **MCP content types.** `ContentBlock = text | image | audio | resource_link | embedded resource` in both 2025-06-18 and the later 2025-11-25 revision; still no video type, so `clip_video` → resource_link is the spec's own shape (modelcontextprotocol.io/specification/2025-11-25/server/tools; schema.ts).
4. **`cgroup.kill`.** Introduced in Linux 5.14 (kernelnewbies L5.14; commit 661ee628); present as a file in the reference container's cgroup dir on kernel 7.1.4. Non-root cgroups only, write-only, fails `EOPNOTSUPP` in threaded cgroups — consistent with 03/06's usage wherever delegation exists.
5. **Delegation defaults.** Upstream (runc cgroup-v2 docs): "Typically, only memory and pids controllers are delegated to non-root users by default" — 06's claim ("memory and tasks are delegated by default, cpu usually is not") is substantively right (name the controller `pids`), and `systemd-run --user … -p Delegate=yes` is the correct documented bare-host path (systemd CGROUP_DELEGATION).
6. **PDEATHSIG mechanics.** Cleared on fork, preserved across ordinary execve, cleared on credential changes; delivered on termination of the *forking thread*, not the whole parent process (man7 PR_SET_PDEATHSIG(2const)) — 03/06's "prctl-after-fork window, re-check getppid(), non-ignorable SIGKILL" is the correct shape; the per-thread nuance is captured in F2.
7. **Rootless podman device/keep-groups recipe.** Podman docs: in rootless mode `--device` is a bind mount; SELinux needs `setsebool -P container_use_devices=true`; "if the user only has access rights via a group, accessing the device from inside a rootless container fails" (docs.podman.io podman-run); `--group-add keep-groups` is the documented crun workaround for exactly that (redhat.com supplemental-groups post; podman#10166 for the live rough edges). 09's "treat `ls -l /dev/dri/renderD128` as the source of truth" hedge is correct and should stay.
8. **Fedora 44 has wayvnc** (0.9.1-4.fc44, packages.fedoraproject.org) — the missing dep on that distro is Hyprland only (F9).
9. **All pinned ports are ≥ 1024** (manager 8756, bridge 8757, view front 6080+) → no capability needed to bind under `--cap-drop=ALL`/rootless; nothing in 01–11 requires root.
10. **A3/A5 surface rules hold as written**: unix socket tokenless with filesystem gate (docker.sock reasoning), TCP bind requires token, generation-on-first-non-loopback-bind with the refuse-only-when-impossible rule, rotation at next start with the drops-sessions consequence stated — no contradiction remains.

---

## Needs runtime test (spike items) — cannot be decided from text

1. **Headless compositor on the target (the P0 chain's core).** Container with renderD128 only (no card0): confirm `AQ_NO_KMS_REQUIREMENT=1` (plus `AQ_DRM_DEVICES` if needed) starts Hyprland; whether an output is auto-created or `hyprctl output create headless` + a monitor rule is required; resolution/scale application; wayvnc capturing that output; the resize path (prior audit's wayvnc#326); the exact env set (WLR_* compat vars may or may not matter on Aquamarine).
2. **Software-renderer substrate for the CI lane.** Whether Hyprland can be made to run on llvmpipe at all on the chosen build (upstream: software rendering "not planned" if `WLR_RENDERER_ALLOW_SOFTWARE=1` does not work — hyprwm/Hyprland#3516). If it can: prove the F1 override key end-to-end. If it cannot: C7's CI lane needs a different substrate (nested compositor, or protocol tests against fake-manager fixtures only).
3. **keep-groups on the real box.** podman 5.8.4 + crun 1.28 + SELinux enforcing, user's supplementary groups vs `renderD128`'s gid (note: a logind seat ACL will not help a headless podman run), then assert a non-llvmpipe Mesa GL_RENDERER inside the container.
4. **cgroup delegation in-container.** Find and document the podman invocation that yields a writable/delegated cgroupfs (cgroup namespace / cgroup driver choice); verify create + `memory.max`/`cpu.max` + `cgroup.kill` in-container; verify F6's failure string against the current ro-cgroupfs reality and the loser-retry outcome.
5. **Manager-death cleanup (F2).** SIGKILL a manager: confirm direct children die; measure what survives of the grandchild tree and the session cgroup; prove a startup sweep can find and `cgroup.kill` stale session cgroups; verify the systemd service-unit supervision claim (`KillMode=control-group` on unit stop) on Fedora 44.
6. **Renderer re-verification at first browser launch (F1b).** Prove a mechanism that reads the browser's actual rendered-GL string without any automation protocol (candidate: session-owned launcher wrapper capturing the browser's logging channel; or compositor GL_RENDERER + a rendered WebGL probe page read back by screencopy).
7. **In-binary screencopy client (C5).** Assert `zwlr_screencopy_manager_v1` capture works from a Rust client against the chosen Hyprland/Aquamarine build (watch the `ext-image-copy-capture-v1` interop and any newer screencopy permission gate), including headless outputs.
8. **View credential + media handoff end-to-end (F13/F10).** Browser (noVNC, cookie path) and non-browser WebSocket consumer (one-time-code path) through the manager front over a non-loopback bind; and a `clip_video` resource_link fetched by a harness in a second container.
9. **Input grant policy.** On the pinned Hyprland build, assert an unprivileged client is actually granted `zwp_virtual_keyboard_manager_v1`/`zwlr_virtual_pointer_manager_v1` (the protocol's `unauthorized` error exists; newer Hyprland versions keep adding permission surfaces).
