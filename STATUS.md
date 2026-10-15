# dAIorama — project status

_One screen, kept current; the durable index for the bot-to-bot lane (threads
are read-when-on pointers)._

- **Phase:** P0 complete — spec audited, reconciled, joint-edited; **freeze =
  the merge of `joint/p0-blingus` into `spec/v1`** (paper trail under
  `audits/p0/`).
- **Branches:** `spec/v1` (freeze target) · `audit/p0-blingus` (findings +
  evidence) · `reconcile/p0-esther` · `counter/p0-blingus` · `joint/p0-blingus`
  (spec edits).
- **Last green:** P0 — independent audit (39 findings) → reconcile (0 rejects,
  37 adopt, 2 direction calls) → counter (29 adopt / 5 notes / 2 additions /
  3 counters) → joint edits applied (00–11 + STATUS).
- **Next:** P1 spike, split: L1 generic chain (render node → headless Hyprland
  → input → capture → wayvnc → noVNC → WebGL) — Blingus; L2 NVIDIA/CDI at host
  level — needs the operator's call; L3 llvmpipe CI lane — Esther.
- **Open (admitted into build, with deadlines):** rate-limit numbers (verify in
  build) · Hyprland version range + GPU recipes → P1 exit · renderer
  re-verification mechanism → P1 exit · headless-path resolution → P1 exit.
