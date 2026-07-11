#!/usr/bin/env bash
# Strict-guard gate for the issue #249 bug class (silent GC guard-coverage
# gaps). Runs the workspace suites with OMNILUA_GC_STRICT_GUARD=1, which
# turns the GC's three silent no-active-heap fallback arms into panics:
#
#   GcRef::new        -> detached allocation, never freed
#   GcRef::downgrade  -> weak handle that upgrades after sweep (UAF)
#   account_buffer    -> dropped pacer charge (accounting drift)
#
# Any guard-coverage gap on any path the suites exercise self-reports with a
# panic backtrace at the exact allocation site. The embedding leak canaries
# (crates/lua-rs-runtime/tests/leak_canaries.rs) run as part of the
# workspace: a counting global allocator asserts net-zero live bytes and a
# zero detached-allocation delta across VM/chunk/coroutine/callback churn.
#
# Run this whenever a change touches GC lifecycle, heap guards, VM
# construction, or embedding entry points. Green here + green oracle = the
# change neither leaks past VM drop nor mints sweep-blind weak handles.
set -euo pipefail
cd "$(dirname "$0")/.."

export OMNILUA_GC_STRICT_GUARD=1
cargo test --workspace --no-fail-fast "$@"
