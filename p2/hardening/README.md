# P2 — hardening on the real chain: 1(a) lifecycle churn, 1(b) live-session drain, 1(c) odd creation params

Raw evidence that the shipped manager + CLI hold up under real lifecycle churn in the
privileged L1 container: three real sessions created, listed, inspected (`renderer: verified`
on each — raw `Renderer: NV117` in every instance log), one closed clean; then the manager
SIGKILLed mid-session and started again — the crash-recovery path.

## Round 1(b) — drain integration with LIVE sessions (green, no findings)

The shipped drain path exercised the way an operator upgrades, with three real sessions
running on the L1 chain:

- `drain` enters and reports the live state — `draining; 3 still running`, `--json`
  `{"active":true,"remaining":3}`, instance status `drain: active, 3 still running`; entering
  again is idempotent.
- `new` is refused with the pinned 04 string byte-for-byte (`manager is draining for upgrade;
  new sessions are refused (3 still running). (draining)` + hint), before anything spawns: the
  process/scope/runtime-dir snapshot is byte-identical before and after the refusal and the
  list is unchanged.
- closing live sessions drops `remaining` 3 → 2 → 1 over the wire; each close tears its scope,
  compositors and runtime dir down cleanly (compositors 9 → 6 → 3, scopes 3 → 2 → 1).
- `drain --wait --timeout 2` with one session left exits 1 after the bound, naming what
  remains (`drain did not reach zero running sessions within 2s (1 still running)`).
- `drain --wait --timeout 60` launched with one session left is still polling 2 s in; the last
  close lands while it polls and the waiter completes `drain complete` rc=0. At zero the state
  stays active — `new` still refused, `(0 still running)` — until the manager stops.
- `daemon end --force` leaves nothing behind: no compositors, no scope dirs, no session
  runtime dirs, no manager process.

Raw: `evidence/drain-live.txt` (container `dai-hardening-c2`, output `hardening-c2.txt`);
harness `run-b.sh`; create-spec `evidence/drain-live-spec.json`. No findings — no src change.

## Round 1(c) — odd creation params: one finding, fixed; re-run green

The matrix (harness `run-c.sh`, fresh L1 container `dai-hardening-d`: eight real sessions
through the manager CLI — 1280x800 @1.5, 64x64, 1x1, 641x481 @1.5, 642x482 @1.25,
1920x1080 @0.25, 7680x4320, 16384x16384) found one defect and pinned its mechanism:

**Finding — silent scale substitution.** For a `(logical, scale)` whose rounded mode is
not an exact multiple of the scale (e.g. 641x481 × 1.5 → mode 962x722), the compositor
replaced the requested scale with one of its own: the instance log reads
`ERR ]: Invalid scale passed to monitor, 1.5 found suggestion 2` — Hyprland 0.56.2
(`src/output/Monitor.cpp`) snaps any scale whose logical size is non-integral to the
nearest scale that makes it integral (1.5→2 for 962x722, 1.5→1.6 for 2560x1440, 1.4→1;
probed live in `scale-substitution-diag.txt`). The session was created, marked
`verified`, and recorded scale 1.5 — while its logical screen was 481x361, not 641x481.
First run raw: `odd-params-firstrun.txt` (`FAILS=1` — the compositor readback asserted
the requested scale and got 2).

