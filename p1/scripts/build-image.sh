#!/usr/bin/env bash
# build-l1.sh — build localhost/blingus/dai-l1:dev via the host podman REST socket
# (there is no podman CLI inside the agent container). Logs to p1-out/l1-build.log.
set -euo pipefail
ROOT="/home/hermes/services/dAIorama"
CTX="$ROOT/p1/l1"
LOG="$ROOT/p1-out/l1-build.log"
cd "$CTX"
echo "build start $(date -u +%FT%TZ)" > "$LOG"
tar -cf - . | curl -sS --unix-socket /run/user/10000/podman/podman.sock \
  -X POST -H 'Content-Type: application/x-tar' --data-binary @- \
  'http://d/v4.0.0/libpod/build?t=localhost%2Fblingus%2Fdai-l1%3Adev&dockerfile=Containerfile' >> "$LOG" 2>&1 || echo "curl rc=$?" >> "$LOG"
echo "build done $(date -u +%FT%TZ)" >> "$LOG"
tail -c 700 "$LOG"
