# Dashboard seams — pre-read (P5)

The dashboard is an optional, separately installed web UI that aggregates sessions/history
across managers (02, 03). By rule the manager has **zero dashboard-aware code**; the dashboard
is a plain pull-only client of the manager API (03, B10). This note inventories the seams that
already exist, the ones that are still open, and the questions packaging (09) and the dashboard
slice have to answer before code.

## What exists

- **Identity and transport.** The `manager.bind` TCP surface with the instance token plus the
  per-request scope credential (`07`, P4-ENDPOINT) is the path a remote dashboard uses. A
  dashboard holds one token per manager (04: `dashboard token set|remove <connection>`) and
  names the scope it queries as.
- **Read surface.** `status` (instance state, versions, session counts, drain), `readiness`,
  `list_sessions` (scoped, paginated), `session_status`, `view`, and `history` (scoped, opaque
  cursors) are everything a v1 dashboard needs for a session/history pane. No new RPCs required
  by this pre-read.
- **Module field.** `status`/`readiness` carry `versions.dashboard_module?` (05) — the manager's
  report of the installed dashboard module version.
- **Install mechanism (09).** `daiorama dashboard install` downloads a version-matched module
  artifact into `<data>/modules/daiorama-dashboard/<version>/` and activates it by atomic
  symlink swap; `dashboard serve` refuses with the pinned string when absent. `dashboard *` is
  currently `not_implemented` in the CLI — that is the implementation surface, not a design gap.
- **Reserved query scope.** 10 (D3) pins that the dashboard queries as the reserved `dashboard`
  scope, not as `operator`.

## Open seams (questions to settle)

1. **Who supplies `versions.dashboard_module`?** Today it is always `None` (the manager never
   looks at `<data>/modules/`). Options: (a) read the active symlink target at manager start and
   on `config reload`; (b) a config key written by `dashboard install`; (c) leave it `None` until
   a manager-side module registry exists. (a) keeps the "no dashboard-aware code" rule — it is a
   directory read, not a dependency — and gives `status` a truthful optional field. Needs a pin
   in 05/09 and one test (absent, active, dangling symlink).
2. **How is the reserved `dashboard` scope created?** `config setup` writes `operator` only, and
   `config scope` has `list|get|validate` — there is **no create path** in the CLI. The dashboard
   needs `scopes/dashboard.ron` plus a network secret (P4 mints secrets only for files that
   already exist). Options: `config scope add <name> --template dashboard`, or have
   `dashboard install` write the file. Either way it needs a pinned template (visibility `all`,
   audit flags, quota) and a test that a fresh setup can hand a dashboard a working credential
   without hand-editing files.
3. **Where do dashboard connection tokens live?** 04 says "dashboard config (0600)", 08 defines
   per-instance manager config but not a dashboard-side store. Needs a path (`<config>/dashboard/
   connections.ron`?) and an explicit "values never printed" rule matching the manager token
   handling.
4. **Version skew.** 10 says the dashboard detects capabilities per manager; 09 says the module
   is version-matched to core **by default**. That means a dashboard can meet an older manager
   (no history? no view?) — the capability handshake the dashboard relies on should be pinned as
   "field presence + protocol version range", not guessed from the core version.
5. **Packaging dependency.** `dashboard install` verifies a checksum from the release manifest
   (09). Until packaging defines the module artifact name/rows, the dashboard slice cannot
   install anything end to end. This is the one hard ordering edge: packaging first, dashboard
   second.

## Recommendation

Keep v1 dashboard work to (1) the module-version source and (2) the scope-creation path — both
small, both manager-side-adjacent, neither making the manager dashboard-aware. The dashboard UI
itself is a separate client and can be built against the existing read surface without core
changes.