**Fix (this branch).** The session conf opts out of the heuristic
(`debug:disable_scale_checks = true` — `(logical, scale) → mode` is the spawner's
arithmetic, not the compositor's suggestion) and the spawn verifies the outcome anyway:
after the monitor rule it reads the primary output back (`hyprctl monitors -j`); a
mismatch that persists refuses the spawn (message names asked vs effective) and tears
down like every other failure, one re-application guards a dropped keyword, and a
compositor that cannot be queried degrades to a warning (renderer-gate precedent).
Opt-out and readback timing proven live in `scale-optout-diag.txt` /
`scale-readback-diag.txt` (requested scales 1.5, 1.25, 1.33, 1.4 land verbatim; a fresh
output is read back within the first 250 ms poll).

**Result — re-run green (`odd-params.txt`, container `dai-hardening-d2`).** All eight
cases: created, renderer `verified`, the compositor carries the computed physical mode
AND the requested scale verbatim (`962x722 @1.5`, `803x603 @1.25`, `480x270 @0.25`, …),
clean close with no process/scope/runtime-dir left, per-case snapshot byte-identical;
`FAILS=0`, `SCRIPT_RC=0`. Products of the run: a 1x1 session and a 16384x16384 session
both create, read back, and close clean.

Notes:
- Probes are diagnostic, not the spawn path: the three `*-diag.txt` runs share one
  recipe (same mounts/device as the matrix; `BIN` + `dbus-run-session`). Their runners'
  `SCRIPT_RC=0`/`RUN-ALL-DONE` footers landed in the container logs, not the captured
  files — every file is the raw script stdout.
- Probe observation to revisit with mid-session work: re-applying monitor rules
  repeatedly to one long-lived output stopped being reflected in later `hyprctl monitors`
  reads in the probe containers (single-apply sessions — the spawn path — are unaffected
  across the eight-case matrix). Mid-session resizes are out of scope (01).

## Finding (round 1(a), fixed this branch)

**After a manager SIGKILL in a keeper-prepared container, `daemon start` refused with
`limits_unavailable` — the startup sweep never ran, and crash leftovers persisted.**

- The keeper prep is one-shot per container: the first start moves every process under
  `<cgroup-root>/keeper` and enables the controllers at the namespace root. The second
  start's probe then read `0::/keeper`, could not enable at its own level (processes
  present), was not a namespace root — and refused (`controllers not delegated to this
  scope; …`), although the container's session subtree was still there and still usable.
- Observed leftovers after the refused restart: one `Hyprland` process per live session
  **survived** the SIGKILL (fork-cleared PDEATHSIG — the grandchild class the design notes
  as cleanup's job) and stayed inside its session scope; both scope dirs and both runtime
  dirs remained. The sweep (`cgroup.kill` per stale scope) is exactly the mechanism that
  should reap these — it could not run because the start refused before sweep/bind.
- The first start's own sweep line (`startup sweep: 0 stale session scope(s) reaped`, on the
  empty subtree) shows the sweep is wired; the failure was the restart's containment
  decision. Raw log: `evidence/lifecycle-churn.txt`.

## Fix (this commit)

The probe now recognizes the keeper-prepared container shape, re-entered
(`containment::keeper_reentry`): a process whose own cgroup is exactly the namespace root's
keeper child (`0::/keeper`) with the root already carrying the needed controllers is the
containerized class on a second start. The managed dir is the namespace root again — the
same instance session subtree the first start built — so the startup sweep runs and reaps.
A keeper-shaped path whose root does **not** carry the controllers is not a container this
prep ever prepared: class 1's refusal stands (moving the root's processes is not ours to do
there). Unit tests: `keeper_prepared_container_reentry_manages_the_namespace_root`,
`keeper_shaped_path_without_the_roots_controllers_stays_the_child`; the existing class
tests are untouched.

## Result (round 1(a) re-run, fixed tip — `evidence/lifecycle-churn-rerun.txt`)

The same churn on a fresh L1 container with the fixed binary. Everything before the crash is
unchanged (3 sessions, `renderer: verified`, raw `Renderer: NV117`, clean close); the crash
step now recovers:

- SIGKILL: two `Hyprland` survivors (one per live session), each alone in its scope, socket
  file left — the same shape as round 1.
- Restart: `restart_rc=0`; the new manager's cgroup reads `0::/keeper` (the re-entry shape);
  the daemon log carries `startup sweep: 2 stale session scope(s) reaped`; after the sweep
  **no compositor process and no scope dir remain** (`/proc` scan + scope-subtree listing in
  the raw log).
