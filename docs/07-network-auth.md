# 07 — Network surfaces, tokens, scopes

This file is the complete enumeration of network-reachable surfaces. Every new
surface is added to this table when introduced — the previous design failed
exactly here (the flagship bridge shape was missing from the enumeration and
the view-port gate had no enforcer).

## The surfaces

| surface | default | when exposed beyond loopback |
|---------|---------|------------------------------|
| manager RPC | unix socket, zero-config, tokenless | TCP bind requires instance token, always |
| MCP bridge, network mode | loopback (`127.0.0.1:8757`) | instance token **and** per-connection scope auth (below) |
| view front (noVNC + proxy to wayvnc) | loopback | instance-token-derived credential, enforced by the front itself |
| dashboard | loopback | same rule as everything else; its own manager connections carry per-connection tokens |
| media endpoint (`/media/<id>`) | on the manager surface | same rule as the surface it rides on |
| unix socket | filesystem permissions only | — (stays tokenless, ever) |

## The exposure rule (one, everywhere)

- **The gate is request-time, not bind-time.** Bind-address checks are fragile
  the moment anything proxies or publishes a port; the component in front
  enforces per request instead. The unix socket stays tokenless — the same
  reasoning as `/var/run/docker.sock`.
- **Any non-loopback bind requires the instance token configured.** It is
  generated automatically the first time a non-loopback bind is requested and
  none exists; if it cannot be generated (read-only location), startup refuses
  loud — string in 04. Never a silently open port.
- **The view port has a real enforcer now (A3).** The manager-owned **view
  front** serves the noVNC static page, checks a token-derived credential
  (cookie or one-time code — a browser cannot send an Authorization header to
  a VNC endpoint, and wayvnc implements no such token), and proxies to a
  loopback wayvnc. Confirmed in the field: wayvnc speaks RFB-over-WebSocket
  only and never serves the noVNC page, so a companion server is required
  regardless — the gate belongs in front of both.
- **TLS is never dAIorama's job** (01 non-goal). Documented pattern: overlay
  network for encryption + identity (Tailscale/WireGuard family), and/or a
  mature reverse proxy for TLS termination, for anyone exposing further.

## Instance token lifecycle (A5 — one rule, no contradiction)

- **Generated on first non-loopback bind** if none exists; written to
  `<data>/<instance>/token` (0600, its own file, never in ordinary config);
  the path and value are printed **once** at that startup (the operator needs
  the value to configure clients; it is not re-printable except by `token show`).
- **The previous contradiction is resolved:** the manager does not "generate on
  first use" *and* "refuse without one" — it generates when it can, and refuses
  (04 string) only when generation is impossible while a non-loopback bind was
  requested.
- **Client side:** supplied via `DAIORAMA_TOKEN` (exact name) alongside
  `--remote`, never as a CLI argument (shell history, process listings).
  Missing-token client error: 04 string.
- **Scope is separate and finer:** this token is the coarse network gate;
  scope identity applies after it, not instead of it.
- **Rotation (A5):** `token rotate` regenerates the file and states plainly
  that the new token takes effect at next manager start and that restarting
  drops every running session (PDEATHSIG — there is no running state to
  migrate, and the previous "no running-state to migrate" note missed exactly
  this). Live token reload is not in v1; deliverable alternative recorded in 11.

## Scope model (A4 — decided: authenticated over network, cooperative on unix)

Scope is the application-layer identity carried on **every** call; the manager
enforces quotas and tool manifests against it regardless of transport.

- **stdio bridge:** scope set once via `DAIORAMA_SCOPE` (+ `DAIORAMA_SCOPE_TOKEN`
  when the scope has a secret) in the harness's own MCP client config. Process
  environment is the authentication; nobody else can set it for that process.
- **network bridge:** per-connection authentication — the connection presents
  scope name + scope secret, validated by the manager on the same presentation
  path as the instance token. **A claimed string is never authentication**; a
  scope without a secret cannot be used over the network.
- **local unix socket:** scope claims are cooperative and documented as such.
  The socket is filesystem-gated, and anything with code execution on the same
  host can bypass config-level restrictions anyway (01, principle 5) — local
  scope authentication would be theater. The shared/central shape, where
  differently-trusted callers come from different boxes, is where scope auth
  is real, and that is the network path.
- **Reserved scope `operator`:** sessions created from the CLI carry it; CLI
  reads are operator-wide. MCP callers default to own-sessions-only; broader
  visibility is deliberate config (`sessions.visibility = all`), never default.
- Scope definitions (quotas, manifests, audit flags) are files — see 08.
- Errors: `scope_unknown`, `scope_auth_required` (05).

## Foreign sockets and interop (B7)

The runtime-directory bind-mount trick is scoped to **CLI/RPC discovery**:
talking to a foreign, compatible **manager** is the stated feature. Guards:

- **Identity probe before adoption, always.** Any discovered socket is probed
  for protocol version + instance ID before any RPC. "My socket" vs "a foreign
  socket" is decided by that probe, never by path alone.
- **Attachment is explicit, never a side effect** of session creation.
- **A foreign compositor socket is not adoptable.** Hyprland's own socket
  found via a bind-mount does not become a session: no child, no PDEATHSIG, no
  ownership — whose session would it even be? Clear refusal, not best-effort.
- **Boundary-inversion hazard, documented as the guard's reason:** if a
  container CLI adopts the *host* manager, session RPCs execute on the host —
  desktops run outside the container and its isolation boundary. That hazard,
  not tidiness, is why adoption is explicit and probed.

## Rate limiting (B9 — requirement on record, mechanism open)

A session driven at inhuman control-call rates is itself a detection signal —
it cuts against the reason the project exists. Requirement: a per-session
control-call rate cap exists for shared/central shapes; exceeded calls fail
`rate_limited` (05). Mechanism is **must-decide-before-ship for shared
shapes**, tracked as an open item in 11 — not generic abuse boilerplate, and
not omitted because the mechanism is undeveloped.

## Posture statement (repeated here on purpose)

None of these config-level restrictions are a security boundary against a
caller with code execution in the same environment: the compositor's input and
capture protocols grant capability to any client holding a socket connection,
and a few lines of code bypass a manifest entirely. Quotas, manifests, and
tokens are harm reduction — bug-catching and clear steering for well-behaved
callers. The real security boundary is, and must remain, the container/OS
isolation layer around the whole thing.
