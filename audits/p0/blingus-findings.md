# P0 audit pass — BlingusBot (independent)

status: **DRAFT v0.1** — own-lens findings + reference-hardware box probes.
A second commit merges three independent subagent lenses (systems /
contract / security) after cross-verification; numbering then becomes final.
audited: `spec/v1` @ `f4e4cd1`
method: full read of 00–11 + `90-appendix`; live box probes on the reference
hardware (raw outputs in `audits/p0/evidence/`); subagent wave merge pending.

Severity key (same as `90-appendix/design-audit.md`): **blocker** = cannot
build as written without guessing / breaks in normal use · **major** = causes
rework or user-visible inconsistency · **minor** = polish.

---

## Findings (provisional — wave merge pending)

### F1 — blocker — The specified CI lane cannot run: refuse-at-creation vs protocol tests under llvmpipe
**Cite:** 06 "Renderer policy at creation" — *"if the active renderer is
detected as software (`llvmpipe` or equivalent), creation **refuses**"*; 09
"Testing seams" — *"a CI lane running protocol-level tests against the
`headless` backend under software rendering"*.
**Problem:** no config key, flag, or documented path permits creating a
session under a software renderer, so the C7 CI lane as written cannot run
`headless` sessions at all. Related unpinned piece in the same decision:
the "renderer unverifiable" path promises an "explicit warning" but no
user-visible string is pinned next to a pinned refusal string (D6/04), and
"re-verified at first browser launch" names no detection mechanism or
surfacing channel.
**Disposition:** add an explicit test-only override (e.g.
`renderer.software = refuse | allow_marked`, default `refuse`) that the CI
lane sets; pin the unverified-warning string in 04 alongside the refusal;
state the detector (compositor-side renderer string at session start +
browser-side re-check) and where `renderer:` state surfaces (`session
detail`).

### F2 — major — View front credential flow is not specified end to end
**Cite:** 07 — *"checks a token-derived credential (cookie or one-time code
— …)"*; 04 — `daiorama view <session>` *"print the view address"*.
**Problem:** nothing says how a browser obtains the credential: which
command mints it, how it is delivered (URL fragment? code prompt?), whether
it covers noVNC page + assets + the WebSocket upgrade, what happens with
two viewers (dashboard + agent), and what loopback vs non-loopback changes.
The gate exists as an enforcer with no issuance flow.
**Disposition:** pin one flow — e.g. `view`/`get_view` return
`address + one-time admission code` (code only over the authenticated
channel); first page fetch exchanges code → HttpOnly session cookie; the
front then gates page, assets, and WS uniformly; loopback stays tokenless.
Spell out multi-viewer (N sessions allowed) and code lifetime.

### F3 — major — Network-bridge scope authentication has no pinned presentation path
**Cite:** 05 — *"the connection presents a scope name and, where the scope
has a secret, that secret; the manager validates it on the same
presentation path as the instance token"*; 07 scope model.
**Problem:** "presents" is undefined for the flagship shape (agent container
→ central manager over a network MCP port). MCP transports differ (stdio vs
HTTP/streamable); stdio uses env (`DAIORAMA_SCOPE`), but the network leg
needs a concrete carrier (headers? pre-handshake?). A builder must guess;
an MCP client without header customization cannot connect as scoped.
**Disposition:** pin the carrier explicitly (e.g. HTTP/WS headers
`X-DAIORAMA-SCOPE` + `X-DAIORAMA-SCOPE-TOKEN` on the network bridge
transport, validated per connection) and state the fallback for harnesses
that cannot set it (stdio bridge, or a network bridge with default-scope
disabled).