- The recovered manager is fully functional: a new session created after the restart lands
  `renderer: verified` and closes clean (scope gone).
- `daiorama doctor` in the same container reports the re-entry through the real probe:
  `ok — keeper-prepared container re-entered: namespace root already carries cpu memory pids`.

## Round 2 — full (a)–(c) re-run on the fixed+merged tip (green, no findings — round closed)

The round rule after round 1 (findings in (a) and (c), both fixed): repeat the full pass on
the fixed tip, now carried onto the merged `spec/v1` (@52f05f3, the frame-cap slice included)
via merge `16f9b61`. Fresh L1 containers per segment, binary built from this tip
(CI `dai-r2-ci`, sha256 `b64b74f5…`).

- (a) lifecycle churn — `round2-lifecycle-churn.txt`: 42/42 assertions `[OK]`, `FAILS=0`.
  Three sessions created (`renderer: verified` on each, raw `Renderer: NV117` in all three
  instance logs), one closed; SIGKILL mid-session leaves two orphaned `Hyprland` survivors,
  one per remaining scope; restart lands `startup sweep: 2 stale session scope(s) reaped` —
  after it, no compositor process and no scope dir remain (`/proc` scan + scope-subtree
  listing in the raw log); the recovered manager creates and closes a new session; doctor
  reports the keeper re-entry. The harness was upgraded this round to self-checking
  (per-assertion `[OK]`/`[FAIL]` + `FAILS=`, exit code = `FAILS`) — (b) and (c) already were.
- (b) drain integration — `round2-drain-live.txt`: 28/28 `[OK]`, `FAILS=0` — drain enters
  (`{active, remaining:3}`), `new` refused with the pinned 04 string and zero spawn, closes
  drop remaining 3→2→1→0, the `--wait` bound fires at 1, `--wait` completes at zero, drain
  sticky at zero, end-state leak sweep clean.
- (c) odd creation params — `round2-odd-params.txt`: 97/97 `[OK]`, `FAILS=0` — all eight
  cases (incl. 641x481 @1.5 → `962x722 scale=1.5` and 642x482 @1.25 → `803x603 scale=1.25`
  reading the requested scale back verbatim; 1x1; 16384x16384) create, carry the requested
  geometry, and close with zero residue.

No findings in the full pass → the round closes: cross-verify handoff is PR #22 at this
round's tip (freeze: the verdict binds to the sha reviewed).

## Round 3 — full (a)–(c) re-run on the merged IPC-refusal tip (green, no findings)

Regression pass on the tip after both IPC refusal slices merged (`c1da5ed` — gate refusals:
invalid-UTF-8 lines and repeated handshakes; dispatch refusals: unknown vs not-yet-built
methods) — the first real-chain exercise of that code. Fresh L1 containers per segment;
binary built from this tip in CI (`dai-r3-ci`, sha256 `10da95d4…`).

- (a) lifecycle churn — `round3-lifecycle-churn.txt`: 42/42 assertions `[OK]`, `FAILS=0`.
  Three sessions created (`renderer: verified` on each, raw `Renderer: NV117` in all three
  instance logs), one closed clean; SIGKILL mid-session leaves two orphaned `Hyprland`
  survivors, one per remaining scope; restart lands `startup sweep: 2 stale session
  scope(s) reaped` — after it, no compositor process and no scope dir remain; the
  recovered manager creates and closes a new session; doctor reports the keeper re-entry.
- (b) drain integration — `round3-drain-live.txt`: 28/28 `[OK]`, `FAILS=0` — drain enters
  (`{active, remaining:3}`), `new` refused with the pinned 04 string and zero spawn, closes
  drop remaining 3→2→1→0, the `--wait` bound fires at 1, `--wait` completes at zero, drain
  sticky at zero, end-state leak sweep clean.
