# 01 — Overview

## What dAIorama is

dAIorama gives an AI agent its own real desktop environment: a real Wayland
compositor, a real browser, real windows, driven by real synthetic input at the
operating-system level, the way a human at a remote desktop session would drive
it. It is not a headless browser automation framework, and it uses no browser
automation protocol at all (no CDP, no WebDriver).

The reason is concrete: automation protocols leave fingerprints —
`navigator.webdriver`, headless flags, CDP artifacts. A real browser in a real
desktop session driven by compositor-level input has none of that; it looks
like a person using a computer because structurally, that is what is happening.

dAIorama grew out of replacing a fragile Camoufox-based setup, but it is not
scoped to that project. It is a general-purpose tool with three first-class
deployment shapes:

- embedded in a single agent's own container (agent and desktop share one container),
- a shared "central" instance several agent containers point at via `--remote`,
- bare-metal, against a desktop that already exists on the machine.

The name is deliberate: the bot gets its own small, self-contained scene — its
own diorama. dAIorama does not care about controlling *your* desktop; that it
is technically capable of it in some configurations is a side effect of the
architecture, not a design goal.

## Goals

- Real, indistinguishable desktops for agents: no automation-protocol fingerprints.
- Rootless throughout: fully functional in rootless Podman with `--cap-drop=ALL`; needing root is treated as a signal the solution isn't clean yet.
- Inert by default: nothing listens on a network port, daemonizes, or starts a desktop unless something explicitly asked for it.
- One mechanism per concept: where a feature looks like "simple version vs flexible version", it is the same mechanism with a different config value.
- Fail loudly and cheaply: no silent degradation, no auto-created sessions, no crash-loop masking.

## Non-goals (do not build these in later)

- **General shell/command execution.** Program launch on the desktop is the compositor's own `exec` action, one entry in the control surface. Everything beyond that belongs to the harness.
- **TLS / certificate management / own cryptography.** Delegated to overlay networks (WireGuard-family, e.g. Tailscale) and mature reverse proxies.
- **Broad package-manager coverage.** GitHub Releases, a Homebrew tap (client-only, see 09), crates.io. Community packaging may happen organically; don't chase it.
- **Full IPC surface exposure at launch.** The internal capability registry grows toward comprehensive over time; the shipped default tool set is the essential set (05).
- **Dashboard relay/tunneling for remote VNC.** Provisioned
  architecturally (the viewer is a noVNC widget fed by websocket data, so a
  relay can be swapped in later), deliberately not built now — v1 remote
  viewing on shared shapes is explicit exposure only (07), and media
  capability fetches are request-forwarding, not this relay.
- **File transfer in and out of sessions.** Persisting a browser profile is a persistence problem and is in scope (09); moving files in/out (downloads, uploads) is a transfer problem and is deferred, named here so it isn't half-built.
- **Audio virtualization, dynamic mid-session display changes.** v1 cuts, named. If audio is built later it follows the standing-stream shape the VNC feed already uses (05), not an MCP tool call.
- **Live, zero-downtime manager upgrades.** Upgrading a shared manager briefly drops its sessions; documented tradeoff (10), no hot-swap infrastructure.

## Design principles (guide for decisions this spec doesn't cover)

1. **Rootless is first-class.** Every choice is filtered through "does this work without root, specifically in rootless Podman with `--cap-drop=ALL`".
2. **Lean by default, opt-in for everything else.** A misconfigured or fresh install is inert, not accidentally exposed or accidentally running things.
3. **No implicit magic.** No default session; every desktop-touching action requires an explicit session ID; a missing one gets an error telling the caller to create one — never an auto-created session.
4. **Compose mature tools.** CDI for GPU access, noVNC for browser viewing, overlay networks/reverse proxies for transport security. The job is the bridge between agent and desktop, not reinventing device permissions or cryptography.
5. **Config-level restrictions are harm reduction, not security.** Quotas and tool manifests catch bugs and steer well-behaved agents; they are not a wall against code-executing agents. The real boundary is the container/OS isolation (07 states the same, plainly).
6. **Build the general mechanism once.** Configuration decides the shape.
7. **Naming: technical inside, thematic outside.** Internal names, RPC fields, tool names, CLI flags stay plain and literal; user-facing display nouns may use the diorama theme (02 pins the exact policy).
