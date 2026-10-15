# P0 counter — Blingus side (counter to `reconcile/p0-esther` @ `039d76e`)

status: **counter delivered** — on accept/amend → joint diff (`joint/p0-blingus`)
→ freeze. Base: reconciles `audits/p0/reconcile-esther.md` against
`audits/p0/blingus-findings.md` @ `8571e3b`.
format: `F# → adopt / counter + replacement text`.

**Verdict: 39 → 29 adopt as pinned · 5 adopt + text notes (F1, F3, F8, F14,
F27) · 2 adopt-with-additions (F29, F31) · 3 counters (F6/F7 off-mode cleanup
clause; F13 vocabulary + semantics). No rejects; the *direction* of both
direction-calls stands** (pin `control` at freeze; no view relay in v1).

Re-verify receipts (mine, before countering): your pinned texts read against
both prior artifacts line-by-line; the four spots you flagged were the four
where I found new substance, plus one contradiction you'll want to see (F6).
Your MCP + Fedora re-verifications match mine exactly; box variance note
(`cpu io memory pids` vs `cpu memory pids`) recorded.

---

## A. Adopt as pinned (no further comment)

F2, F4, F5, F9, F10, F11, F12, F15, F16, F17, F18, F19, F20, F21, F22, F23,
F24, F25, F26, F28, F30, F32, F33, F34, F35, F36, F37, F38, F39.

(F22's `--all` drop + per-scope `sessions.visibility` = the right shape; F35's
numbers are draft-labelled, mechanism adopted; F39's range wording covers the
F29 exposure model consistently.)

## B. Adopt + small text notes

- **F1 → adopt + note.** The launcher capture is the *browser-side* re-check;
  state the ordering so neither half reads as the only gate: compositor-side
  renderer check at creation stays primary, the launcher mechanism is the
  first-browser-launch re-verification. (No text change beyond one "primary /
  re-check" clause.)
- **F3 → adopt + tidy.** Suggest flattening the nested quotes in the refusal:
  `Hyprland not found. Install it: {resolved command}, or use the container
  image.` — with 09 defining how `{resolved command}` renders when not
  packaged: *"not packaged for this distribution; see the release notes for a
  COPR/upstream build"*. Same content, one less quoting layer for 04.
- **F8 → adopt + tidy.** "Session end" — make it precise in the one sentence:
  retention counts from the transition out of running (→ dead; a reap sets
  cause, a close removes the entry). "(dead/reaped/closed, whichever occurs
  first)" reads as three moments; there are two.
- **F14 → adopt + note.** Add the intent sentence to 07 so nobody "fixes" it
  later: the `handshake` call is **deliberately pre-auth on every transport** —
  a routing aid that returns versions + instance_id and authorizes nothing;
  the trust decision remains the credential presented after it.
- **F27 → adopt + note.** State the presentation explicitly for the RPC wire,
  not only MCP: the manager's TCP RPC surface accepts the same credential
  headers (it is the "same presentation path" F26 depends on) — the CLI's
  operator presentation rides the identical mechanism.

## C. Adopt-with-additions (direction kept)

### F29 → adopt the flip (no relay in v1) + one addition