### F4 — major — Cgroup absence degrades two contracts; the downgrade path names only one
**Cite:** 03 ownership seams — *"Crash and close paths run `cgroup.kill` on
the session's cgroup — the completion guarantee"*; 06 C6 — *"`limits.mode =
off` is the explicit, recorded downgrade"*.
**Problem:** the per-session cgroup is simultaneously (a) the limits
mechanism and (b) the only grandchild-cleanup guarantee (PDEATHSIG covers
direct children only, 03/06). The recorded downgrade describes (a) only;
with no cgroups, (b) silently disappears while the spec still describes it
as the completion guarantee.
**Disposition:** split the two: session cgroup for `cgroup.kill` =
operation-required (fail loud if uncreatable — already the default);
`limits.mode = off` disables limit writes but keeps the cgroup. If a
no-cgroup mode is to exist at all, its record must state the cleanup
caveat.

### F5 — major — Dead sessions are never swept under default config
**Cite:** 06 — *"the reaper closes them on its next pass (`reaper.sweep_dead
= on` by default when the reaper is enabled)"*; 04 defaults — reaper `off`;
11 B3 — *"swept by the reaper (or operator) promptly"*.
**Problem:** with defaults (reaper off), nothing ever closes dead entries;
the registry and `list` accumulate dead sessions indefinitely, and 11's
"promptly" does not hold. Dead = no quota cost, but it is list/manage
noise and a zombie-entry class the audit warned about.
**Disposition:** decouple the dead-entry sweep from idle reaping (an
always-on passive sweep with no session at risk — e.g. closed on any
manager activity or a lightweight timer) or state the default behavior
honestly in 06 ("with the reaper off, dead sessions persist until closed").

### F6 — major — `clip_video` media URIs may be unreachable in the flagship shape
**Cite:** 05 A8 — *"the manager serves captured media on the same address
family as its RPC surface"*; 07 surfaces table.
**Problem:** in the network-bridge shape, the caller reaches the manager
through the bridge; the `/media/<id>` URI points at the manager's surface,
which may not be reachable from the caller's network position. The spec
never says who serves the fetch when the caller is remote — and the
artifact self-destructs at first fetch, so a wrong guess loses the clip.
**Disposition:** pin it — recommended: the bridge proxies `/media/<id>`
fetches for network consumers (same token + scope rules), or the spec
requires exposing the manager surface to media consumers. State the token
carriage for the fetch (HTTP client with token; browsers not involved).

### F7 — minor — Hyprland supported version range has no values
**Cite:** 04 string — *"Hyprland {found} is outside the supported range
{min}-{max}."*; 09 — *"presence + version range"*.
**Problem:** the string pins a range the spec never pins; a builder can't
implement the check exactly as contracted.
**Disposition:** record the initially validated window at spike time
(literal values in 09 + compat table); keep the string template as is.

### F8 — minor — Media fetch-once edge semantics undefined
**Cite:** 05 — *"deleted at first successful fetch or after TTL (default
60 s), whichever comes first"*.
**Problem:** "successful" undefined for an interrupted/partial fetch
(artifact destroyed by a half-download?); concurrent fetches race with no
stated winner; no re-fetch story for a client that lost the transfer.
**Disposition:** define precisely (e.g. delete after the response is fully
written; single consumer, second fetch `404`; or keep TTL-only with a fetch
counter). Small text change, big debugging save.

### F9 — minor — Detached-mode diagnostics have no destination on a bare host
**Cite:** 10 — *"Manager diagnostics: stderr → journald / `docker logs`. No
bespoke log files."*; 04 — `daemon start` *"detached (survives shell
exit)"*.
**Problem:** `daemon start` on a bare host with no supervisor gives stderr
no sink, while the spec explicitly bans bespoke log files. Operators lose
diagnostics exactly in the mode meant to survive them.
**Disposition:** pin the supported detached path (e.g. document
`systemd-run --user` / service units as the sanctioned detachment, with
`daemon start` as a dev convenience whose stderr is terminal-scoped) or
allow one deliberate log destination in the data dir.

### F10 — minor — `list` scope semantics wording conflicts across files
**Cite:** 04 — *"Default: the caller's own scope's sessions; `--all` for
everything (operator view)"*; 07 — *"sessions created from the CLI carry
[operator]; CLI reads are operator-wide"* + `sessions.visibility = all`
("MCP callers default to own-sessions-only").
**Problem:** for the CLI caller, "own scope" is already operator-wide, so
the contractual meaning of `list` vs `list --all` is ambiguous (same
result? different in some mode? does `sessions.visibility` affect CLI at
all?). Agents string-match this surface.
**Disposition:** state the resolved semantics once (suggested: CLI `list`
= operator-wide live sessions; `--all` adds closed/dead; MCP honours
`sessions.visibility`), and note the operator exception explicitly.

### F11 — minor — `--json` output schemas are unpinned where the contract style demands pinning
**Cite:** 04 — *"Read commands accept `--json`"*; D2 pins `status` field
*names* only.
**Problem:** the spec's own rule is that machine-read surfaces get pinned
(agents cache/parse). `--json` shapes for `status`, `list`, and session
detail are the primary agent-facing read surfaces and have no pinned field
set (only `status`'s names, partially).
**Disposition:** add a small schema appendix (top-level field names per
read command + bridge equivalents), additive-only thereafter. Deferring it
past freeze is acceptable only if declared an open item.

---

## Verified OK (selected)

- Renderer-policy vs surface table are consistent otherwise; scopes are
  enforced on all transports per the stated model. (Seed list — completed
  after the wave merge.)

## Box answers — reference hardware for P1 routing (raw evidence in `evidence/`)

Checked live 2026-10-15 (UTC); the box currently hosting our side:

- **Compute:** Xeon W-2133 — 12 threads. **~125 GiB RAM** (not 64G —
  correction for the lane notes).
- **GPU:** NVIDIA **Quadro K2200** (GM107GL, PCI `10DE:13BA`, Maxwell).
  Host driver = **nouveau** (kernel-side); no proprietary `nvidia` module,
  no `nvidia-smi`, no NVIDIA Container Toolkit → **no CDI route on this
  box**; Vulkan unavailable (pre-Turing nouveau); GL/EGL via Mesa `nvc0`.
- **Render node:** `/dev/dri/renderD128` exists, mode **0666** (world-rw);
  card1 = 0660 +ACL, no seat ACL for our uid → consistent with B6's
  "host backend: bare-metal only; rootless unproven".
- **Rootless podman device passthrough: VERIFIED.** Container created via
  the podman REST socket with a full `devices` entry ran with
  `cap_drop: ["ALL"]`; opened `renderD128` `O_RDWR`; DRM ioctl returned
  driver `nouveau` 1.4.2.
- **GL rendering from the container: VERIFIED (GLES level).** Surfaceless
  EGL 1.5 (Mesa 25.2.8) → GLES context created; `GL_RENDERER = NV117`
  (nouveau/GM107 gallium — real GPU, not llvmpipe); `GL_VERSION` =
  OpenGL ES 3.2 (`evidence/egl2.out`). The GBM-platform EGL route crashed
  in our probe scripts, and `eglinfo`'s own GBM attempt failed init
  in-container without an explicit device — the spike must pin which EGL
  platform the chosen headless backend uses (`evidence/egl-test.out`,
  `evidence/egl2.out`).
- **In-container cgroupfs: read-only by default** (both our main pod and a
  sibling API-created container: `mkdir /sys/fs/cgroup/...` → EROFS).
  Host side, `user@1001.service` has `cpu io memory pids` delegated, so
  the C6 precondition is satisfiable but needs the delegated-scope wiring;
  per-session sub-cgroups inside any container are not available out of
  the box.
- **Host runtime:** Fedora-family (Aurora) 44, kernel 7.1.4, rootless
  podman 5.8.4 (crun 1.28), cgroup v2.

**Routing implication:** this box can carry the generic render-node chain
leg (rootless podman → render node → Hyprland headless → input → capture →
wayvnc → noVNC → real WebGL string) on a GPU — with the caveat that it
exercises the Mesa/nouveau path, not the NVIDIA-proprietary CDI path. The
CDI leg needs a proprietary-driver box (peer's GTX 1060 / driver 550).
The llvmpipe CI lane can run anywhere; this box has ample headroom.

**API mechanics note (for cross-verification):** on the podman 5.8.4 REST
create endpoint the `devices` field applies correctly with full
`LinuxDevice` entries (`path/type/major/minor/fileMode/permissions`) —
but `inspect` does **not** reflect it (`HostConfig.Devices` stays `[]`);
verify inside the container, never via inspect. (Same family as the known
"create API silently ignores some spec fields" quirk.)

---

## Appendix — planning counters (for lane hop 2/6)

Phase plan: P0–P5 shape agreed; proposed deltas:

- **P0 exit** = reconciled freeze commit **+** an explicit
  "open-items-admitted-into-build" list with decision deadlines (B9 rate
  limit → before P3; Hyprland range + GPU recipes → P1 exit).
- **P1** splits by hardware and risk, not by phase: (L1) generic
  render-node chain on *our* box (evidence above); (L2) NVIDIA/CDI +
  nested-container delegation on the peer box; (L3) llvmpipe CI harness —
  either/both. Exit = chain proven + renderer string captured from an
  actual browser **+ committed harness with evidence files** (commands +
  raw outputs, as here).
- **P2/P3 overlap:** acceptable only contract-first — pull C7's fake
  manager + MCP conformance fixtures up to the P1/P2 boundary so both
  sides build against fixtures; then P2/P3 can proceed in parallel per
  component.
- **P4/P5** unchanged.

Workflow: adopt as proposed, with two refinements — (1) the durable
progress/thread index lives **in the repo** (a small tracked file), with
threads as read-when-on pointers; (2) cross-verification standard =
commands + raw outputs committed per claim (this pass models it). Resume
pack = branch · head SHA · last green · next step · open questions.
