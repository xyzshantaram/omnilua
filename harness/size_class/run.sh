#!/usr/bin/env bash
# size_class/run.sh — #113 measure-first size-class histogram driver.
#
# Builds the `size_class_histogram` example (a global-allocator-instrumented
# binary that runs one bench workload and dumps every live allocation size
# against the macOS libmalloc class table) and runs it across the workloads
# that matter for #113's RSS gap. Each workload runs in its own process so the
# peak-live snapshot is clean. Output is tee'd to harness/size_class/out/.
#
# Usage:
#   harness/size_class/run.sh                 # the default #113 workload set
#   harness/size_class/run.sh foo bar         # explicit workload names
set -euo pipefail

cd "$(dirname "$0")/../.."
OUT="harness/size_class/out"
mkdir -p "$OUT"

WORKLOADS=("$@")
if [ ${#WORKLOADS[@]} -eq 0 ]; then
    # closure_ops + binarytrees are the #113 tall poles; concat_chain +
    # string_format_mixed + table_hash_pressure complete the five-row
    # done-condition set (docs/ISSUE_BURNDOWN_SPEC.md); the rest are controls.
    WORKLOADS=(closure_ops binarytrees table_hash_pressure concat_chain \
               string_format_mixed gc_pressure fibonacci)
fi

echo "building size_class_histogram (release) ..."
cargo build -q --release -p omnilua --example size_class_histogram

BIN="target/release/examples/size_class_histogram"
for w in "${WORKLOADS[@]}"; do
    src="harness/bench/workloads/${w}.lua"
    if [ ! -f "$src" ]; then
        echo "skip: no such workload $src" >&2
        continue
    fi
    echo "==== $w ===="
    "$BIN" "$src" | tee "$OUT/${w}.txt"
    echo
done
