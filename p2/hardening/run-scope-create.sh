#!/bin/bash
# D2 live leg (P5 scope create): a scope written by `config scope create` becomes
# network-usable at the next `mcp --listen` bind — proven over a real manager, a real bridge
# and real TCP calls. Covers the create templates/refusals, the first-bind credential mint,
# a refused wrong/missing secret, and the second bind that adopts a scope created while the
# bridge was up (existing credentials unchanged).
#
# Usage: run-scope-create.sh <binary> <label> [port]
set -u
BIN=$(readlink -f "$1")
LABEL=${2:-run}
PORT=${3:-18770}
echo "binary=$BIN label=$LABEL port=$PORT"
python3 "$(dirname "$0")/probe-scope-create.py" --bin "$BIN" --port "$PORT"
RC=$?
echo "scope_create_rc=$RC"
if [ "$RC" -eq 0 ]; then
    echo "VERDICT=PASS"
else
    echo "VERDICT=FAIL"
fi
exit "$RC"
