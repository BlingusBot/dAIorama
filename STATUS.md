# dAIorama — project status

_One screen, kept current; the durable index for the bot-to-bot lane (threads
are read-when-on pointers)._

- **P2 — complete.** W1 contract layer + fixtures, W2 runtime (containment ·
  spawner · health · serve · renderer gate · real-chain run), W3 CLI, and six
  hardening rounds (lifecycle churn, drain, odd creation params, config races,
  frame/gate refusals, two-client concurrency) — **merged**; `spec/v1` @ `2d83625`.
- **P3 — W1 complete, merged.** `daiorama mcp` in both modes — stdio (framing,
  both handshakes, scope manifest, relay) and `--listen` (HTTP `POST /mcp`,
  per-request `Daiorama-Token` / `Daiorama-Scope` / `Daiorama-Scope-Token`
  gate, `Host`/`Origin` validation, bounded bodies, MCP-level refusals, a head
  that refuses proxy-disagreement shapes). Scope secrets pin to
  `<data>/<instance>/scope-tokens/<scope>` (0600). Merged as `spec/v1 @ 7c9d11c`; the strict-head parser, the transport limits (10 s
  per-read progress timeout, 64-connection cap, gate before body) and a total 15 s head
  window follow it (`spec/v1 @ d7fe685`).
- **Phase/branches:** P0 **frozen @ `4eeb585`** (paper: `audits/p0/`) · **P1 L1
  green** (`p1/` @ `eb35a4d`) · P2 through `2d83625`; P3 W1 merged
  (`p3/bridge/*` → `spec/v1 @ d7fe685`).
- **Repo target:** fork workflow (`EstherOS/dAIorama`); upstream
  `ygypt/dAIorama` takes our PRs post-sign-off.
- **Last green:** rate-limit tree — 217 (+5 ignored) unit · 7 daemon ·
  2 doctor · 27 fixtures · 7 mcp · 22 mcp_listen, fmt/clippy clean; B9 bucket,
  pace/reject bounds, per-session isolation and capture serialization pinned in
  unit + dispatch tests. Views live leg remains in `p4/views/evidence/`.
- **Next:** P4 (views front + network/auth lifecycle); the capture tools,
  `/media/<id>` relay and the manager store have all landed (`src/runtime/capture.rs`,
  `p3/media/`, `src/media/`).
- **Open (tracked, not omitted):** L3 llvmpipe lane (waits on the operator's
  workflow token scope) · L2 NVIDIA/CDI (operator's call) · rate-limit numbers
  → P4 · per-family GPU recipes · resize/scale path · renderer re-verification
  of the L2 image · a body trickled a byte at a time can still hold a
  connection slot (a total-body deadline was rejected for authentic slow
  uploads; measured as WATCH in `probe-head-deadline.txt`). Refusal closes and
  the media write bound are no longer open: every refusal site — the media
  surface included — rides one half-close + bounded linger, and a media client
  that accepts no bytes is cut off at the write progress bound
  (`probe-media-stall.txt`).
