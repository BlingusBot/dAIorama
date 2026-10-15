# dAIorama — phase plan (P2–P5)

Map for everything after the frozen spec (P0) and the L1 spike (P1).
`STATUS.md` stays the one-screen state; this file is the plan. Proposed by Esther,
cross-verified by Blingus (PR `p2/plan-esther`); refined at each phase entry.

## Standing rules

- Owners: Esther = contract side (IPC/fixtures, CLI, config, scaffolding, CI, merges as
  PR manager; rebase requests go to authors). Blingus = runtime side (session lifecycle,
  compositor spawn, cgroups/containment, health, pod ops on the reference box).
- Cross-verify each other's PRs before merge; raw evidence per PR (P0/P1 standard).
- Phase exits go upstream (`ygypt/dAIorama`) as their own PRs; PII/no-name scan first.
- Every exit restates its carried proofs — nothing silently dropped.

## Carried proofs (from 09's spike list; these have phase homes now)

- renderer override end-to-end + launcher capture -> P2 (runtime).
- cgroup delegation wiring in-container + startup sweep vs a killed manager -> P2.
- resize/scale path -> P2 (spawner creation params; L1-extension run on the reference box).
- llvmpipe CI lane (L3) -> P2 (Esther; GitHub Actions on the public repo once token
  `workflow` scope lands; env wiring deferred to operator).
- view credential + media handoff over a non-loopback bind -> P4.
- Hyprland version range + GPU recipes -> P2 doctor; rate numbers (B9) -> P4 with 07.

## P2 — core (CLI + manager)

Ships: one `daiorama` binary whose CLI + manager drive real headless sessions on the
reference box (create/list/close/status, control dispatch, config, containment).

- W1 contract layer (Esther) — scaffolding, IPC frame + codes, config module, fake-manager
  fixtures + conformance suite. In flight: `p2/fixtures-esther`.
- W2 session runtime (Blingus) — spawner (L1 chain as a daemon-managed child), registry +
  state machine, health checks, teardown + parent-death, startup sweep, cgroup scope +
  limits + off-mode, renderer check + launcher capture, resize/scale at creation.
- W3 CLI wiring (Esther) — cold/hot live, lazy start, drain/daemon semantics, config
  reload semantics, doctor's CLI side.
- W4 containment proof (Blingus; Esther verifies) — raw cgroup evidence: scope, kill path,
  sweep, off-mode cost; compose wiring.
- W5 CI lane (Esther) — llvmpipe protocol lane; local fallback until env lands.

Exit: lifecycle verbs headless on the real chain; fixtures green both boxes; containment
verified with raw output; config per 08; 04's pinned strings asserted byte-for-byte where
exercised; rebase-clean on `spec/v1`.

## P3 — bridge (MCP)

Ships: `daiorama mcp` (stdio + `--listen`); scope auth; deny-by-default exposure with the
always-on lifecycle set; first landing category = input (nine tools), plus screenshot,
get_view/stop_view, list_history, list_capabilities; media endpoint (single-use TTL'd
clip_video links); handshake for 2026-07-28 + legacy `initialize`; `-32022` mapping.

- W1 bridge core + conformance (Esther) — both modes, both handshake paths, error envelope 1:1.
- W2 media handoff + runtime calls (Blingus) — screenshot/clip capture from real sessions.
- W3 end-to-end (both) — a real harness drives a real session on the reference box:
  create -> input -> screenshot -> history.

Exit: MCP conformance green; harness-level E2E on the real chain.

## P4 — views + network

Ships: views per 06 (wayvnc + manager-owned front, per-view credential per 07, port
allocation from `view.port_base`, generations, idempotent get_view, view-down != session-dead);
network per 07 (manager `8756` / bridge `8757`, token + scope auth on non-loopback,
`view.advertise_base` exposure); drain/upgrade ops (04/10); rate-limit numbers verified.

Exit: view credential + media handoff over a non-loopback bind (carried proof); the
compose remote-view shape verified; auth failures = the pinned codes.

## P5 — dashboard + packaging

Ships: dashboard module (separate artifact; install/version-match per 09; degrade-by-
capability per 10); release manifest (minisign) + install script; compose snippets for all
legitimate shapes; container image; CI/release lanes; compat table with release notes.

Exit: v0.1 artifacts — signed manifest; installable bare-host / container / compose;
dashboard installs and degrades properly.

## Sequence (now)

1. Contract layer PR (in flight) -> Blingus cross-verify.
2. This plan -> Blingus cross-verify -> merge.
3. Runtime W2 starts against fixtures; L3/CI when env lands.
4. Each phase exit -> upstream PR; `STATUS.md` updated at every merge.
