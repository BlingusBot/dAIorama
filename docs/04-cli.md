# 04 — CLI surface

Names, flags, defaults, and refusal text are part of the contract: agents and
scripts string-match them, so they are pinned here and changed only by spec
amendment. All names are plain; no theme words (02).

Global shape: `daiorama <command> [flags]`. `--help` on every command surfaces
creation/usage options in place (D1). Read commands accept `--json`.

## Global flags and environment

| flag/env | meaning |
|----------|---------|
| `--instance <name>` | Which local instance to target. Default `default`. |
| `--remote <addr>` | Which manager to talk to: `host:port` or a literal unix socket path. One flag, one meaning, everywhere. Overrides `remote.default` for that call. `--remote` NEVER triggers a local start (B5). |
| `--json` | Machine-readable output on read/status commands. |
| `DAIORAMA_TOKEN` | Instance token for non-loopback access (exact name, pinned — nits). |
| `DAIORAMA_SCOPE` | Scope name for a stdio bridge process (05). |
| `DAIORAMA_SCOPE_TOKEN` | Scope secret for that scope, when it has one (07). |

## Commands

**Sessions** (all take `--remote`):
- `daiorama new [--backend headless|nested|host] [--resolution WxH] [--scale F] [--view] [--label NAME]` — create a session. Defaults: backend `headless`, resolution `1920x1080`, scale `1.0`. Prints the session ID.
- `daiorama close <session>` — explicit teardown. Always explicit; never a side effect (01).
- `daiorama list [--all]` — sessions with status, scope, origin, idle-since. Default: the caller's own scope's sessions; `--all` for everything (operator view).
- `daiorama status [<session>]` — no argument: instance status (below). With ID: session detail (state, view state, address, idle-since, versions).
- `daiorama control <session> <action> [args]` — one control action, same action set as the MCP control surface (05).
- `daiorama view <session> [--stop]` — print the view address (starting the view if needed, same mechanism as `get_view`), or stop it.

**Instance management:**
- `daiorama serve` — run the manager in the foreground (container main process, deliberate terminal supervision, systemd `ExecStart`). Signals cascade to everything it owns.
- `daiorama daemon start` — start the manager detached (survives shell exit).
- `daiorama daemon end [--force]` — stop the manager. Plain `end` refuses while sessions are live and lists them; `--force` stops the manager and everything it owns (a session drop, stated in output).
- `daiorama status` — instance status: state, mode (attached/background), pid, socket path, core + protocol versions, session counts, drain state. This is the single manager-status surface (no separate `daemon status`).
- `daiorama drain [--wait]` — enter drain: new sessions refused (code `draining`), running sessions reported. `--wait` blocks until zero running, bounded by `--timeout <secs>`.

**Config** (authority rules in 08):
- `daiorama config get <key>` / `config set <key> <value>` — dot-path keys, max one level of nesting. Against `--remote`: RPC to that manager. Local, no manager needed: operates on the files directly (cold).
- `daiorama config edit` — `$EDITOR` on the core config file. Refused against `--remote` (explicit refusal, not fetch-edit-push). Scope files are edited where they live (08); `config scope` commands are for reading them.
- `daiorama config setup` — first-run wizard: detect environment (deps, GPU via `doctor`), write starter config, offer commented starter scope files. Sequence in 10.
- `daiorama config reload` — re-read core config on a running manager (reload rules: 08). Targets a running manager; if none is running: `no manager running` — config applies at next start anyway. Never auto-starts (cold class).
- `daiorama config validate [<path>]` — validate core config and/or scope files; unknown tool/category names are load errors listing valid names (D4).
- `daiorama config scope list` / `config scope get <name>` / `config scope validate <name>` — scope defs are first-class, readable and scriptable (D4).
- `daiorama config effective [--scope <name>]` — resolved view: which values apply where, effective quotas/manifests/audit flags per scope (D4).

**Token** (lifecycle: 07):
- `daiorama token show` — print the local instance token (and its file path).
- `daiorama token rotate` — regenerate the token file. States plainly that the new token takes effect on next manager start, and that restarting drops running sessions (07).

