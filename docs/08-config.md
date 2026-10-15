# 08 — Config, files, and paths

## Filesystem contract (C8 — exact paths, per instance)

Per-instance discrimination is what lets several managers coexist on one host
without colliding token/history paths. `--instance <name>` selects; default
`default`.

| kind | path | notes |
|------|------|-------|
| core config | `${XDG_CONFIG_HOME:-~/.config}/daiorama/<instance>/config.ron` | dot-path keys, one nesting level max |
| scope defs | `${XDG_CONFIG_HOME:-~/.config}/daiorama/<instance>/scopes/<scope>.ron` | one file per scope, filename = scope name |
| data root | `${XDG_DATA_HOME:-~/.local/share}/daiorama/<instance>/` | persistent; survives restarts |
| token | `<data root>/token` | mode 0600, own file (07) |
| history | `<data root>/history/` | per-session segments (below) |
| modules | `<data root>/modules/` | dashboard module installs (09) |
| runtime / socket | `${XDG_RUNTIME_DIR}/daiorama/<instance>/manager.sock` | fallback below when `XDG_RUNTIME_DIR` is unset/unusable |
| container equivalents | `/config`, `/data`, `/run/daiorama` | compose mounts reference these (09) |

**Runtime-dir fix, single-sourced:** manager startup checks whether
`$XDG_RUNTIME_DIR` is set and usable; if not, it creates a fallback equivalent
(same permission model) before anything else, and compositor children inherit
it because the manager forks them. One check, one place, bare-metal and
container alike. **The image entrypoint does not pre-create it separately** —
the in-binary ensure is the only code path, so the shipped image exercises the
same one (previous belt-and-suspenders duplication resolved by deletion, not
by adding a second path).

## Per-key authority (A2 — the ambiguity closed)

Every config key has exactly one authority. `config get/set` respects it:

| scope of key | authority | how it is read/written |
|--------------|-----------|------------------------|
| `remote.default`, `manager.start_mode` (client behavior) | client-local | the local file, direct; `--remote` on a client-local key is an error — these keys do not exist on a remote |
| session defaults, `view.*`, `reaper.*`, `history.*`, `media.*`, `limits.*`, `manager.bind`, `sessions.visibility` | manager-side | this instance's files, direct locally; RPC when `--remote` |
| scope quotas/manifests/audit flags | manager-side, files | `scopes/*.ron`, edited in place (below) |
| dashboard connections/tokens | dashboard-local | dashboard config file, 0600 |

So `remote.default` works as designed (client routing value, read before any
manager is located — the old "config set remote.default" contradiction is gone),
and `config set` is uniformly RPC **only** when `--remote` is given. `config
edit` is local-only; remote is explicitly refused (04).

**Reload semantics, explicit:**

- **Core config: restart-only except where reloadable.** `config reload` (or
  manager restart) re-reads the file. Listener bind addresses and the token
  are restart-only (stated in `config reload` output when it encounters them
  dirty); everything else re-reads cleanly. There is no file-watcher magic.
- **Scope files: read per call.** Fresh read on each call that consults them
  (caching allowed, invalidated on mtime change), validated at load. Editing a
  scope affects the next call — deliberate, because scopes are the
  hand-edited-by-humans files.

## Schema rules

- Dot-path, **one nesting level max** — kept shallow so `get`/`set` stay
  simple; list-tolerant things (scopes) live in their own file form instead
  (this is the designed split, not a drift).
- A `config.version` field is checked at startup with a clear migration path
  if ever bumped.
- Format: RON for both core config and scope files. Rationale recorded in 11,
  including the rejected TOML alternative and why the strict-validation fix
  (below) answers the hand-edit concern.

## Defaults

Single master table in 04 (ports, resolution, reaper off, history 8×4 MiB /
7-day retention, media TTL, token path). Not duplicated here — 04 is where a
"what is the default" question is answered, and only once.

## History storage (mechanics)

- Plain text, line-per-entry, append-only. No database, no structured binary —
  format was never the wear risk.
- Buffered writer, flushed on a short interval (or small batch); **no fsync per
  call** — losing seconds of buffered entries in a hard crash is acceptable
  (operational aid, not the source of truth for session state).
- Rotation: bounded by segments × cap (defaults in 04), oldest dropped;
  retention window after session close (delete-now default? no — 7 days, 04),
  then removed. Unbounded retention was the actual risk; this bounds it.
- Records carry scope + origin (06). Not tamper-evident and not claimed to be
  (07 posture).

## Scope files and validation (D4)

- Scopes are readable, scriptable, reviewable: `config scope list`,
  `config scope get <name>`, `config scope validate <name>`,
  `config effective [--scope]` (04).
- **Startup validation + on-demand validation.** Unknown tool or category
  names are load errors listing the valid names — a typo must never silently
  shrink a manifest (deny-by-default turns silent shrinkage into mystery
  capability loss, which the design forbids).
- `config setup` emits commented starter files for config and a first scope —
  first-run produces something valid and editable, not an empty page.
- `get`/`set` deliberately do not address scopes: they are the one list-tolerant
  shape, edited as files. That exclusion is stated in `config --help` so nobody
  guesses (D4).
