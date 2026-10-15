# dAIorama — spec working outline

Status: WIP. The spec is the anchor commit; nothing gets coded before it freezes.
Sources: source design doc (re-stage needed) + `design-audit.md` (8 blockers / 24 majors / minors).
Process: draft → multi-lens audit panel (systems / UX / ML) → reconcile → sign-off → freeze.

## 0. Scope & framing
- scope/identity contract (A4)
- per-key config authority (A2)

## 1. Contracts (machine-usable)
- network surfaces incl. view-port gate + enforcer (A3)
- error/denial contract (A6)
- version handshake + rejection policy (A7)
- session token lifecycle + rotation semantics (A5)
- capture/clip resource links vs ephemeral capture (A8)

## 2. Session lifecycle & seams
- idle reaper "activity" definition (B1)
- view-state registry + port allocation (B2)
- hang vs crash health action (B3)
- process-death guarantees, stated precisely (B4)
- lazy-start semantics + mode determinism (B5)
- host backend scope + exclusivity granularity (B6)
- interop semantics (B7)
- history/dashboard scoping (B8)
- rate limiting placement (B9)
- manager→dashboard reporting seam (B10)

## 3. Packaging & release
- dashboard install mechanism (C1)
- release manifest (C2)
- upgrade procedure (C3)
- GPU detection without a configured container (C4)
- bare-host requirements + capture decision (C5)
- cgroups v2 containment: preconditions + fallback (C6)
- testing seams = spike exit criteria (C7)
- filesystem contract table (C8)

## 4. Ergonomics & flows
- first-run + refusal UX (D1)
- start-mode visibility (D2)
- dashboard operator flows (D3)
- scope ergonomics: readable, scriptable, validated (D4)
- pin-history defaults + RON rationale (D5)
- llvmpipe fork decision (D6)

## 5. Decisions ledger
- every A/B/C/D item gets: decision + rationale + rejected alternative
- carry forward the audit's "what holds" list as settled context

## Inputs (staged)
- `daiorama-design.md` — source design doc (design channel attachment, 2026-10-05)
- `design-audit.md` — 4-lens audit (8 blockers / 24 majors / minors)
- this outline
