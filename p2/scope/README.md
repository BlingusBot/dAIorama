# P2 — session containment: privileged real-run evidence

Raw evidence that the C6 session-scope mechanics hold on a real cgroupfs with real
compositors — the `#[ignore]d` tests from the branch tip executed in throwaway
containers on the reference box (rootless podman, cgroup v2, private cgroup namespace).

## Files

- `evidence/scope-real-probe.txt` — `real_box_probe` in a fresh privileged container:
  the production probe runs the keeper prep at the namespace root (nsroot drained,
  `+cpu +memory +pids` enabled — the class-2 recipe from `p2/cgroup-matrix/`).
- `evidence/scope-real-chain.txt` — `real_containment_chain` in a fresh privileged
  container, inside the L1 image (mutter + Hyprland + render node, dbus session):
  1. keeper prep through the production probe;
  2. one real session spawns with containment on — both compositor pids land in the
     session's scope, `memory.max` = 4294967296;
  3. `kill_tree` (`cgroup.kill`) → processes gone, scope dir gone;
  4. startup sweep → a stale scope's live process dies and the scope dir is removed;
  5. parent-death witness → a child spawned through the forker + `pre_exec` PEP dies
     when its parent is SIGKILLed (no teardown code runs).
- `evidence/scope-real-chain-spec.json` — create-spec for the chain run (mount sources
  genericized — substitute your checkout/artifact paths). The probe run is the same
  spec with `real_box_probe` as the test filter and a probe-named log.

## Build / run recipe

`daiorama-tests` is built from the branch tip in a `rust:slim` container
(`rustup component add rustfmt clippy`; `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`,
`cargo test --no-run --locked`; the unit-test executable is copied out as
`daiorama-tests`), then executed in the privileged L1 container as
`dbus-run-session -- ./daiorama-tests <test> --ignored --nocapture --test-threads=1`
via the podman REST create-spec above.

## Result

All legs green in ~1.1 s (`1 passed; 0 failed`); post-run state shows the nsroot
controller set enabled and no scope directories left under the instance subtree.
