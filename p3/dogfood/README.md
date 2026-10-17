# P3 dogfood — harness compatibility (the pinned entry point)

`daiorama mcp` is pinned as *the* command every harness points at (05 §The bridge). A harness
does not handcraft JSON-RPC: it hands the command to its MCP client SDK and lets the SDK
negotiate, validate every result and match responses by id. This drill exercises exactly that
path with the published `mcp` client (`mcp==2.3.0`, client `mode='auto'` — the harness default),
against a real manager and real compositor sessions in the privileged L1 container.

## Run

```
# inside the privileged L1 container, under dbus-run-session:
BIN=/out/hardening/daiorama-p3 bash p3/dogfood/run-sdk.sh
```

`run-sdk.sh` sets up a scope (`agent`: 2 sessions, own visibility, `input` grant), starts the
manager, runs the raw handshake probes (`sdk_client.py --raw`), then the SDK drill
(`sdk_client.py`), and finishes with the leak checks (compositor processes, session scopes,
runtime dirs). Both are self-checking: `[OK]`/`[FAIL]` per assertion and a final `FAILS=`.

The client SDK is a plain Python package (`python3 -m venv`, `pip install mcp==2.3.0`); mount its
`site-packages` at `/sdk/lib/python3.14/site-packages` and export that as `PYTHONPATH`.

## What it found (all fixed in this branch)

1. **`server/discover` answered the `initialize` shape.** A modern client's first call got
   `{protocolVersion, capabilities, serverInfo}` — no `supportedVersions` — so the SDK's
   `DiscoverResult` validation failed and it fell back to the handshake path.
2. **`initialize` answered a revision outside the handshake set.** The fallback got
   `protocolVersion: "2026-07-28"` and the SDK refuses to continue:
   `RuntimeError: Unsupported protocol version from the server: 2026-07-28`. Net effect: **no
   harness could connect at all.**
3. **Successful `tools/call` results omitted `content`.** The field is required on the MCP
   result; a client SDK rejects a result without it (`content` → Field required).
4. **The per-request revision gate read the wrong `_meta` key.** The 2026-07-28 declaration is
   `_meta["io.modelcontextprotocol/protocolVersion"]`; the bridge read `_meta.protocolVersion`,
   so it never saw a real modern request: the `-32022` refusal was dead for real clients and no
   modern result envelope was ever sent. `ListToolsResult` then failed validation on
   `ttlMs`/`cacheScope`/`resultType` (all required on the 2026-07-28 wire).

The fix: proper `DiscoverResult`/`InitializeResult` shapes, `content` always on the wire, the
namespaced `_meta` key honoured (un-namespaced kept for older callers), and the modern result
envelope (`resultType`, plus `ttlMs`/`cacheScope` on the cacheable list result) stamped only on
requests that declare the modern revision — a handshake-era caller keeps the pre-2026 shape.

## Evidence

- `evidence/sdk-before.txt` — the drill against the pre-fix tip: the SDK cannot connect at all
  (`RuntimeError: Unsupported protocol version from the server: 2026-07-28`); the raw probes show
  why: `server/discover` answers the `initialize` shape (no `supportedVersions`), and `initialize`
  answers a revision outside the handshake set.
- `evidence/sdk-after.txt` — the same drill green on the fixed tip, including a real session
  created, listed, statused, refused on a deny-by-default tool, closed — plus leak checks.
