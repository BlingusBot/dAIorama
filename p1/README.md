# P1 — L1 spike (generic chain, reference box)

**Status: green (2026-10-15)** — the full chain verified end to end on the reference box, rootless, single render-node device. Harness + raw evidence in this tree.

Chain: **rootless podman → render node → headless compositor chain → Hyprland (headless outputs) → virtual input → capture → wayvnc → noVNC → non-llvmpipe WebGL**

| # | Leg | How | Evidence |
|---|-----|-----|----------|
| 1 | Rootless + device | `cap_drop: ALL`; one device: `/dev/dri/renderD128` (0666); no KMS, no seat | `specs/chain-run-spec.json` |
| 2 | Headless parent | `mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080` — native headless backend ("using no mode setting", gbm renderer on renderD128) | `evidence/l1-chain-mutter.log` |
| 3 | Hyprland | nested via aquamarine Wayland backend; `hyprctl output create headless HEADLESS-T1` → 1920x1080@60 monitor, GPU rendering | `evidence/l1-chain-hypr.log` |
| 4 | Input | `wtype 'dAIorama-input-OK-123'` into a foot window; text visible in frame | `evidence/l1-chain-input.png` |
| 5 | Capture | `grim` (zwlr_screencopy) | `evidence/*.png` |
| 6 | Remote | `wayvnc 127.0.0.1 5900` + `websockify --web=/usr/share/novnc 6080 127.0.0.1:5900`; `vnc.html` → HTTP 200 | `evidence/l1-chain-run2.txt`, `evidence/l1-chain-websockify.log` |
| 7 | WebGL | chromium (ozone wayland): `RENDERER: ANGLE (Mesa, NV117, OpenGL ES 3.2)` — real GPU, not llvmpipe | `evidence/l1-chain-webgl.png` |

Golden run log: `evidence/l1-chain-run2.txt`.

## Reproduce (reference box)

```sh
# 1. build the image via the host podman REST socket (no podman CLI inside the agent container)
scripts/build-image.sh          # -> localhost/blingus/dai-l1:dev
# 2. run the chain spec (libpod create+start; the container runs scripts/run-l1.sh)
podman-api run-json specs/chain-run-spec.json
# evidence lands in the /out mount; the run log ends with exit=0
```

## Findings (feed the P1 exit items)

- **Native standalone headless Hyprland is not startable in a seatless container with aquamarine 0.15.1** — verified against source + runtime: `CBackend::start()` requires a primary allocator, which only a DRM/wayland impl with `drmFD() >= 0` can supply; the DRM backend enumerates only `card*` udev nodes (never `renderD*`) and opens devices through libseat, which needs an **active seat** (unavailable in a plain container: builtin/seatd are VT-bound, no `/dev/tty0`). `AQ_NO_KMS_REQUIREMENT=1` relaxes the KMS gate but not allocator/seat. Raw probes: `evidence/probes/l1-headless4.log`, `l1-headless9.log`, `dbg6.txt`.
- **aquamarine's Wayland backend requires parent globals at or above `xdg_wm_base 6` / `wl_compositor 6` / `wl_seat 9` / `zwp_linux_dmabuf_v1 4` (+feedback +main_device).** weston 16 and sway/wlroots top out at `xdg_wm_base 5` → protocol kill. **mutter 50.5 headless advertises 7/6/10 → clean fit; no shim needed.** A bind-clamp shim was written for older parents and parked (`l1/xdg-clamp.c`; note: `wl_registry_bind` is header-inlined in modern libwayland, so LD_PRELOAD interposition must target `wl_proxy_marshal_array_flags`).
- **The nested-compositor workaround (the other named fallback in the settled compositor entry) is demonstrated green end-to-end.** Recommended `--backend=headless` implementation: invisible minimal parent (mutter `--headless` today) + Hyprland nested + dedicated `HEADLESS-*` outputs on demand (`hyprctl output create headless <name>` — the dedicated virtual-output feature the design wants, working on 0.56.x).
- Version pins observed: hyprland 0.56.2+ds-3, aquamarine 0.15.1, mutter 50.5, mesa 26.2.4 on nouveau NV117 (Quadro K2200), Debian sid base.

## Layout

- `l1/` — Containerfile (debian:sid base), `xdg-clamp.c` (parked shim), `webgl-test.html` (probe page).
- `scripts/` — `build-image.sh`, `run-l1.sh` (the chain runner), `probes/` (key diagnostic stages).
- `specs/` — podman libpod specs used (chain run, tool verification).
- `evidence/` — golden run log + per-component logs + screenshots + probe logs.
- `NOTES.md` — lab notebook (full discovery trail, gotchas, next steps).
