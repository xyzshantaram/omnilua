#!/usr/bin/env bash
# GC gate for the issue #249 bug class (silent GC guard-coverage gaps). The
# three no-active-heap guard checks now panic UNCONDITIONALLY in every build —
# there is no env flag to enable; the panics are always on:
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
# construction, or embedding entry points. It is simply the convenience
# runner for the whole workspace under --no-fail-fast; green here + green
# oracle = the change neither leaks past VM drop nor mints sweep-blind weak
# handles.
set -euo pipefail
cd "$(dirname "$0")/.."

unset LUA_RS_GC_QUARANTINE LUA_RS_GC_STRESS
cargo test --workspace --no-fail-fast "$@"
