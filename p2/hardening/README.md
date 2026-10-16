# P2 — hardening round 1(a): lifecycle churn on the real chain

Raw evidence that the shipped manager + CLI hold up under real lifecycle churn in the
privileged L1 container: three real sessions created, listed, inspected (`renderer: verified`
on each — raw `Renderer: NV117` in every instance log), one closed clean; then the manager
SIGKILLed mid-session and started again — the crash-recovery path.

## Finding (the round's first bug)

**After a manager SIGKILL in a keeper-prepared container, `daemon start` refuses with
`limits_unavailable` — the startup sweep never runs, and crash leftovers persist.**

- The keeper prep is one-shot per container: the first start moves every process under
  `<cgroup-root>/keeper` and enables the controllers at the namespace root. The second
  start's probe then reads `0::/keeper`, cannot enable at its own level (processes present),
  is not a namespace root — and refuses (`controllers not delegated to this scope; …`),
  although the container's session subtree is still there and still usable.
- Observed leftovers after the refused restart: one `Hyprland` process per live session
  **survived** the SIGKILL (fork-cleared PDEATHSIG — the grandchild class the design notes
  as cleanup's job) and stayed inside its session scope; both scope dirs and both runtime
  dirs remained. The sweep (`cgroup.kill` per stale scope) is exactly the mechanism that
  should reap these — it cannot run because the start refuses before sweep/bind.
- The first start's own sweep line (`startup sweep: 0 stale session scope(s) reaped`, on the
  empty subtree) shows the sweep is wired; the failure is the restart's containment
  decision.

Fix direction (next commit on this branch): make the probe recognize the keeper-prepared
container shape — a process under `<root>/keeper` whose namespace root already carries the
needed controllers is the containerized class re-entered; the managed cgroup dir is the
namespace root (same session subtree as the first start), so the startup sweep can run and
reap. Unit tests cover the re-entry; the existing classes stay untouched.

## Files

- `run-a.sh` — the churn harness (real CLI + daemon inside the L1 container; `BIN` points at
  the built binary; output captured to the raw log).
- `evidence/lifecycle-churn.txt` — raw container output of the run: create ×3, list/status
  with renderer marks, close, SIGKILL, refused restart, post-state.
- `evidence/lifecycle-churn-spec.json` — create-spec for the L1 run (mount sources
  genericized — substitute your checkout/artifact paths).

## Gate + run recipe

Gate + binary in a `rust:slim` container from this tip (`rustup component add rustfmt
clippy`; `cargo fmt --check`; `cargo clippy --all-targets --locked -- -D warnings`; `cargo
test --locked`; `cargo build --locked`; copy `target/debug/daiorama` out), then the L1
create-spec above (`dbus-run-session -- bash /src/p2/hardening/run-a.sh`).

Gate (fresh container, this tip): fmt clean, clippy `-D warnings` clean, suite green —
128 + 4 + 2 + 27 passed, 0 failed, 5 ignored.

## Result (round 1(a))

Everything before the crash is green: three real sessions on the real chain, marks
`verified` with the raw renderer string, close cleans its scope and processes. The crash
step: SIGKILL is handled as designed for direct children (mutter + one `Hyprland` per
session die), but the restart refuses (above), so the sweep never reaps. Fix next commit;
the round repeats after it.
