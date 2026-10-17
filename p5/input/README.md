# Input surface — pre-read (P5)

Status: **unbuilt.** The nine control tools are pinned in 05 and classified for rate limiting in
07 B9, but `src/runtime/service.rs` routes them to `not_implemented`; `Cargo.toml` has no Wayland
client crate. The shipped binary cannot click or type, so the observe-then-act loop the capture
tools were built for has no act half. This note settles the mechanism before code.

## Surface (pinned, 05 — not reopened)

| method | params | semantics |
|---|---|---|
| `pointer_move` | `{x, y}` | absolute, session output pixels — the same space as `screenshot` output |
| `pointer_click` | `{button: left\|right\|middle}` | down+up at the current pointer position |
| `pointer_button_down` / `_up` | `{button}` | the drag primitive: down → move → up |
| `pointer_scroll` | `{dx, dy}` | wheel lines; +x right, +y down |
| `key_tap` | `{key}` | xkb keysym name; tap = down+up |
| `key_down` / `key_up` | `{key}` | chords are modifier down/up around a tap |
| `type_text` | `{text}` | literal text, as a user would type it |

Control class: all nine are rate-limited per (scope, session) bucket; audit records `tool.call`
without arguments (06). Input does not require an open view — `get_view` is observation, not
authorization, and a headless agent clicks the same session it screenshots.

## Option A — in-binary Wayland client (recommended)

Verified in the L1 image (`strings /usr/bin/Hyprland`): both managers are compiled in — `zwlr_virtual_pointer_manager_v1` and `zwp_virtual_keyboard_manager_v1` — so the compositor side exists today; nothing in this option depends on a newer Hyprland.

One `wayland-client` connection **per call**, opened against the session's own `wayland-<n>`
socket in its runtime dir — stateless, so nothing to pool, drop or leak across sessions. No child
process per event, so no per-keystroke fork and no argv injection surface; the rate limit bounds
the connect cost. (A per-session pooled connection is a later optimization, not a v1 need.)

- **Pointer:** `zwlr_virtual_pointer_manager_v1` → `motion_absolute(time, x, y, x_extent,
  y_extent)` for `pointer_move`; `button`; `axis` (wheel lines); `frame` to commit a batch.
  `pointer_move` is in **screenshot pixels**; the compositor's absolute space is logical, so the
  call converts by the session scale (`x_logical = x / scale`) and passes logical extents. Both
  resolution and scale come from the session record; a resolution change mid-session is out of
  scope for v1 (documented, and `screenshot` remains the truth for coordinates).
- **Keyboard:** `zwp_virtual_keyboard_manager_v1` → upload a **built-in US keymap** (the v1
  contract: one fixed layout, stated), then send evdev keycodes with `key(state, keycode, time)`.
  `key_down`/`key_up` are direct; `key_tap` is down+up; `type_text` maps each character through
  the built-in table, applying shift where the character sits on the shifted level, and taps it.
- **Keys:** `key_tap`/`key_down`/`key_up` accept xkb keysym names (`Return`, `a`, `Shift_L`,
  `F5`) resolved against the built-in US table; an unknown name or a character with no US keymap
  entry is `bad_request` naming the key/offset, never a silent no-op. No `libxkbcommon` at
  runtime; the table is data in the binary.
- **Capability detection:** if the compositor lacks either protocol, the input methods answer
  the canonical `internal` code with a fixed message naming the missing interface — no silent
  drop, no new error code (05's set is closed).

Dependencies, pinned exactly: `wayland-client` (pure-Rust backend, no `use_system_lib`),
`wayland-protocols` (client + unstable), `wayland-protocols-wlr` (client); protocol codegen with
`wayland-scanner` at build time only. No libwayland C dependency at link time — the static-binary
posture stays.

## Option B — argv-configured input commands (rejected)

Mirroring the capture erratum (`input.*_command` defaults, operator override) looked cheap, but
it cannot express the pinned surface: `wtype` has no pointer; `wlrctl pointer move` is relative
and cannot place the cursor at absolute session pixels; `ydotool` would need `/dev/uinput` and
acts at the host input layer, breaking session isolation and the container's device policy. The
desktop image also deliberately carries neither `wtype` nor `wlrctl`. B buys a subprocess per
event and still fails `pointer_move`'s contract — rejected on the contract, not on taste.

## Acceptance (once ruled)

1. Session in the L1 image; `hyprctl cursorpos` is the oracle for `pointer_move` (absolute pixel
   target), and a session app with a click target proves down/up and drag.
2. `wev` (test-only app) proves keysym names and modifiers; `type_text` types a known string and
   the app's log is read back.
3. Missing-protocol leg: a compositor without virtual input answers the pinned refusal, session
   still screenshots.
4. Rate leg: the tenth rapid `pointer_move` in a tight bucket answers `rate_exceeded` (`pace`
   mode), and audit carries `tool.call` with no arguments.
5. Geometry leg: sessions at 1280×720 and 1920×1080 both land the pointer on the target pixel
   (extents, not a hardcoded size), plus one **scaled** session (`scale = 2`) where a screenshot
   pixel maps to a logical coordinate.

## Open decisions

1. **A vs B** — A recommended; B rejected on `pointer_move` as above.
2. **Missing-protocol message** — the exact fixed string (code is `internal`, pinned).
3. **Geometry source** — compositor output at session creation; resize during a session is out of
   scope in v1 and stated.
4. **Keysym breadth** — US keymap in v1 (letters, digits, punctuation, named keys, modifiers);
   other layouts are a later slice and would be a config or per-session keymap, not silent
   layout detection.
