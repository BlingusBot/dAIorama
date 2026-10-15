# 09 — Packaging, releases, and the container image

## Distribution channels (§8, scope stated)

| channel | covers | scope |
|---------|--------|-------|
| GitHub Releases + install script | primary; static binary | full (client + manager) on Linux |
| Homebrew tap (not homebrew-core) | secondary | **client-only** — macOS gets client subcommands only; manager subcommands are Linux-only (stated in README + manifest, not implied) |
| crates.io | tertiary, for Rust-literate users | never required for end users |
| container image | builds the binary directly | bypasses channels entirely |
| broader packagers (nixpkgs, AUR, apt/dnf) | none | community may do it later; not chased (01) |

Install script: `--client-only` installs CLI + bridge without manager
preflight — the harness-side artifact must exist where the harness runs (an
agent's own container), and this is its named delivery (C2).

## Release manifest (C2)

- **Signed per-release manifest**: artifacts × target triples × sha256.
  Compose snippets and install commands pin by **digest**.
- **One multiplatform core binary** containing CLI + manager + bridge (bridge
  is a subcommand, also standalone-invokable). Versioned as core semver. The
  protocol version is its own semver; the compat table ships with release notes.
- **Dashboard is a separate module artifact** (below) — never a compile-time
  feature flag: prebuilt downloads don't shrink with flags, a real module
  shrinks the default download.
- Client-only install mode and macOS client scope are manifest facts.

## Dashboard module (C1 — a mechanism, not a name)

`daiorama dashboard install`:

- Downloads the release asset for this target triple, **version-matched to the
  core binary by default**; `--version <v>` or `--from <path|url>` (offline)
  override.
- Verifies checksum from the manifest.
- Installs to `<data>/modules/daiorama-dashboard/<version>/`; activation is an
  atomic symlink swap; the previous version stays installed — reverting is
  re-activating it explicitly (`--version <old>`), deterministic, no mystery
  rollback state.
- **Container case stated:** runtime downloads evaporate on container recreate.
  The module belongs on the persisted `/data` mount (08); the alternative —
  an image variant that bundles it — is documented alongside. Compose snippets
  cover both.
- `daiorama dashboard serve` refuses with a clear "module not installed,
  run dashboard install" when absent. The dashboard is never something that
  springs into existence as a side effect of `--view` or anything else.

## Container image (§9)

- **No default `CMD`** — reflected honestly, with ready-to-copy compose
  snippets, one per legitimate shape: bare bridge → remote manager; full local
  stack; manager-only exposing itself to a thin remote bridge; harness + desktop
  sharing one image. Snippets are the documentation.
- Common-case compose stays genuinely simple: one GPU line, MCP port, VNC port,
  optional volume.
- **Persistence scoped narrowly:** browser profile, cookies, logins survive;
  session state is disposable by design and gets no volume.
- **Bridge binding supports both shapes equally:** published port (flagship —
  agent's container → dAIorama's container) and localhost/mounted unix socket
  (harness installed in the same image). No binding assumption that serves one.
- **Shared/central shape:** a single central instance many agents point at via
  `--remote` — zero special handling, same manager, same registry.
- **GPU recipes per family** (C4/C5), listed with host prerequisites:
  AMD/Intel — render-node passthrough recipe (`--device /dev/dri/renderD128`,
  `--group-add keep-groups`, SELinux boolean where enforcing) with the honest
  caveat that group-mapping rough edges are currently active in Podman and
  `ls -l /dev/dri/renderD128` on the target is the source of truth; NVIDIA —
  CDI (`--device nvidia.com/gpu=all`), host requires Container Toolkit ≥ 1.18
  (auto-refreshing CDI spec). Exact recipes resolve in the spike; the compose
  snippets carry the validated versions.
- Crash handling: no auto-restart of the compositor; dead session + explicit
  fresh `new` (06).

## Bare-host dependencies and version checks (C5)

| dependency | check | on failure |
|------------|-------|------------|
| Hyprland | presence + version range | missing: exact install command for the detected package manager + point at the image; out of range: named-mismatch error (strings, 04) |
| wayvnc | presence + version | install command / error |
| screen capture | — decided below | — |
| browser | detected (bare-host: the user's own; container image: ships Chromium) | bare-host: refusal naming the requirement; profile persistence per 09 volumes |
| noVNC assets | shipped with the core (embedded) | — (view front serves them, 07) |
| clipboard | capability check at session start | degrade loudly in `doctor`, not silently |

**Screen capture decision (C5, made):** in-binary screencopy-protocol client.
No external `grim`-style tool — this keeps the static-binary claim intact.
The browser question is likewise named, not deferred: who supplies it and
where it lives is in the table above.

**Version-check pattern (§8):** startup checks Hyprland's IPC surface against
a known-compatible range and fails with a specific, named error — a confusing
downstream IPC failure becomes an immediate legible one. In-container startup
additionally verifies the actual renderer and fails loud (03); it does not
attempt to diagnose GPU passthrough from inside the container it can't see
(that is `doctor`'s job, 10).

## Testing seams = spike exit criteria (C7)

The P0 spike must **leave behind a repeatable harness**, not just a result:

- a committed container script that reproduces the full chain,
- a CI lane running protocol-level tests against the `headless` backend under
  software rendering (correctness only),
- a separate manual/GPU lane for real-hardware checks,
- fake-manager fixtures + MCP conformance fixtures,
- the renderer check.

Phrasing tension resolved: llvmpipe is functional enough for protocol tests
and **never acceptable for real sessions** (fingerprint + performance) — that
distinction is exactly why the fail-loud check exists. The real-world software-
fallback incident found during audit is the case study, not paranoia.

**Spike chain (P0, load-bearing, before anything is built on it):** rootless
Podman → render-node access → headless Hyprland → virtual pointer/keyboard →
screen capture → wayvnc → noVNC → a real, non-llvmpipe WebGL renderer string
in an actual browser. If the chain doesn't hold, the fallback (niri, or nested
compositor) must be real and tested — not aspirational.
