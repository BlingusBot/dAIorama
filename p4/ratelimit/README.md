# P4-RATELIMIT — design note (runtime side)

Scope: the B9 rate limit as pinned in 07 — a **per-session token bucket over control-class
calls** (input class; reads exempt), configured per scope, `pace` by default. This note pins
the implementation the spec left open and the numbers the build verifies.

## Where it counts

- The bucket is keyed by **session id** and lives in the manager (`RuntimeService`), per 07.
- Only **control-class** calls consume tokens: the nine input tools (`pointer_move`,
  `pointer_scroll`, `pointer_click`, `pointer_button_down|up`, `key_tap`, `key_down`, `key_up`,
  `type_text`). Reads (`status`, `list_*`, `session_status`, `list_capabilities`,
  `screenshot`, `clip_video`, `get_view`, `history`) are exempt — capture is observation, not
  control, and a rate-limited read would punish exactly the observe-then-act loop.
- `create_session`/`close_session`/`drain` are lifecycle, not control: exempt (the quota and
  drain rules already bound them).
- A call for an **unknown/closed session** fails its normal gate first — the bucket never
  invents a session.

## The bucket

Per session: `per_sec` tokens/second refill, `burst` capacity, continuous refill (fractional
tokens carried), monotonic clock (`Instant`). Defaults from 07: `per_sec = 20`,
`burst = 40`. Configured per scope in the scope file:

```ron
limits: (
  rate: (per_sec: 20, burst: 40, mode: pace),
)
```

Absent block = the defaults; validation: `per_sec > 0`, `burst >= 1`. A burst below one drag's
call count (3–4 calls) would reject normal motion, so the default 40 leaves that room and a
scope may lower it only deliberately.

## Modes (07, verbatim)

- **`pace` (default).** A call that arrives with no token waits until one exists, in arrival
  order, then proceeds. Because dispatch now runs on the blocking pool (the dispatch-stall
  fix), the wait costs a worker, not the reactor. The queue is **bounded**: a call whose
  computed wait exceeds `max_wait_ms` (default 30 000) is not parked — it fails
  `rate_limited` with `retry_after_ms`, so a flood cannot grow an unbounded backlog of parked
  workers. Arrival order is per session (a per-session lock around bucket + queue position).
- **`reject`.** No token → immediate `rate_limited` with `hint: retry_after_ms` (the time to
  the next token; `burst` exhausted at `per_sec`).

`rate_limited` envelopes carry `scope` and `session_id` in the message/hint as 07 pins; the
existing `ErrorEnvelope::rate_limited(scope)` gains the retry hint.

## Testing

1. **Bucket unit tests** (paused clock): burst cap, continuous refill, fractional carry,
   `per_sec` override from the scope file, defaults when the block is absent.
2. **Dispatch tests**: 41 input calls at once under defaults → the 41st waits ≈50 ms in
   `pace`; under `reject` it fails `rate_limited` with `retry_after_ms` ≥ 1; a read at any point
   is untouched; two sessions do not share a bucket; a call to another scope's session hits
   `session_not_found` before any bucket math.
3. **Pace bound**: a synthetic `per_sec = 1, burst = 1` with six queued calls at
   `max_wait_ms = 250` → the fifth/sixth fail `rate_limited`, the first four complete in order.
4. **Live leg** (no compositor needed for the bucket, but the input tools need a session to
   exercise the real path): a real session + 60 `pointer_move` calls through the bridge at
   defaults — wall time ≥ (60−40)/20 s ≈ 1 s, all succeed in `pace`; the same with `reject`
   returns `rate_limited` for the overflow with a hint. Raw committed.

## Open questions

- **Q1** `max_wait_ms` as a scope-level knob (`limits.rate.max_wait_ms`, default 30 000) — or a
  fixed constant? Recommend the knob; the right bound depends on the caller's harness.
- **Q2** Should `screenshot`/`clip_video` really stay exempt? I recommend yes (observation,
  and capture has its own `capture.timeout_s` cost control); confirm.
- **Q3** Config home: `limits.rate` in the **scope** file (as 07 says "per scope") — or also a
  core-config default the scope can override? Recommend scope-only, defaults from the build
  (one source of numbers).
