# P2 — the real-chain run (lifecycle verbs, real compositors)

The P2 exit run: the lifecycle verbs against the **real** chain — mutter `--headless` parent →
nested Hyprland on the render node — driven over the manager socket exactly as an operator
client would.

## What runs

`src/runtime/service.rs::tests::real_chain_lifecycle_verbs` — ignored by default; it needs the
L1 image's compositors and a render node. The CI lane builds the test binary in a `rust:slim`
container (fmt · clippy `-D warnings` · tests), then the binary runs inside the L1 image:

```
dbus-run-session -- ./daiorama-tests real_chain --ignored --nocapture --test-threads=1
```

Run shape: cap-drop=all, the render node passed through as a device, no other privileges.

## What it proves

- **create** — two sessions over the wire: pinned defaults, then `1024x768 @ 1.5`
  (`mode = logical × scale`); both answer `state: running`.
- **list** — both sessions present.
- **session_status** — the pinned session object.
- **close** — kill-then-remove for both; the closed id then answers `session_closed`
  (tombstone), and the final list is empty.

Raw output: `evidence/chain-run1.txt` — green, ~1.4 s.

## What it surfaced

The first runs failed at the child readiness gate: Hyprland came up fully (monitors rendering,
Xwayland alive, instance log healthy) but never presented its IPC socket. Root cause: **unix
socket path length** (`sun_path`, 108-byte cap) — Hyprland's instance-directory name runs ~61
chars, so with the session's own directory in the same path the IPC `bind()` exceeded the cap
and failed while the compositor kept running.

Fix (same PR): the compositors' `XDG_RUNTIME_DIR` is a short dedicated directory
(`dr-<id8>` under the temp root) with an explicit budget guard in the spawner; the session
directory (logs, lock, config) stays as before. The failure path also now carries a full
diagnostic dump (logs, directory walk, instance log, child kernel state, process table).
