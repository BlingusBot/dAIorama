# P2 — hardening round 1(a): lifecycle churn on the real chain

Raw evidence that the shipped manager + CLI hold up under real lifecycle churn in the
privileged L1 container: three real sessions created, listed, inspected (`renderer: verified`
on each — raw `Renderer: NV117` in every instance log), one closed clean; then the manager
SIGKILLed mid-session and started again — the crash-recovery path.

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
- Everything else in this round is green; the round continues to (b) drain integration with
  live sessions.

## Files

- `run-a.sh` — the churn harness (real CLI + daemon inside the L1 container; `BIN` points at
  the built binary; output captured to the raw log). Extended after the fix: `[5b]`
  post-restart create/close, `[6b]` doctor + shell cgroup.
- `evidence/lifecycle-churn.txt` — round 1(a) raw container output (the finding).
- `evidence/lifecycle-churn-rerun.txt` — round 1(a) re-run on the fixed tip (the proof).
- `evidence/lifecycle-churn-spec.json` — create-spec for the L1 run (mount sources
  genericized — substitute your checkout/artifact paths). The re-run used the same recipe
  (container name `dai-hardening-b`, output `hardening-b.txt`).

## Gate + run recipe

Gate + binary in a `rust:slim` container from this tip (`rustup component add rustfmt
clippy`; `cargo fmt --check`; `cargo clippy --all-targets --locked -- -D warnings`; `cargo
test --locked`; `cargo build --locked`; copy `target/debug/daiorama` out), then the L1
create-spec above (`dbus-run-session -- bash /src/p2/hardening/run-a.sh`).

Gate (fresh container, fixed tip): fmt clean, clippy `-D warnings` clean, suite green —
130 + 4 + 2 + 27 passed, 0 failed, 5 ignored (+2 re-entry tests).