**MCP bridge:**
- `daiorama mcp [--listen <addr>] [--manager <addr>]` — run the bridge. Default: stdio, targeting the local manager. `--listen` runs it as a network service (auth rules in 07). This is the entry every harness config points at.

**Diagnostics and packaging:**
- `daiorama doctor` — host-runnable environment check: GPU family + exact next step, Hyprland/wayvnc presence and versions, Wayland, cgroups v2 delegation, package-manager-specific install commands. In-container it verifies the actual renderer and fails loud rather than diagnosing blindly (09).
- `daiorama dashboard serve` — serve the dashboard (module must be installed).
- `daiorama dashboard install [--from <path|url>]` — install the dashboard module (mechanism: 09).
- `daiorama dashboard connect <address>` / `dashboard connections list|remove <name>` — dashboard-side manager connections (D3; flows in 10).
- `daiorama dashboard token set|remove <connection>` — store/remove per-manager tokens in the dashboard config (0600).
- `daiorama version` — core version + protocol version (skew display, C3).

## Pinned defaults

| setting | default |
|---------|---------|
| instance name | `default` |
| manager socket | `$XDG_RUNTIME_DIR/daiorama/<instance>/manager.sock` |
| manager TCP port (when bound) | `8756`; non-loopback requires token (07) |
| bridge network listen | `127.0.0.1:8757`; non-loopback requires token + scope auth (07) |
| view front port base | `6080`, allocated upward per active view (06) |
| resolution / scale | `1920x1080` / `1.0` |
| idle reaper | off (06, rationale in 11) |
| history | 8 segments × 4 MiB per session; retention 7 days after close (08) |
| token file | `<data-dir>/<instance>/token`, mode 0600 (08) |
| media TTL | 60 s or fetch-once, whichever first (05) |
| manager start mode | `auto` (attach/background heuristic, 10); pin via `manager.start_mode = auto|attached|background` |

## Cold vs hot (auto-start classification, B5)

"Hot" = may lazily start a local manager when none is reachable. "Cold" =
answers from files/void and never starts one. With `--remote`: nothing local is
ever started; unreachable remote is an error, never a local fallback.

| class | commands |
|-------|----------|
| cold | `list`, `status`, `config get/set/edit/reload/validate/setup`, `config scope *`, `config effective`, `version`, `doctor`, `token show`, `token rotate`, `dashboard install/connect/connections/token` |
| hot | `new`, `close`, `control`, `view`, `drain`, `daemon end` |
| neither | `serve`, `daemon start`, `mcp` (explicit process runners) |

A cold command against "no manager" is an answer, not an error: `no manager
running`. When a hot command lazily starts one it prints one line: `started
manager (mode=<attached|background>, pid=<pid>) socket=<path>` (B5).

## Refusal and error strings

Canonical, shared verbatim by CLI, bridge, and dashboard (D2). Operators grep
these; wording changes are spec changes.

- `manager unreachable at {addr}` — the shared reachability string. Never followed by a local-start attempt when `--remote` was given.
- `no manager running` — cold commands' honest answer.
- `Hyprland not found. Install it: {exact command for the detected package manager}, or use the container image.`
- `Hyprland {found} is outside the supported range {min}-{max}.`
- `session refused: active renderer is {renderer} (software). This session would carry a known automation fingerprint. Run 'daiorama doctor'.`
- `refusing to bind {addr}: non-loopback binds require a token; none could be generated at {path}.`
- `token required for {addr}: set DAIORAMA_TOKEN (value from 'daiorama token show' on the manager host).`
- `session limit reached: scope '{scope}' {n}/{max}; global {g}/{gmax}. Close a session first.`
- `backend 'host' is exclusive; in use by session {id}. Close it or use --backend=headless.`
- `manager is draining for upgrade; new sessions are refused ({n} still running).`
- `protocol mismatch: this client speaks {cv} (min {cm}); manager speaks {mv} (min {mm}).`
- `no session specified; create one first (tool: create_session).` — MCP-side form of the missing-ID error.

Final wording is frozen at sign-off; that is the last moment wording may change freely.
