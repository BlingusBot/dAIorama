# P2 — hardening on the real chain: 1(a) lifecycle churn, 1(b) live-session drain

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

## Residual (round 2 candidates)

- Crashed sessions' runtime dirs (`/tmp/dr-<id8>`) are not reaped by the sweep — only the
  scopes and their processes are. A clean close removes them; a crash leaves them behind
  (they hold the compositor logs). Not part of the sweep's pinned contract (03); decide
  whether crash-time rt-dirs should be reaped (spawner-owned naming) or kept for diagnosis.
- Everything else in this round is green; round 1(b) (above) is green with no findings — the
  round continues to (c) odd creation params (scale 1.5, tiny and huge resolutions).

## Files

- `run-a.sh` — the churn harness (real CLI + daemon inside the L1 container; `BIN` points at
  the built binary; output captured to the raw log). Extended after the fix: `[5b]`
  post-restart create/close, `[6b]` doctor + shell cgroup.
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

## Gate + run recipe

Gate + binary in a `rust:slim` container from this tip (`rustup component add rustfmt
clippy`; `cargo fmt --check`; `cargo clippy --all-targets --locked -- -D warnings`; `cargo
test --locked`; `cargo build --locked`; copy `target/debug/daiorama` out), then the L1
create-spec above (`dbus-run-session -- bash /src/p2/hardening/run-a.sh`).

Gate (fresh container, fixed tip): fmt clean, clippy `-D warnings` clean, suite green —
130 + 4 + 2 + 27 passed, 0 failed, 5 ignored (+2 re-entry tests).

Re-gate with the 1(b) commit tree (docs + harness only, no `src/` delta; fresh `rust:slim`):
fmt clean (no rewrite), clippy `-D warnings` clean, same suite — 130 + 4 + 2 + 27 passed,
0 failed, 5 ignored.
