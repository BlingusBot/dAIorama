# Dashboard v1 — scope and shape (proposal for pinning)

The dashboard is a separate, optional module (03) that talks to managers as just another
client. The five seams are pinned in `README.md` (D1–D5). This note scopes **what v1 actually
is**, so the module artifact has something to build against. Two items need a decision before
the artifact exists (marked **Q**); everything else follows from 03/04/05/09/10.

## What v1 shows

One page, three panes, **control-plane read-only** (B10 reads; the one interactive affordance
is Open view, below):

1. **Managers** — one row per connection (D3): name, address, `readiness` (state, mode,
   versions, sessions running/dead, drain), last-seen + error when unreachable. A manager is
   never edited from here; connections are CLI-managed (10 D3).
2. **Sessions** — for the selected manager, `list_sessions` as the `dashboard` scope:
   id, label (or empty), state, backend, resolution, scale, origin, idle-since. **View down is
   shown distinctly from session dead** (06, 10): the row carries the session state and the
   view state separately, and the copy never conflates them.
3. **Session detail** — `session_status` plus a `list_history` pane (opaque cursor paging, the
   same read surface as the CLI). A history record is rendered with scope/origin/cause; bytes
   and counts only — the log has no arguments or media ids by construction (06).

**Control-plane reads only**: no create, close, drain, config, screenshot or capture — a
dashboard that can kill a desktop is a different authorization conversation. The one exception
to "read-only" is Open view, and it is deliberately named as such: **RFB is input, not
observation**, the front has no read-only mode, and the hand-off runs under the session's own
view gate (07), not the dashboard's scope.

## Opening a view

The only action v1 offers is **Open view**: `get_view` for a session, then the browser is
handed the front URL the manager returned (address + credential). The credential remains the
view's own bootstrap capability (07); the dashboard does not mint a second auth path and does
not store the credential. The dashboard never proxies the RFB stream — the browser connects to
the front directly — so the interaction is a window the operator already has via `view`, and
the call itself still travels under the `dashboard` scope. Stopping a view stays a CLI action
(`view stop`) in v1.

**Q1 — include Open view in v1?** It makes the dashboard operationally useful (the thing an
operator actually wants) at the cost of one extra call and a URL hand-off. Recommendation:
**yes**, with the credential shown only in the link the browser follows — and named as an
interactive affordance rather than described as another read.

## Server shape

The browser cannot speak the manager frame protocol, so the module ships a small server:

- `daiorama-dashboard serve [--bind <host:port>]` — serves the static UI and a JSON read API
  (`/api/managers`, `/api/sessions`, `/api/history`, `/api/view`) backed by the frame client.
- The server holds connection tokens from `connections.ron` (D3) and never exposes them to the
  browser; manager calls are made server-side as the `dashboard` scope.
- Static assets are the module's own files; no CDN, no remote fonts, no telemetry.
- **Polling, not push:** the UI refreshes the selected pane on an interval (default 5 s) by
  calling `readiness`/`list_sessions`; no websocket or long-poll in v1 (B10 pull-only).

**Q2 — bind policy.** 04 defines no dashboard port and no dashboard auth. Recommendation:
default `127.0.0.1:8760`, and **refuse a non-loopback bind in v1** with a pinned message
(`dashboard serve is loopback-only in v1; use SSH forwarding`), because the server holds every
connection token and the UI has no user authentication. It is an operator-local UI: its only
control surface is a window the operator already has via `view`. Remote humans use SSH
forwarding, the same rule as the dashboard module's loopback shape in 07's table.

## Deferred to a later slice (not v1)

Docs 10 D3 defines two write interactions that core has **no RPCs for** today: take-over
(`agent | human | shared`, explicit transitions) and rename (label-only). The method surface in
05 is closed — there is no `rename_session` and no `set_input_owner` — so the dashboard cannot
implement them without a spec change plus manager work. v1 shows labels and input state only as
far as the record already carries them (labels exist today; input ownership does not).

**Q3 — where do take-over/rename land?** Recommendation: a separate slice that adds the two
manager methods to 04/05 plus the dashboard buttons, after v1 ships; adding write methods to a
read-only dashboard is exactly the kind of scope creep that makes the first release late.

## Artifact contents

`daiorama-dashboard-<version>-<triple>.tar.gz` (D5):

```
daiorama-dashboard            # server binary (frame client + static file serving)
ui/                           # index.html, app.css, app.js (no build step; no bundler output)
LICENSE                       # module licence
README.md                     # install/serve/connection basics
```

No configuration file of its own beyond `connections.ron`; everything else is flags. The module
version equals the core version it ships with (D5), and `dashboard serve` refuses absent or
skewed activation (D1/D5 strings).

## Build order (consumes packaging)

1. Packaging: manifest row + `config scope create` + installer (D5).
2. Module: static UI + server against the closed read surface; Open view if Q1 is yes.
3. Later: take-over/rename methods + UI (Q3), if the operator wants them.

Nothing here requires a manager change beyond D1 (`versions.dashboard_module`) and D2 (the
`dashboard` scope), both already pinned.

## Implementation status (2026-10-17)

The module scaffold exists: `dashboard/` is a workspace member (`daiorama-dashboard` binary,
plain `ui/` assets), with the D3 store, the loopback server and the read API above. It is not
yet wired to any installer (D5 is packaging's), and the dashboard CLI verbs under `daiorama`
still refuse — the module runs standalone until that wrapper lands.

Verified so far: unit + end-to-end tests (fake manager over the real frame protocol: readiness,
sessions, history, view, traversal refused, tokens absent from every response body, non-loopback
bind refused with the pinned string), plus a live leg against a real manager —
`/api/managers` returned the manager's real readiness through the `dashboard` scope, page 200,
and no token appeared in the body.

**D3 amendment:** each connection also stores `scope_token` (the `dashboard` scope's secret);
the instance token alone cannot authenticate a network call (07). `scope-token set` is the
store verb; both values arrive on stdin.