- (c) odd creation params — `round3-odd-params.txt`: 97/97 `[OK]`, `FAILS=0` — all eight
  cases (incl. 641x481 @1.5 → `962x722 scale=1.5` and 642x482 @1.25 → `803x603 scale=1.25`
  reading the requested scale back verbatim; 1x1; 16384x16384) create, carry the requested
  geometry, and close with zero residue.

No findings in the full pass → the round adds nothing to fix; cross-verify handoff is this
branch's PR at this tip (freeze: the verdict binds to the sha reviewed).

## Round 4 — full (a)–(c) re-run on the post-parity tip (green, no findings)

Regression pass on the tip after the config write-lock slice (`9aaffd1` — serialized
read-modify-write, atomic replace) and the no-parameter dispatch slice (`4fcd6ee` — `status`,
`readiness`, `config_reload`, `drain` refuse stray keys) merged — the first real-chain exercise
of both. Fresh L1 containers per segment; binary built from this tip in CI (`dai-r4-ci`,
sha256 `a26d9146…`).

- (a) lifecycle churn — `round4-lifecycle-churn.txt`: 42/42 assertions `[OK]`, `FAILS=0`.
  Three sessions created (`renderer: verified` on each, raw `Renderer: NV117` in all three
  instance logs), one closed clean; SIGKILL mid-session leaves two orphaned `Hyprland`
  survivors, one per remaining scope; restart lands `startup sweep: 2 stale session
  scope(s) reaped` — after it, no compositor process and no scope dir remain; the
  recovered manager creates and closes a new session; doctor reports the keeper re-entry.
- (b) drain integration — `round4-drain-live.txt`: 28/28 `[OK]`, `FAILS=0` — drain enters
  (`{active, remaining:3}`), `new` refused with the pinned 04 string and zero spawn, closes
  drop remaining 3→2→1→0, the `--wait` bound fires at 1, `--wait` completes at zero, drain
  sticky at zero, end-state leak sweep clean. The no-parameter gate holds on the live path:
  the CLI's drain/status calls carry no params and still answer.
- (c) odd creation params — `round4-odd-params.txt`: 97/97 `[OK]`, `FAILS=0` — all eight
  cases (incl. 641x481 @1.5 → `962x722 scale=1.5` and 642x482 @1.25 → `803x603 scale=1.25`
  reading the requested scale back verbatim; 1x1; 16384x16384) create, carry the requested
  geometry, and close with zero residue.

No findings in the full pass → the round adds nothing to fix; cross-verify handoff is this
branch's PR at this tip (freeze: the verdict binds to the sha reviewed).

## Round 5 — full (a)–(c) re-run on the final parity-follow-up tip (green, no findings)

Confirming pass on the tip after the parity follow-ups merged (`4686218` — restart-only
`config set` line, reload names applied keys, the 08 lock row) — the first real-chain
exercise after that slice touched the config write path. Round 4 ran on `bc95f9c`; two
merges landed after it, so this round re-establishes green on the actual final tip. Fresh
L1 containers per segment; binary built from this tip in CI (`dai-r5-ci`, sha256
`67f85ef0…`), gates at that tip: fmt clean, clippy `-D warnings` clean, `144 (+5 ignored) ·
4 · 2 · 27` passed, build ok (`round5-gates.log`).

- (a) lifecycle churn — `round5-lifecycle-churn.txt`: 42/42 assertions `[OK]`, `FAILS=0`.
  Three sessions created (`renderer: verified` on each, raw `Renderer: NV117` in all three
  instance logs), one closed clean; SIGKILL mid-session leaves two orphaned `Hyprland`
  survivors, one per remaining scope; restart lands `startup sweep: 2 stale session
  scope(s) reaped` — after it, no compositor process and no scope dir remain; the
  recovered manager creates and closes a new session; doctor reports the keeper re-entry.
- (b) drain integration — `round5-drain-live.txt`: 28/28 `[OK]`, `FAILS=0` — drain enters
  (`{active, remaining:3}`), `new` refused with the pinned 04 string and zero spawn, closes
  drop remaining 3→2→1→0, the `--wait` bound fires at 1, `--wait` completes at zero, drain
  sticky at zero, end-state leak sweep clean (compositors=0 scopes=0 daiorama=0 dr_dirs=0).
