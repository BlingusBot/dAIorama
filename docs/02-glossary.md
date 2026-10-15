# 02 — Glossary

Nouns pinned once. If a term is defined here, no other file redefines it.

| term | meaning |
|------|---------|
| **dAIorama** | The product. |
| **daiorama** | The CLI binary and command name (`daiorama new`, etc.). |
| **instance** | One manager plus everything it owns, addressed as a unit. Named; default name `default`. Several instances can coexist on one host (each gets its own runtime path, data, token — see 08). |
| **manager** | The one persistent process per instance; owns sessions, views, the registry, and the history log. The only daemon that exists. "The manager" in a command context means the instance being targeted (local by default, `--remote` to pick another). |
| **session** | One desktop: one compositor process (Hyprland) plus everything launched into it, owned by the manager. Created explicitly, closed explicitly. |
| **view** | A live VNC stream of one session (wayvnc + noVNC page behind a manager-owned front). A session may have zero or one active view. `--view` requests one at creation; `get_view` starts or returns one on demand. |
| **scope** | The application-layer identity an MCP caller acts as. Carries a quota profile and a tool manifest. Enforced by the manager on every call. |
| **scope token** | Per-scope secret that authenticates a network caller as that scope (07). |
| **instance token** | The manager-wide secret gating non-loopback network access to an instance (07). Env var: `DAIORAMA_TOKEN`. |
| **bridge** | The MCP bridge: speaks MCP to a harness on one side, talks to a manager socket on the other. Stateless w.r.t. sessions; owns nothing. Entry point: `daiorama mcp` (04, 05). |
| **dashboard** | The optional web UI aggregating sessions/history across managers. A separate, optionally-installed module (09). A plain client of the manager API; the manager has zero dashboard-aware code. |
| **daemon** | Standard meaning: the manager running detached. `daemon` is not a second process — there is exactly one daemon (the manager). |
| **backend** | How the compositor produces output: `headless` (default), `nested`, `host` (03). |
| **reaper** | Optional idle-timeout sweep that closes sessions nobody is using. Off by default (06). |
| **drain** | Operator state: new sessions refused, existing ones reported, until empty (10). |
| **label** | Human-readable name attached to a session or connection. Mutable, non-unique, never used as an identifier. IDs are stable and opaque; rename changes labels only. |
| **runtime dir** | Per-instance socket directory (default under `$XDG_RUNTIME_DIR`). |
| **data dir** | Per-instance persistent directory: history, token file, modules (08). |
| **config dir** | Per-instance configuration: `config.ron`, `scopes/` (08). |
| **history** | Append-only log of sessions and control calls, tagged by origin (shell or MCP) and scope. Per-session, rotated, capped (06, 08). |
| **media artifact** | Screenshot/clip returned by a capture tool; ephemeral by default, persisted only under explicit per-scope audit config (05). |
| **view front** | The manager-owned HTTP+WebSocket listener in front of a session's view: serves the noVNC page, enforces the token gate, proxies to the loopback wayvnc (07). |

## Naming policy

- **Plain and stable for anything an operator or agent reads in a machine context**: CLI names, flags, RPC fields, MCP tool names, error codes and messages, statuses. These get grepped and string-matched; wording is part of the contract.
- **Theme nouns are display-only**: a session may be a "daiorama" and the manager an "exhibition" and the dashboard a "museum" in dashboard chrome and marketing prose — nowhere else.
- **The collision is resolved here**: the product is dAIorama; no normative surface ever calls a *session* a "daiorama". Docs, CLI, errors say "session".
- Error and status strings are written once, quoted in this spec (04), and reused verbatim by CLI, bridge, and dashboard — no per-surface paraphrases (see 04, "Refusal and error strings").
