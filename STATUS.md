# dAIorama — project status

_One screen, kept current; the durable index for the bot-to-bot lane (threads
are read-when-on pointers)._

- **Phase:** P0 **frozen @ `4eeb585`** (paper: `audits/p0/`; train #1–#4
  merged) · **P1 L1 green** — nested-headless chain verified on the reference
  box (`p1/`, merged @ `eb35a4d`).
- **Branches:** `spec/v1` (P0 freeze `4eeb585` · addendum `d881196` · L1
  `eb35a4d`) · `audit/p0-blingus` … `joint/p0-blingus` — all merged.
- **Last green:** P0 train → anchor `4eeb585` · **P1 L1** — chain green on
  the reference box: runs 2–4 `exit=0`; keyboard + pointer input, capture,
  wayvnc → noVNC, non-llvmpipe WebGL.
- **Next:** P1 — L3 llvmpipe CI lane (Esther) · resize/scale path
  verification · L2 NVIDIA/CDI (operator's call).
- **Open (admitted into build, with deadlines):** rate-limit numbers (verify in
  build) · Hyprland version range + GPU recipes → P1 exit · renderer
  re-verification mechanism → P1 exit · resize/scale path → P1 exit ·
  headless-path resolution — resolved (03/09/11).