- (c) odd creation params — `round5-odd-params.txt`: 97/97 `[OK]`, `FAILS=0` — all eight
  cases (incl. 641x481 @1.5 → `962x722 scale=1.5` and 642x482 @1.25 → `803x603 scale=1.25`
  reading the requested scale back verbatim; 1x1; 16384x16384) create, carry the requested
  geometry, and close with zero residue.

Regression identity check: each round-5 raw diffed against its round-4 counterpart with
timestamps, uuids and pids normalized is empty apart from those normalizations — the pass
is structurally identical on the moved tip.

**P2 hardening is complete.** Three consecutive full (a)–(c) rounds on merged tips are
green (rounds 3, 4, 5), the last on the final tip `4686218`; no round since round 1 has
added a finding. This branch is the completion handoff for cross-verify (freeze: verdict
binds to the sha reviewed). Phase-level follow-ups left are the two residual items below —
both scope decisions, not defects.

## Soak/leak pass — post-P2 on the P3-era tip `0020763` (green, no findings)

The rounds above answer "is each transition clean?"; this pass asks the repeat question a
one-shot round cannot: does walking the same path many times return the manager's
per-session and per-connection resources to baseline? Tip `0020763` (merged listen/SDK
slice); binary built from that tip in CI (`dai-soak-ci`, sha256 `9d16fbde…`); L1 container
`dai-soak-b`; harness `run-e.sh`. Gates at that tip: fmt clean, clippy `-D warnings` clean,
`165 (+5 ignored) · 4 · 2 · 27 · 7 · 22` passed, 0 failed (`evidence/soak-gates.log`).

- **Sequential soak — 24 real create → status (`renderer: verified`) → close cycles** over
  an eight-entry resolution/scale matrix (1280x800, 1920x1080, 640x480, 800x600; scales
  1 / 1.5 / 2). Every cycle tears down to zero scope dirs, zero compositor processes and
  zero `/tmp/dr-*` runtime dirs, and `list` reports no sessions. Manager footprint is flat
  across all 24: **fds 11, threads 14** at baseline and at every cycle boundary (`SNAP`
  lines in the raw log). RSS 5,700 → 6,096 kB over the whole run (+396 kB, <1%).
- **RPC soak — 400 sequential `list` calls** on the same manager: 400/400 answered, fds 11
  before and after, threads unchanged — the per-connection path holds nothing open.
- **Burst soak — 5 rounds of 3 concurrent sessions**, closed out of order: 3 scopes and 9
  compositor processes while live, zero of each after every round; fds 11 after every burst.
- Hardware path confirmed in the run: the first session's compositor log carries
  `Renderer: NV117`, and every session marked `renderer: verified`.
- End state: `daemon end --force` leaves no daiorama process, no compositor, no scope dir,
  no session runtime dir.

**328/328 assertions `[OK]`, `FAILS=0`** (`evidence/soak.txt`). No leak in the per-session or
per-connection path; the manager's resident footprint is O(1) in the number of sessions
(Hyprland is a child process, not a manager-held socket — fds/threads read 11/14 with 0, 1
or 3 sessions live). No src change this round.

Not covered (unchanged residual): crash-time runtime dirs (`/tmp/dr-<id8>`) are still not
reaped by the startup sweep — see the residual item below.

## Residual (tracked, post-round)

- Crashed sessions' runtime dirs (`/tmp/dr-<id8>`) are not reaped by the sweep — only the
  scopes and their processes are. A clean close removes them; a crash leaves them behind
  (they hold the compositor logs). Not part of the sweep's pinned contract (03); decide
  whether crash-time rt-dirs should be reaped (spawner-owned naming) or kept for diagnosis.
- The repeated-reapply observation (probe containers, round 1(c) notes): revisit when
  mid-session output work lands — nothing shipped re-applies a rule to a live output yet.