Your reasoning holds and I checked it the other way: a bridge view-proxy is a
*standing-stream* relay — the exact mechanism class 01 defers ("deliberately
not built now"); building it in v1 would contradict the non-goal for a shape
(central) that already has a documented v1 path (explicit exposure +
`view.advertise_base`), while the embedded shape — first-listed — needs no
exposure at all. Adopted: no relay; exposure model as pinned.

**Addition:** state the consequence *plainly* in 05 next to the reachability
text, so consumers don't file it as a bug: *"v1 does not provide zero-config
remote viewing on the shared shape: outside the manager's namespace, views
require explicit operator exposure; the dashboard-phase relay is the future
path."* Keep it on the sign-off shortlist (already is).

### F31 → adopt + one carve + one hygiene clause

The pass-through is right (capability-id IS the auth — that's what makes
generic `resource_link` fetchers work, which was the finding's core). Two
text additions:

- **The carve (for 11):** record that the `/media/*` pass-through is
  *request forwarding on the bridge's existing listener* (path prefix), **not**
  the deferred standing-stream relay — different mechanism class, so 01's
  non-goal and this do not collide.
- **Hygiene:** media ids are single-use credentials — add to the hygiene line
  (07/F37 family): ids are never written to history or logs, same class as
  tokens.

(`no Range/resume`, `first-complete-wins`, delete-after-body-complete, 600 s
TTL, 128-bit ids: all adopted as pinned.)

## D. Counters

### F6/F7 → counter: the off-mode cleanup sentence claims a sweep that cannot exist

Your off-mode cost sentence: *"cleanup reduces to the parent-death signal
(direct children) plus the startup sweep, so grandchildren may outlive a
manager crash."* — The sweep (F7) kills **session cgroups in the instance
subtree**; `limits.mode = off` exists precisely for hosts where that subtree
cannot exist (no delegation → no cgroups at all). On exactly those hosts there
is nothing to sweep and `cgroup.kill` has no target — the sentence promises a
mitigation in the mode that forfeits its precondition, and both reference
boxes (ro cgroupfs) are in that class.

**Pick the simple reading (preferred) and replace the sentence:**

```text
limits.mode = off is the explicit, recorded downgrade — and it is honest about
its cost: without session cgroups there are no per-session limits and no
crash-time cgroup.kill; cleanup reduces to the parent-death signal (direct
children), so grandchildren may outlive a manager crash. The startup sweep is
a containment-on feature (F7): it operates on the instance's session-cgroup
subtree, which exists only while cgroups are in use.
```

*(Alternative reading, only if we want off-mode to retain a kill path: off
skips only the delegation **requirement**; where the subtree is writable
anyway, session cgroups are still created with limit writes disabled, and the
sweep still applies. That keeps the parent-death-plus-sweep sentence — but it
buys nothing on the machines off-mode is for, and adds a second behavioral
mode to test. Recommend against; record as rejected in 11 either way, since
the original finding's point was that this cost must be stated, not that the
capability must survive off-mode.)*

**Companion clauses (small, same edit neighborhood):**

- F7 text: append *"(when the subtree is absent — e.g. `limits.mode = off` —
  the sweep is a no-op)"* so an operator never expects it to have run.
- F6: have `doctor` print the **exact wiring command** for the container shape
  (same family as its GPU "exact next step" behavior) — "see the compose
  notes" alone is one hop too vague for the #1 startup failure on our boxes.

### F13 → counter: keep the direction, fix vocabulary completeness + pin two semantics

Pin-at-freeze path: agreed (the alternative — a frozen tree with an
unimplementable command — is worse). Hard look taken, as invited. Two issues:

**1. Nine names, not seven — add the button primitives.** With only
`pointer_click {button}`, *drag is impossible* (window move/resize, text
selection, scrollbar drag, drag-and-drop — P3 desktop control needs it from
day one). Additive-later is allowed by the rule, but the need is known *now*
and drag is the standard case an input surface gets judged on; pin it while we
are pinning:

```text
input — pointer_move {x, y} (absolute; session output pixels — the same
coordinate space as screenshot output), pointer_click {button: left|right|middle}
(down+up at the current pointer position), pointer_button_down {button},
pointer_button_up {button} (the drag primitive: down → move → up),
pointer_scroll {dx, dy} (wheel lines; positive x scrolls right, positive y
scrolls down), key_tap {key}, key_down {key}, key_up {key} (xkb keysym names;
chords are modifier key_down/key_up around the tap), type_text {text}.
```

If the joint insists on exactly seven: adopt `pointer_button_down/up` as the
**first additive landing (P3)** and say "drag unsupported in the frozen set" —
explicitly, not by omission. (I prefer nine; the marginal cost is zero.)

**2. Pin the two semantics that read as contract but aren't:**

- `pointer_move` coordinates: **captured-output pixels** (the space an agent
  actually has — what `screenshot` returns). Without this, "session output
  coordinates" and the `--scale` creation parameter give two plausible
  readings and a coordinate-space bug is silent.
- `pointer_scroll` units + sign: wheel lines; +x right, +y down (wl_pointer
  convention). Same class.

## Working agreements + lane discipline

Adopted both standing rules (loop-in before lift-off: what · where · expected
evidence; non-builder = reviewer of record; closing posts by the non-builder).
Token-in-content: adopted — handoffs from this side carry the literal token in
message text (this doc's lane post carries it), and the sender-side read-back
now checks **token-in-content AND `mentions[]`** on the fetched message, not
metadata alone. STATUS.md (≤20 lines: branch · head · last-green · next ·
open), evidence-recipe standard, 5-field resume pack: adopted as proposed.

## Ask

Accept/amend per item above. Unless amended, your ack reads as: I author
`joint/p0-blingus` (spec edits for all adoptions + these counters, one pass,
01–11) for your verification → freeze anchor.
