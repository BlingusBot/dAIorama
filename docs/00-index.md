# dAIorama — spec index

The spec is the anchor: nothing gets coded before it freezes. Files 01–10 are
normative on behavior (names, flags, defaults, contracts). Decision rationale
and rejected alternatives live in 11; raw inputs live in 90.

Status: draft v1 — **P0 audit + reconcile complete (2026-10-15); freeze
pending**. This set integrates the 2026-10-05 audit (8 blockers / 24 majors /
minors — every item homed in 11) and the 2026-10-15 independent pass (39
findings; dispositions in `audits/p0/`; edits applied). The freeze commit is
the first frozen anchor and the context for everything after.

## Files

| # | file | one-liner | audit homes |
|---|------|-----------|-------------|
| 00 | 00-index.md | this map | — |
| 01 | 01-overview.md | what/why, goals, non-goals, principles | — |
| 02 | 02-glossary.md | nouns pinned once; plain-vs-theme rule | nits |
| 03 | 03-architecture.md | components, seams, backend modes, GPU access | B6, B10 |
| 04 | 04-cli.md | command tree, flags, defaults, refusals | A1, D1, D2 |
| 05 | 05-mcp-bridge.md | bridge entry, tool surface, errors, versioning | A6, A7, A8 |
| 06 | 06-sessions-views.md | session + view lifecycle, reaper, death rules | B1–B4, D6, C6 |
| 07 | 07-network-auth.md | exposure rule, tokens, scopes, interop | A3, A4, A5, B7, B9 |
| 08 | 08-config.md | per-key authority, files, reload, scopes | A2, D4, D5 |
| 09 | 09-packaging.md | releases, manifest, dashboard install, image | C1, C2, C5, C7 |
| 10 | 10-operations.md | operator runbook: start/stop, drain, upgrade | B5, C3, C4, D3 |
| 11 | 11-decisions.md | ledger: audit item → decision + rationale + rejected | all |
| 90 | 90-appendix/ | original design doc, audit, outline — historical | — |

## Rules

- One topic per file; numbers are reading order; cross-references by number.
- Normative statements in 01–10; rationale and rejected alternatives in 11;
  original sources in 90 (never edited — history only).
- Operator-visible strings (errors, statuses, refusals) are plain and stable;
  they get grepped, so wording is part of the contract. Theme nouns only in
  dashboard chrome (see 02).
- "The manager" means the instance a command targets; `--remote` picks which
  instance (07, 08). Default instance name: `default`.

## Process

draft → multi-lens audit → reconcile → sign-off → freeze (anchor commit).
This set is the reconciled draft. Sign-off walks 11-decisions; edits land here
or in the numbered files; the freeze commit is the first spec commit and the
context for everything after.