- Round 2 (the full (a)–(c) pass on the fixed+merged tip) added no findings — the round is
  closed and out for cross-verify; the two items above are scope decisions for later.
- Round 4 (the full (a)–(c) pass on the post-parity tip `bc95f9c`) added no findings either —
  with round 3's clean pass, the last two full rounds on merged tips are green; the two
  items above remain scope decisions for later.
- Round 5 (the full (a)–(c) pass on the final parity-follow-up tip `4686218`) added no
  findings — that is three consecutive clean full rounds, the last on the phase's final
  tip; hardening is complete and this branch is the handoff. The two items above are the
  only follow-ups, both scope decisions for later.

## Files

- `run-a.sh` — the churn harness (real CLI + daemon inside the L1 container; `BIN` points at
  the built binary; output captured to the raw log). Extended after the fix: `[5b]`
  post-restart create/close, `[6b]` doctor + shell cgroup. Self-checking since round 2:
  per-assertion `[OK]`/`[FAIL]` + a final `FAILS=`, exit code = `FAILS`.
- `run-b.sh` — the 1(b) drain harness (same pattern): three live sessions, the refusal drill,
  remaining-drop closes, the `--wait` bound + completion (close lands while it polls), the
  sticky-at-zero check, then the end-state leak sweep. Prints `[OK]`/`[FAIL]` per assertion
  and a final `FAILS=` count.
- `evidence/lifecycle-churn.txt` — round 1(a) raw container output (the finding).
- `evidence/lifecycle-churn-rerun.txt` — round 1(a) re-run on the fixed tip (the proof).
- `evidence/lifecycle-churn-spec.json` — create-spec for the L1 run (mount sources
  genericized — substitute your checkout/artifact paths). The re-run used the same recipe
  (container name `dai-hardening-b`, output `hardening-b.txt`).
- `evidence/drain-live.txt` — round 1(b) raw container output (green; `FAILS=0`).
- `evidence/drain-live-spec.json` — create-spec for the 1(b) run (mount sources genericized;
  container `dai-hardening-c2`, output `hardening-c2.txt`).
- `run-c.sh` — the 1(c) odd-params harness (eight-case matrix; per-assertion `[OK]`/`[FAIL]`
  and a final `FAILS=` count).
- `diag-scale.sh`, `diag-scale2.sh`, `diag-scale3.sh` — the three live probes: substitution
  matrix with the heuristic on; the `disable_scale_checks` opt-out proof; readback timing.
- `evidence/odd-params-firstrun.txt` — round 1(c) first run (the finding; `FAILS=1`).
- `evidence/odd-params.txt` — round 1(c) re-run on the fixed tip (green; `FAILS=0`).
- `evidence/scale-substitution-diag.txt`, `evidence/scale-optout-diag.txt`,
  `evidence/scale-readback-diag.txt` — the probe raws (containers
  `dai-hardening-d-diag`, `-diag2`, `-diag3`).
- `evidence/odd-params-spec.json` — create-spec for the 1(c) re-run (mount sources
  genericized; container `dai-hardening-d2`, output `hardening-d2.txt`).
- `evidence/round2-lifecycle-churn.txt`, `evidence/round2-drain-live.txt`,
  `evidence/round2-odd-params.txt` — round 2 raws on the fixed+merged tip (all green,
  `FAILS=0`; containers `dai-r2-a`, `dai-r2-b`, `dai-r2-c`).
- `evidence/round2-a-spec.json`, `evidence/round2-b-spec.json`, `evidence/round2-c-spec.json`
  — create-specs for the round-2 runs (mount sources genericized).
- `evidence/round3-lifecycle-churn.txt`, `evidence/round3-drain-live.txt`,
  `evidence/round3-odd-params.txt` — round 3 raws on the merged IPC-refusal tip (all green,
  `FAILS=0`; containers `dai-r3-a`, `dai-r3-b`, `dai-r3-c`).
