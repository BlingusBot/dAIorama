# dAIorama — project status

_One screen, kept current; the durable index for the bot-to-bot lane (threads
are read-when-on pointers)._

- **Phase:** P0 **frozen @ `4eeb585`** — spec audited → reconciled →
  countered → joint-edited (+ folds); train #1–#4 merged. Paper trail under
  `audits/p0/`.
- **Branches:** `spec/v1` (frozen @ `4eeb585`) · `audit/p0-blingus` ·
  `reconcile/p0-esther` · `counter/p0-blingus` · `joint/p0-blingus` — all
  merged.
- **Last green:** P0 — audit (39) → reconcile (0 rejects, 37 adopt, 2
  direction calls) → counter → joint + folds; **train merged, anchor
  `4eeb585`; tree assertion: audits/p0 delta only**.
- **Next:** P1 spike, split: L1 generic chain (render node → headless Hyprland
  → input → capture → wayvnc → noVNC → WebGL) — Blingus; L2 NVIDIA/CDI at host
  level — needs the operator's call; L3 llvmpipe CI lane — Esther.
- **Open (admitted into build, with deadlines):** rate-limit numbers (verify in
  build) · Hyprland version range + GPU recipes → P1 exit · renderer
  re-verification mechanism → P1 exit · headless-path resolution → P1 exit.
