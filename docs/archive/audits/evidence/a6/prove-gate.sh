#!/usr/bin/env bash
# Prove the A6 coverage gate actually fails, then restore the measured baseline
# and prove it passes again. Restoring is unconditional (trap) so an interrupted
# run can never leave an inflated threshold on disk.
set -uo pipefail
cd "$(dirname "$0")/../../../.." || exit 1

CONFIG=vitest.config.ts
EVIDENCE=docs/audit/evidence/a6
BACKUP=$(mktemp /tmp/a6-vitest-config.XXXXXX.ts)
cp "$CONFIG" "$BACKUP"
BEFORE=$(sha256sum "$CONFIG" | cut -d' ' -f1)
restore() {
  cp "$BACKUP" "$CONFIG"
  AFTER=$(sha256sum "$CONFIG" | cut -d' ' -f1)
  echo "RESTORED sha256=$AFTER (before=$BEFORE)" | tee -a "$EVIDENCE/gate-run-restored.log"
  rm -f "$BACKUP"
}
trap restore EXIT

python3 - <<'PY'
from pathlib import Path

p = Path('vitest.config.ts')
s = p.read_text()

# Inflate the global lines floor above the measured 33.2%.
old_global = """const GLOBAL_COVERAGE_THRESHOLDS = {
  lines: 33.2,"""
new_global = """const GLOBAL_COVERAGE_THRESHOLDS = {
  lines: 34.2,"""
assert s.count(old_global) == 1, 'global block not found'
s = s.replace(old_global, new_global)

# Inflate one package glob above its measured floor (99.9 > 98.24).
old_pkg = "['packages/core', 98.24, 100, 95.23, 98.38]"
new_pkg = "['packages/core', 99.9, 100, 95.23, 98.38]"
assert s.count(old_pkg) == 1, 'packages/core row not found'
s = s.replace(old_pkg, new_pkg)

p.write_text(s)
print('armed: global lines 33.2 -> 34.2, packages/core lines 98.24 -> 99.9')
PY

echo "=== run with thresholds above baseline (expect non-zero) ==="
TMPDIR=/tmp pnpm exec vitest run --coverage >"$EVIDENCE/gate-run-thresholds-armed.log" 2>&1
ARMED_EXIT=$?
{
  echo "EXIT=$ARMED_EXIT"
  echo "--- threshold violations ---"
  grep -E "^ERROR: (Coverage|Uncovered)" "$EVIDENCE/gate-run-thresholds-armed.log" || echo "(no violation lines found)"
} | tee -a "$EVIDENCE/gate-run-thresholds-armed.log"

echo "=== restoring measured baseline ==="
# trap restores on exit; do the confirming run after an explicit restore so the
# pass is measured on the exact bytes that ship.
restore
trap - EXIT

echo "=== run with restored baseline (expect 0) ==="
TMPDIR=/tmp pnpm exec vitest run --coverage >"$EVIDENCE/gate-run-restored.log" 2>&1
RESTORED_EXIT=$?
{
  echo "EXIT=$RESTORED_EXIT"
  grep -E "^Statements|^Branches|^Functions|^Lines" "$EVIDENCE/gate-run-restored.log" || true
} | tee -a "$EVIDENCE/gate-run-restored.log"

echo "ARMED_EXIT=$ARMED_EXIT RESTORED_EXIT=$RESTORED_EXIT"
