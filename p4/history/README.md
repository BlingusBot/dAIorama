# P4-HISTORY — implementation note

Scope: the record format and the read surface (06 B8); retention stays a separate owner.

## Pinned here

- **Layout:** `<data>/history/<session>/seg-NNNN.log`, JSONL, one record per line; 0600 files
  in 0700 dirs. Rotation: 8 segments × 4 MiB, oldest dropped — rotation is this slice's,
  retention is not.
- **Record:** `{at, event, session_id, scope, origin, cause?, detail?}`. `at` is RFC 3339 UTC;
  events today: `session.created`, `session.closed`, `view.started`, `view.stopped`,
  `capture.screenshot`, `capture.clip`, `tool.call`. Control-call **arguments are never
  recorded**; neither are media ids, tokens, or credentials.
- **Durability boundary:** writes are buffered with a flush per record to the OS and **no
  fsync** (08). A hard crash may lose seconds of entries — acceptable for an operational aid.
- **Session end:** `close`/death writes `<session>/.ended` (`<ts>\tscope=…\torigin=…\tcause=…`,
  0600). The retention sweep removes whole session dirs (history + media) seven days later and
  never infers "ended when" from mtimes.
- **Reads:** `list_history {session_id?, scope?, limit?, cursor?}` →
  `{records, next_cursor}`. Visibility is the session rule: own scope by default, operator
  sees all, a stranger's id is `session_not_found`. `limit` 1..=1000 (default 100). Cursors are
  opaque (`base64("seg:line")`), session-scoped, and a cursor without `session_id` is
  `bad_request`. Cross-session listing returns the newest records with no cursor.
- **Restart:** history outlives the manager; when the live registry no longer has the session,
  visibility falls back to the scope recorded in the log, so the operator (and the owning
  scope) can still read it.

## Tests

Record round-trip + 0600, newest-first ordering, cursor pages without gaps or duplication,
rotation at the segment budget, durable `.ended` marker, recorded-scope survival across a
reopened manager, dispatch-level create/close/view records, visibility (stranger →
`session_not_found`), cross-session cursor refusal, and "arguments are not recorded".
