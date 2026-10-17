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

One lazily-opened `wayland-client` connection per session (opened on first input call, closed on
session end), bound to the session's own `WAYLAND_DISPLAY` / `XDG_RUNTIME_DIR` — the same env the
manager already owns. No child process per event, so no per-keystroke fork and no argv injection
surface.

- **Pointer:** `zwlr_virtual_pointer_manager_v1` → `motion_absolute(time, x, y, x_extent,
  y_extent)` for `pointer_move`; `button`; `axis` (wheel lines); `frame` to commit a batch.
  Extents are the session output size (the screenshot space), read from the compositor at session
  creation; a resolution change mid-session is out of scope for v1 (documented, and `screenshot`
  remains the truth for coordinates).
- **Keyboard:** `zwp_virtual_keyboard_manager_v1` → upload the compositor's keymap once, then send
  evdev keycodes with `key(state, keycode, time)`. `key_down`/`key_up` are direct; `key_tap` is
  down+up; `type_text` maps each character to an xkb keysym (`xkb_utf32_to_keysym`), applies
  level/shift modifiers where the keysym sits on a shifted level, and taps it.
- **Keys:** `key_tap`/`key_down`/`key_up` accept xkb keysym names (`Return`, `a`, `Shift_L`,
  `F5`) and resolve through the session keymap; an unknown name is `bad_request` naming the key,
  never a silent no-op.
- **Limits:** `type_text` is Latin-capable text, not an IME: characters with no keysym mapping
  are refused with `bad_request` naming the offset; no clipboard, no compose/IME, no dead keys in
  v1. Documented in 05 when this lands.
- **Capability detection:** if the compositor lacks either protocol, `session` creation records
  an input capability flag and the input methods answer a pinned refusal — **not** a crash and
  **not** a silent drop. The code and exact string are an open decision below (05's canonical
  error set is closed; I would reuse `internal` with a fixed message rather than add a code).

Dependencies: `wayland-client` + `xkbcommon` (Rust crates), build-time protocol generation from
vendored XML (`wayland-scanner`). Static-binary posture: `wayland-sys` can link or `dlopen`;
whether the release keeps the static claim with a dynamic libwayland is an open item, not a
blocker for the first landing.

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
5. Geometry leg: a session at 1280×720 and one at 1920×1080 both land the pointer on the target
   pixel (extents, not a hardcoded size).

## Open decisions

1. **A vs B** — A recommended; B rejected on `pointer_move` as above.
2. **Missing-protocol error shape** — reuse `internal` with a fixed message (my preference) vs a
   new canonical code (05 change).
3. **Static-link posture** — dynamic libwayland vs `dlopen`; decide when the dependency lands,
   record in 11 if the static claim changes.
4. **Geometry source** — compositor output at session creation; resize during a session is out of
   scope in v1 and stated.
5. **Keysym breadth** — Latin-1 + named keysyms in v1; full Unicode text is a later slice.
