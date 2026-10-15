# P2 — cgroup containment matrix (reference box)

Raw evidence for the containment classes that W2/W4 and the fixtures' containment
envelope build on. Method: throwaway containers on the reference box (rootless
podman 5.8.4, crun 1.28, cgroup v2, systemd user session), each running a scripted
probe; create-specs in `specs/`, raw in-container logs in `logs/` (verbatim).
Box specifics (paths, uids) as-is; the box user is the unprivileged user (uid 1001,
`user-1001` in cgroup paths).

## Matrix

| run mode | /sys/fs/cgroup mount | scope mkdir | enable controllers (nsroot) | limits (memory.max / pids.max) | move proc | log |
|---|---|---|---|---|---|---|
| **default container** (unprivileged, private cgroupns) | `ro` | ✗ `Read-only file system` | ✗ | ✗ | ✗ | `cgroup-probe.txt` |
| default + remount attempt inside | mount(2) `permission denied` (no SYS_ADMIN) | ✗ | ✗ | ✗ | ✗ | `cgroup-probe2.txt`* |
| **unprivileged + `cgroupns=host`** | `ro` (host path visible: `…libpod-<id>.scope/container`) | ✗ | ✗ | ✗ | ✗ | `cgroup-probe-hostns.txt` |
| **privileged, private cgroupns** | `rw` | ✓ | ✗ directly — `EBUSY` while nsroot has processes; ✓ after keeper prep (below) | ✓ after prep | ✓ | `cg-dai-cg-priv.txt`, `cg-priv2.txt`–`cg-priv5.txt` |
| **privileged + `cgroupns=host`** | `rw` | ✗ at root (`Permission denied`); own-subtree path untested | — | — | — | `cg-dai-cg-priv-hostns.txt` |
| **manager as host user process** | n/a | ✓ (delegation present: `cpu io memory pids`) | ✓ | ✓ | ✓ | host-side facts, P0 evidence |

*`cgroup-probe2.txt` contains `MKDIR_OK`/`SUBCTL_OK`/`MOVE_OK` lines that are **false
positives** — they ran under `/tmp/cg` on tmpfs after the cgroup2 mount failed. Kept for
the record; superseded by the later runs. (Same class of reading error the later probes
were designed to exclude: check the mount before trusting the write.)

## The mode=on recipe for a privileged container (proven in `cg-priv5.txt`)

cgroup v2's no-internal-processes rule applies to the delegated namespace root
(`nsdelegate`): enabling controllers in `…/cgroup.subtree_control` returns **EBUSY**
while the nsroot has member processes (`cg-priv4.txt`).

1. `mkdir /sys/fs/cgroup/<keeper>` — move **every** pid from nsroot into it.
2. `echo '+cpu +memory +pids' > /sys/fs/cgroup/cgroup.subtree_control` → rc=0.
3. `mkdir` session scopes as siblings: controller files present (`cpu.max`, `memory.max`,
   `pids.max`, …); limits write + process moves verified.

## Implications

- **Three containment classes** for the fixtures README / envelope: (a) unavailable →
  off-mode refusal class (default containers; ro cgroupfs); (b) on, containerized
  (privileged + keeper prep); (c) on, host-process manager (delegation already present
  host-side).
- podman already nests every container in its own `libpod-*.scope`: for a
  session-per-container shape, cgroup containment comes from the podman layer (kill =
  cgroup kill, sweep = scope cleanup) — worth naming as the fallback seam where
  in-container scopes are unavailable.
- Where the manager runs determines its class; the manager should detect and record it
  at startup (feeds the `limits.mode` detection in W2).
