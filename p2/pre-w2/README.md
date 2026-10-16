# P2 pre-W2 bundle — resize/scale extension + containment/renderer/doctor experiments

2026-10-15. Evidence-first; every claim has its raw log under the subdirectories.
Nothing here depends on the fixtures; all of it feeds W2/W4 (and L3 for the renderer part).

## resize-scale — creation params + live resize (carried P1 proof, now green)

Semantics confirmed on the reference box: Hyprland `monitor` resolution = output **mode**
(buffer pixels); `scale` = logical divisor (logical = mode ÷ scale); grim/screencopy
captures at **buffer pixels**.

| request | monitor json (w×h, scale) | grim PNG |
|---|---|---|
| mode 2560x1440@60, scale 1 | 2560×1440 @1 | 2560×1440 |
| mode 1920x1080@60, scale 2 | 1920×1080 @2 | 1920×1080 |
| mode 3840x2160@60, scale 2 | 3840×2160 @2 | 3840×2160 |
| mode 1280x800@60, scale 1 (live shrink from 3840×2160) | 1280×800 @1 | 1280×800 |
| mode 1920x1080@60, scale 1.5 | 1920×1080 @1.5 | 1920×1080 |

- Live resize works in place: `hyprctl keyword monitor "HEADLESS-R,<mode>,0x0,<scale>"`;
  input (`wtype`) and window reflow survive every transition (visual check in the PNGs).
- Spawner implication: creation params should be `(logical_resolution, scale)` →
  mode = logical × scale. A 1920×1080-logical desktop at scale 2 = mode 3840×2160 (row 3).
- **Carried into build:** the spawner's create path now uses exactly this — `(logical, scale)`
  → mode = logical × scale (merged with the runtime slices); the live-resize op wiring rides
  the session-life-cycle slice. This section stands as the carried proof for the resize/scale
  exit item.

## sweep — containment primitives for W2/W4 (all green)

- Scope + limits: `memory.max`/`pids.max` written and read back OK (keeper prep per
  `p2/cgroup-matrix`).
- **Kill path `cgroup.kill`**: one write reaps every process in the scope (tree included);
  scope rmdir succeeds after.
- **Parent-death (PDEATHSIG)**: child with `PR_SET_PDEATHSIG=SIGKILL` dies when the
  manager is SIGKILLed.
- **Fork inheritance**: children born *after* the parent is inside a scope land in that
  scope (forked child reported `0::/dai-inh`) — the manager's correct order is
  spawn-into-scope, then fork.
- **Startup sweep**: stale `dai-sess*` scope holding a live process → kill+rmdir pass
  leaves nothing behind, no leaked process. Manager boot pattern: scan scope prefix →
  `cgroup.kill` → rmdir.

## renderer — allow_software path: detection works, rendering/capture doesn't (this stack)

- GL forcing matrix (surfaceless EGL): `LIBGL_ALWAYS_SOFTWARE=1` and
  `MESA_LOADER_DRIVER_OVERRIDE=llvmpipe` both yield `llvmpipe (LLVM 22.1.8, 256 bits)`;
  `GALLIUM_DRIVER=llvmpipe` has no effect on this path.
- **Soft compositor runs and its renderer is reportable**: nested Hyprland started with
  the software env reports `Renderer: llvmpipe (…)` in its instance log — exactly the
  string a `renderer.allow_software` gate would read.
- **But a soft session produces no frames**: its headless output's swapchain fails
  (`GBM: Failed to allocate a GBM buffer: bo null`, `Swapchain: Failed acquiring a
  buffer`), so capture/view fail (`grim` → `failed to create buffer` in every variant,
  including soft-env and ppm; `wayvnc` starts but never listens on 5900). No frames =
  nothing to copy.
- **Full-software parent is worse**: mutter under llvmpipe advertises
  `zwp_linux_dmabuf_v1` v3 while aquamarine hard-binds v4 → protocol kill →
  `CBackend::create() failed` crash. Second hard version gate (same class as L1's
  `xdg_wm_base ≥6` rule). L3-relevant: a fully software chain needs an aquamarine clamp
  or a different parent/buffer route.
- Browser leg: chromium on the soft session runs WebGL via **SwiftShader**
  (`ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero)…))` captured from the
  window title — the launcher-capture mechanism works for software too, no CDP).
- Implication for `renderer.allow_software` semantics: software sessions are
  *detectable and gated* but **not viewable/capturable** on this stack — the flag buys
  CI/protocol value, not pixels, until a working buffer/export path exists (candidates:
  aquamarine version clamp, shm-only export, newer aquamarine).

## doctor — host-probe prototype

`probe.py` emits JSON: tool presence + versions (Hyprland 0.56.2, mutter 50.5, wayvnc
0.10.1, chromium 155), GPU render node + surfaceless-EGL renderer (NV117), containment
class (off-mode in the plain container; on-classes per `p2/cgroup-matrix`), runtime dir.
W2/W3 can wire this directly as the doctor's host side; needs nothing from the fixtures.

## Notes

- All runs: rootless podman, `cap_drop ALL`, single `renderD128` device.
- `renderer/evidence/abort-first-run.*` = the first (mis-designed) software run — kept
  as the first occurrence of the dmabuf v4-on-v3 kill; `h3-soft-soft.*` is the same
  crash reproduced deliberately (mutter under llvmpipe).
- `diag.txt` is the env-forcing matrix run; its chain attempts had a mis-targeted log
  grep (wrong file) — the decisive chain evidence is `diag2.txt` + instance logs.
