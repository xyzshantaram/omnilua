#!/usr/bin/env bash
# Audit-ratchet check: counts the number of mode=audit entries in
# harness/type-vocabulary.tsv. Fails if the count exceeds the value
# committed to harness/audit-ratchet.lock.
#
# Purpose: prevent slow-creep growth of "soft" violations. New entries
# in the registry must enter as `enforce`, never `audit`. Existing
# `audit` entries can graduate to `enforce` (which lets the lock be
# decremented) or stay, but the count is strictly non-increasing.
#
# Usage:
#   harness/check_audit_ratchet.sh         # check (CI use)
#   harness/check_audit_ratchet.sh --update # write current count to lock
#                                             (architect-only operation)

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGISTRY="$ROOT/harness/type-vocabulary.tsv"
LOCK="$ROOT/harness/audit-ratchet.lock"

if [ ! -f "$REGISTRY" ]; then
    echo "[audit-ratchet] missing $REGISTRY" >&2
    exit 2
fi

current=$(awk 'BEGIN{c=0} !/^[[:space:]]*#/ && NF>=4 && $4=="audit" {c++} END{print c}' "$REGISTRY")

if [ "${1:-}" = "--update" ]; then
    echo "$current" > "$LOCK"
    echo "[audit-ratchet] updated $LOCK to $current"
    exit 0
fi

if [ ! -f "$LOCK" ]; then
    echo "[audit-ratchet] missing $LOCK (run --update once to initialize)" >&2
    exit 2
fi

allowed=$(tr -d ' \t\n\r' < "$LOCK")

if [ "$current" -gt "$allowed" ]; then
    echo "[audit-ratchet] FAIL: $current audit entries in registry, lock allows max $allowed" >&2
    echo "  New entries must enter as mode=enforce, not mode=audit." >&2
    echo "  If you're an architect intentionally allowing this, run:" >&2
    echo "      harness/check_audit_ratchet.sh --update" >&2
    echo "  and commit the updated $(basename "$LOCK")." >&2
    exit 1
fi

if [ "$current" -lt "$allowed" ]; then
    echo "[audit-ratchet] OK: $current/$allowed entries (consider running --update to tighten)"
else
    echo "[audit-ratchet] OK: $current/$allowed entries"
fi
exit 0
