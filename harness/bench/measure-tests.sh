#!/usr/bin/env bash
# harness/bench/measure-tests.sh — record one official-test-suite pass-count
# measurement to the evidence ledger.
#
# Runs harness/run_official_all.sh, parses the Pass/Fail/Total/Timeout footer,
# appends a kind=tests row to harness/evidence/ledger.jsonl. Schema mirrors the
# kind=bench shape so harness/bench/history.py can render it as another panel.
#
# Usage:
#   bash harness/bench/measure-tests.sh
#
# Ledger row:
#   {kind:"tests", target:"official-suite", metric:"pass_count",
#    value:<pass>, total:<total>, fail:<fail>, timeout:<timeout>,
#    runtime_s:<elapsed>, commit, ts, os, arch, cpu, runner:"run_official_all",
#    schema_version:1, evidence:"harness/impl/official/run_all.tsv"}

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

LEDGER="$ROOT/harness/evidence/ledger.jsonl"
TSV="$ROOT/harness/impl/official/run_all.tsv"

TS=$(date -u +%Y%m%dT%H%M%SZ)
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
OS_NAME="$(uname -sr)"
ARCH="$(uname -m)"
CPU="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ *//' || echo 'unknown')"

START=$(date +%s)
TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT

if ! bash "$ROOT/harness/run_official_all.sh" >"$TMP" 2>&1; then
    echo "[warn] run_official_all.sh returned non-zero — recording counts anyway" >&2
fi
ELAPSED=$(( $(date +%s) - START ))

# Footer format:
#   Total:    44 official tests
#   Pass:     44  (100%)
#   Fail:      0
#   Timeout:   0
total=$(awk '/^[[:space:]]*Total:/ {print $2; exit}' "$TMP")
pass=$(awk '/^[[:space:]]*Pass:/ {print $2; exit}' "$TMP")
fail=$(awk '/^[[:space:]]*Fail:/ {print $2; exit}' "$TMP")
timeout=$(awk '/^[[:space:]]*Timeout:/ {print $2; exit}' "$TMP")

if [ -z "$total" ] || [ -z "$pass" ]; then
    echo "[err] failed to parse pass/total from run_official_all.sh output" >&2
    cat "$TMP" | tail -20 >&2
    exit 2
fi

mkdir -p "$(dirname "$LEDGER")"

python3 - "$LEDGER" "$TS" "$COMMIT" "$OS_NAME" "$ARCH" "$CPU" "$pass" "$total" "$fail" "$timeout" "$ELAPSED" <<'PY'
import json, sys
ledger, ts, commit, os_name, arch, cpu, pass_, total, fail, timeout_, elapsed = sys.argv[1:]
row = {
    "kind": "tests",
    "target": "official-suite",
    "metric": "pass_count",
    "value": int(pass_),
    "total": int(total),
    "fail": int(fail),
    "timeout": int(timeout_),
    "runtime_s": int(elapsed),
    "commit": commit,
    "ts": ts,
    "os": os_name,
    "arch": arch,
    "cpu": cpu,
    "runner": "run_official_all",
    "schema_version": 1,
    "evidence": "harness/impl/official/run_all.tsv",
}
with open(ledger, "a", encoding="utf-8") as f:
    f.write(json.dumps(row, sort_keys=True) + "\n")
print(f"appended: commit={commit} pass={pass_}/{total} fail={fail} timeout={timeout_} runtime={elapsed}s")
PY
