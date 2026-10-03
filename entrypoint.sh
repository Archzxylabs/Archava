#!/bin/bash
set -uo pipefail

echo "[archava] Starting LiveKit voice worker in background..."
python3 backend/agent.py start &
WORKER_PID=$!

echo "[archava] Starting Public API server on port ${PORT:-5002}..."
node server/index.mjs &
API_PID=$!

cleanup() {
  local exit_status="$1"
  trap - SIGTERM SIGINT
  echo "[archava] Stopping API and voice worker..."
  kill -TERM "$WORKER_PID" 2>/dev/null || true
  kill -TERM "$API_PID" 2>/dev/null || true
  wait "$WORKER_PID" 2>/dev/null || true
  wait "$API_PID" 2>/dev/null || true
  exit "$exit_status"
}

trap 'cleanup 0' SIGTERM SIGINT

# Wait for any process to terminate
wait -n "$WORKER_PID" "$API_PID"
EXIT_STATUS=$?
# Both children are services; even a clean unexpected exit triggers restart.
if [ "$EXIT_STATUS" -eq 0 ]; then EXIT_STATUS=1; fi
echo "[archava] Process exited with status $EXIT_STATUS. Shutting down container..."
cleanup "$EXIT_STATUS"
