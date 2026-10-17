# daiorama-dashboard

The optional dashboard module (`docs/09-packaging.md`, `p5/dashboard/`). It is a plain manager
client: no manager code knows it exists, and it never gets a private RPC surface.

## Run

```
daiorama-dashboard connect local 127.0.0.1:8756
printf '%s' "$TOKEN"        | daiorama-dashboard token set local
printf '%s' "$SCOPE_SECRET" | daiorama-dashboard scope-token set local
daiorama-dashboard serve --bind 127.0.0.1:8760 --ui ./ui
```

`connect` takes the manager's `manager.bind` address; the two secrets are the instance token
and the `dashboard` scope's network secret (D2/D3). Both are read from **stdin** and never
printed; the store is `<config root>/daiorama-dashboard/connections.ron`, 0600 in a 0700
directory.

`serve` refuses any non-loopback bind with `dashboard serve is loopback-only in v1; use SSH
forwarding`: the server holds every manager token and the UI has no user authentication, so a
non-loopback bind is a different product (Q2). Requests are additionally pinned to this
listener: `Host` must be the bound authority (or `localhost` on the bound port), an `Origin`
that is present must match it, and anything else answers `403` — a hostile page whose domain
resolves to `127.0.0.1` is refused (DNS rebinding). The server holds at most 64 connections
and closes a connection whose read makes no progress for 10 s.

## API

Read-only, same-origin JSON:

| route | returns |
|---|---|
| `GET /api/managers` | per connection: `{name, address, ok, readiness|error}` |
| `GET /api/sessions?manager=<name>` | `list_sessions` for that manager, as the `dashboard` scope |
| `GET /api/history?manager=&session=&cursor=&limit=` | `list_history`, opaque cursor passthrough |
| `POST /api/view` `{manager, session}` | `get_view`; the browser follows the returned address + credential (the view's own gate, 07) |

Manager tokens never reach the browser — the module is the only holder. View credentials are
returned only by the explicit Open view action (Q1) and are not stored.

## UI

`ui/` is plain HTML/CSS/JS with no build step and no remote assets. Managers, sessions and
session detail (history + Open view) poll every 5 s. View-down is rendered distinctly from
session-dead.

## Build

```
cargo build -p daiorama-dashboard
```

The crate is a workspace member; the core binary does not include it (09: dashboard is a
separate artifact, never a compile-time feature flag). The module's protocol version is pinned
in `src/client.rs` and must match the core it ships with (D5).
