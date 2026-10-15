# P1 L1 — running notes (Blingus side) — FINAL(ish)

Chain goal: rootless podman → render node → headless Hyprland → virtual input → capture → wayvnc → noVNC → non-llvmpipe WebGL.

## RESULT (2026-10-15): chain green, all legs verified on the reference box

Golden run: `l1-chain-run2.txt` (+ logs + `l1-chain-input.png`, `l1-chain-webgl.png`).

1. rootless podman, cap_drop ALL, single device /dev/dri/renderD128 (mode 0666) — works.
2. parent: `mutter --wayland --no-x11 --headless --virtual-monitor 1920x1080 --wayland-display=wayland-m`
   — mutter's native headless backend: renders on renderD128, **no KMS, no seat** ("using no mode setting", "Created gbm renderer").
3. child: `WAYLAND_DISPLAY=wayland-m Hyprland --i-am-really-stupid` — aquamarine Wayland backend connects;
   `hyprctl output create headless HEADLESS-T1` → HEADLESS-T1 1920x1080@60 monitor; GPU rendering (mesa 26.2.4, NV117).
4. input: `wtype 'dAIorama-input-OK-123'` → typed text visible in a foot window in the captured frame.
4b. pointer: `wlrctl` move deltas exact (cursorpos 480,270 → 780,470 → 657,539; round 2: 480,270 → 680,420), click rc=0, cursor visibly rendered at target. Quirk: `wlrctl keyboard type` rc=0 but no delivery on Hyprland 0.56.2 (focus correct) — `wtype` stays the keyboard path.
5. capture: `grim` → PNG (screencopy, 69 globals incl. zwlr_screencopy_manager_v1 v3).
6. remote: `wayvnc 127.0.0.1 5900` LISTENs; `websockify --web=/usr/share/novnc 6080 127.0.0.1:5900`; `curl /vnc.html` → HTTP 200.
7. WebGL: chromium (ozone wayland) on the test page → title reports
   `RENDERER: ANGLE (Mesa, NV117, OpenGL ES 3.2)` — real GPU, not llvmpipe.
8. repeatability: two consecutive clean runs (run3, run4) — `exit=0`, identical leg sequence, compositor renderer lines captured inline.

Protocols present on Hyprland: zwlr_screencopy_manager_v1, zwlr_virtual_pointer_manager_v1 (v2),
zwp_virtual_keyboard_manager_v1, zwlr_data_control_manager_v1, zwlr_output_manager_v1, wp_cursor_shape_manager_v1.

## Findings (for the spec / P1 exit items)

- **Native "headless" Hyprland in a plain container is blocked with aquamarine 0.15.1** (evidence: probes/ logs):
  - Hyprland requests backend HEADLESS (mandatory) + DRM (if-available) + Wayland (fallback).
  - `CBackend::start()` REQUIRES a primary allocator — taken only from an impl with `drmFD() >= 0`.
    Headless-alone → "no allocator available" → start fails.
  - DRM backend enumerates only `card*` sysname udev nodes (`enumDRMCards` — renderD* never enumerated;
    `AQ_DRM_DEVICES` filters within that set only), and device open goes through libseat which needs an
    **active seat** — unavailable in a plain container (builtin+seatd both fail: VT-bound, no /dev/tty0).
  - `AQ_NO_KMS_REQUIREMENT=1` bypasses the KMS-capability gate but not the seat/allocator constraints.
  - Hyprland's own CI (hyprtester) runs in a VM with a real KMS card; `HYPRLAND_HEADLESS_ONLY=1` is set by
    hyprtester but **not read anywhere in Hyprland v0.56.2 source** (and the prebuilt binary does not change
    behavior with it).
- **aquamarine's Wayland backend demands parent globals at or above**: xdg_wm_base v6, wl_compositor v6,
  wl_seat v9, zwp_linux_dmabuf_v1 v4(+feedback+main_device). weston 16 (v5/v5/v9) and sway/wlroots (v5/v6/v9)
  are too old on xdg_wm_base → protocol error. **mutter 50.5 headless advertises xdg_wm_base v7, compositor v6,
  seat v10, dmabuf v5 — fits cleanly, no shim needed.** (A bind-clamp LD_PRELOAD shim `l1/xdg-clamp.c` was
  written for older parents; parked — not needed on this path. It could not work via `wl_registry_bind`
  interposition anyway since that symbol is header-inlined in modern libwayland; the right interposition point
  would be `wl_proxy_marshal_array_flags`.)
- Nested Hyprland creates its own output at start (WAYLAND-1); `hyprctl output create headless <name>` adds
  HEADLESS-N outputs on demand — this is the "dedicated virtual-output feature" the spec's headless path can stand on.
- `hyprctl` needs `HYPRLAND_INSTANCE_SIGNATURE` (grab from `$XDG_RUNTIME_DIR/hypr/`) when Hyprland was started
  without start-hyprland.
- Container facts: `cap_drop: ALL` breaks apt (setgroups) — bake tools at build time; `/run/udev` bind+relabel
  is refused (SELinux) — don't mount it; logs: `debug { disable_logs = false }` or Hyprland logs vanish into
  $XDG_RUNTIME_DIR/hypr/<sig>/hyprland.log.

## Image
`localhost/blingus/dai-l1:dev` — debian:sid + hyprland 0.56.2 + wayvnc 0.10.1 + wtype + grim + noVNC/websockify +
mesa + mutter 50.5 + foot + chromium 155 + gcc/libwayland-dev + xdg-clamp.so. Built by `scripts/build-image.sh`.

## Next
- Repo: `p1/` on branch `p1/l1-blingus` — harness + evidence + this note; PR #5 (Esther merges as the L1 bundle).
  Pointer round + repeatability double-run folded; renderer evidence added.
- Open after merge: resize/scale-path verification (spec amendment item), llvmpipe CI lane (L3, Esther).