- `evidence/round3-a-spec.json`, `evidence/round3-b-spec.json`, `evidence/round3-c-spec.json`
  — create-specs for the round-3 runs (mount sources genericized).
- `evidence/round4-lifecycle-churn.txt`, `evidence/round4-drain-live.txt`,
  `evidence/round4-odd-params.txt` — round 4 raws on the post-parity tip (all green,
  `FAILS=0`; containers `dai-r4-a`, `dai-r4-b`, `dai-r4-c`).
- `evidence/round4-a-spec.json`, `evidence/round4-b-spec.json`, `evidence/round4-c-spec.json`
  — create-specs for the round-4 runs (mount sources genericized).
- `evidence/round5-lifecycle-churn.txt`, `evidence/round5-drain-live.txt`,
  `evidence/round5-odd-params.txt` — round 5 raws on the final parity-follow-up tip (all
  green, `FAILS=0`; containers `dai-r5-a`, `dai-r5-b`, `dai-r5-c`).
- `evidence/round5-a-spec.json`, `evidence/round5-b-spec.json`, `evidence/round5-c-spec.json`
  — create-specs for the round-5 runs (mount sources genericized).
- `evidence/round5-gates.log` — round-5 gate + build output at `4686218` (container
  `dai-r5-ci`; the image carries no `git`, so the tip is the mounted worktree).
- `run-e.sh` — the soak/leak harness (24 sequential create/status/close cycles, a 400-call
  `list` soak, five 3-session bursts closed out of order; manager fd/thread/RSS sampled at
  every boundary, scope/compositor/runtime-dir residue checked after every close).
  Self-checking like the others: per-assertion `[OK]`/`[FAIL]`, final `FAILS=`, exit code =
  `FAILS`.
- `evidence/soak.txt` — soak/leak raw on the P3-era tip `0020763` (green; 328 `[OK]`,
  `FAILS=0`; container `dai-soak-b`, output `soak-b.txt`).
- `evidence/soak-spec.json` — create-spec for the soak run (mount sources genericized).

## Gate + run recipe

Gate + binary in a `rust:slim` container from this tip (`rustup component add rustfmt
clippy`; `cargo fmt --check`; `cargo clippy --all-targets --locked -- -D warnings`; `cargo
test --locked`; `cargo build --locked`; copy `target/debug/daiorama` out), then the L1
create-spec above (`dbus-run-session -- bash /src/p2/hardening/run-a.sh`; the later rounds
substitute `run-b.sh` / `run-c.sh` / `run-e.sh`).

Gate (fresh container, fixed tip): fmt clean, clippy `-D warnings` clean, suite green —
130 + 4 + 2 + 27 passed, 0 failed, 5 ignored (+2 re-entry tests).

Re-gate with the 1(b) commit tree (docs + harness only, no `src/` delta; fresh `rust:slim`):
fmt clean (no rewrite), clippy `-D warnings` clean, same suite — 130 + 4 + 2 + 27 passed,
0 failed, 5 ignored.

Re-gate with the 1(c) tip (geometry readback — spawner + tests; fresh `rust:slim`): fmt
clean, clippy `-D warnings` clean — 133 + 4 + 2 + 27 passed, 0 failed, 5 ignored.

Round 2 with the merged tip (merge `16f9b61`; fresh `rust:slim`, container `dai-r2-ci`):
fmt clean, clippy `-D warnings` clean — 135 + 4 + 2 + 27 passed, 0 failed, 5 ignored. The
L1 binary for the whole round is this tree's `target/debug/daiorama` (sha256 `b64b74f5…`).

Round 3 at the merged IPC-refusal tip (`c1da5ed`; fresh `rust:slim`, container `dai-r3-ci`):
fmt clean, clippy `-D warnings` clean — 138 + 4 + 2 + 27 passed, 0 failed, 5 ignored. The
L1 binary for the whole round is this tree's `target/debug/daiorama` (sha256 `10da95d4…`).

