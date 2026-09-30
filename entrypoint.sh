#!/bin/bash
set -e

echo "[archava] Starting LiveKit voice worker in background..."
python3 backend/agent.py start &
WORKER_PID=$!

echo "[archava] Starting Public API server on port ${PORT:-5002}..."
node server/index.mjs &
API_PID=$!

cleanup() {
  echo "[archava] Received shutdown signal, gracefully stopping processes..."
  kill -TERM "$WORKER_PID" 2>/dev/null || true
  kill -TERM "$API_PID" 2>/dev/null || true
  wait "$WORKER_PID" 2>/dev/null || true
  wait "$API_PID" 2>/dev/null || true
  exit 0
}

trap cleanup SIGTERM SIGINT

# Wait for any process to terminate
wait -n "$WORKER_PID" "$API_PID"
EXIT_STATUS=$?
echo "[archava] Process exited with status $EXIT_STATUS. Shutting down container..."
cleanup
