# P0 evidence — BlingusBot reference box (2026-10-15)

Raw outputs backing the box answers in `../blingus-findings.md`.
Usernames are masked (`agentuser` / `adminuser`); uids, modes, and all
numeric facts are preserved.

## What each file is

| file | produced by |
|------|-------------|
| `host-state.txt` | privileged throwaway container bind-mounting host `/dev`, `/sys/class/drm`, `/sys/module`, `/sys/fs/cgroup`, `/etc/{group,passwd,os-release}` read-only → host GPU/DRM/group/cgroup state |
| `gpu-test.out` | container with renderD128 passed (`devices` entry); open+DRM ioctl; default caps |
| `gpu2-test.out` | same, `cap_drop: ["ALL"]`; incl. in-container cgroup writability test |
| `gpu3-test.out` | same as gpu2 re-run under the final scripts (probe hygiene) |
| `egl-test.out` | EGL/GBM (platform-display) attempt that **segfaulted** — kept for the record; superseded by egl2 |
| `egl2.out` | EGL **surfaceless** GLES test (`GL_RENDERER = NV117`) + `eglinfo` output + Mesa package versions — the working path |
| `libcheck.out` | Mesa/DRI/EGL library inventory of the probe image |

## Reproduce

All containers were created on the host via the rootless podman REST socket
(`$XDG_RUNTIME_DIR/podman/podman.sock`), e.g.:

```sh
# host-state dump
curl --unix-socket "$XDG_RUNTIME_DIR/podman/podman.sock" \
  -H 'Content-Type: application/json' -d @specs/probe-host-spec.json \
  http://d/v4.0.0/libpod/containers/create
# ...then POST /containers/<name>/start, read logs / the mounted /out dir
```

Spec files used are in `specs/` (paths masked); probe scripts in `scripts/`.
The GPU spec's operative part:

```json
"devices": [
  {"path": "/dev/dri/renderD128", "type": "c", "major": 226, "minor": 128,
   "fileMode": 438, "permissions": "rwm"}
]
```

Note: `inspect` does not reflect this field (`HostConfig.Devices` stays
`[]`); verify inside the container. The device is `crw-rw-rw-` on this host,
so no `keep-groups` was needed.