Round 4 at the post-parity tip (`bc95f9c`; fresh `rust:slim`, container `dai-r4-ci`): fmt
clean, clippy `-D warnings` clean — 142 + 4 + 2 + 27 passed, 0 failed, 5 ignored. The L1
binary for the whole round is this tree's `target/debug/daiorama` (sha256 `a26d9146…`).

Round 5 at the final parity-follow-up tip (`4686218`; fresh `rust:slim`, container
`dai-r5-ci`): fmt clean, clippy `-D warnings` clean — 144 + 4 + 2 + 27 passed, 0 failed, 5
ignored. The L1 binary for the whole round is this tree's `target/debug/daiorama` (sha256
`67f85ef0…`).

## Round 6 — concurrency: `drain` entering while a create is in flight (unit fix + live pass)

The first round that exercises **two clients at once** instead of one client's call sequence.

**Finding — drain/create TOCTOU in the service (fixed this branch).** `create_session` checked
drain *before* taking the create lock, and `drain()` set the flag without that lock at all, so
the two were never ordered. A create that had passed its check could still be inside
`spawner.spawn()` when drain was entered: drain would report `{active:true, remaining:n}`
without that session (its record is only inserted after the spawn returns), and `drain --wait`
— which polls exactly that number for zero — could report `drain complete` while a compositor
chain was still starting. The contract the manager states for itself is the opposite: "no
spawn may start" once draining (04/10, and the comment on the check itself).

Fix (small, in `src/runtime/service.rs`): `drain()` takes the create lock before setting the
flag (so an in-flight spawn finishes and is counted, or the flag lands first), and
`create_session` re-reads the drain state inside its critical section (so a create that queued
behind another create is refused instead of spawning after drain was entered). Two unit tests
pin both interleavings deterministically — a blocking stub spawner holds the critical section
open (`drain_waits_for_an_in_flight_create_and_counts_it`), and the queued-create case is
arranged by holding the lock from the test (`a_create_that_queued_past_drain_is_refused_before_it_spawns`).

**Observation — the live surface serializes (not fixed; a decision, not a defect).** While
probing the live manager, a second client's request never lands during a create: with a
16k-geometry create in flight, a fresh connection's handshake waited **1200 ms** and a `list`
on an already-handshaken connection waited **1386 ms**, both returning at the same millisecond
the create did (`evidence/r6-concurrency-probe.txt`; the manager has 12 tokio workers plus the
spawner forker thread, so this is not thread starvation). Consequence: on this box the
drain/create race above cannot be *observed* end to end — which is exactly why it needed a
unit-level fix rather than a live one: the invariant now holds by lock, not by that
serialization. The serialization itself is worth a decision before P3 relays concurrent tool
calls through the bridge; it is recorded here, not changed.

**Live pass (`evidence/r6-drain-create-race.txt`, binary built from this branch).** Three
iterations of a create in flight when `drain --json` is issued (drain at +350 ms, create
~680 ms): drain answered `{active:true, remaining:1}` and exactly one session was running —
`remaining` accounted for the session, and the note per iteration records that drain answered
after the create finished (serialized). `drain --wait --timeout 3` in the same setup never
printed `drain complete`; it exited 1 with `(1 still running)`, and exactly one session was
live after it gave up. `new` while draining is refused with the pinned 04 string and the
runtime-dir/scope snapshot is unchanged. `daemon end --force` leaves no compositors, no scope
dirs, no session runtime dirs. `FAILS=0`, `SCRIPT_RC=0`.

Round 6 gates (`evidence/r6-gates.log`, fresh `rust:slim`, container `dai-r6-gate3`): fmt
clean, clippy `-D warnings` clean — **146 + 4 + 2 + 27** passed, 0 failed, 5 ignored (144 + the
two new tests). Harness: `run-d.sh`; live probes: `probe-timeline.py`, `probe-hammer.py`;
create-spec: `evidence/r6-drain-create-spec.json`.
