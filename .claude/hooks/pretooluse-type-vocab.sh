#!/usr/bin/env bash
# PreToolUse hook (Edit/Write): reject the tool call if the proposed file
# content would introduce a type-vocabulary violation (a `pub struct/enum/
# trait/type NAME` whose canonical owner is a different file).
#
# This is the upgrade from the Stop-hook gate: catches violations BEFORE
# the file lands, so an agent doesn't spend 30 minutes building scaffolding
# around a duplicate type only to have its commit rejected.
#
# Receives a JSON payload on stdin describing the impending tool use:
#   {"tool_name": "Write" | "Edit", "tool_input": {...}}
#
# Exits:
#   0  allow the tool call
#   2  block the tool call (with a stderr message Claude can read)

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REGISTRY="$ROOT/harness/type-vocabulary.tsv"

PAYLOAD="$(cat)"

if [ ! -f "$REGISTRY" ]; then
    exit 0
fi

TOOL_NAME=""
TARGET_PATH=""
PROPOSED_CONTENT=""

if command -v jq >/dev/null 2>&1; then
    TOOL_NAME=$(echo "$PAYLOAD" | jq -r '.tool_name // empty')
    TARGET_PATH=$(echo "$PAYLOAD" | jq -r '.tool_input.file_path // empty')
    case "$TOOL_NAME" in
        Write)
            PROPOSED_CONTENT=$(echo "$PAYLOAD" | jq -r '.tool_input.content // empty')
            ;;
        Edit)
            PROPOSED_CONTENT=$(echo "$PAYLOAD" | jq -r '.tool_input.new_string // empty')
            ;;
        *)
            exit 0
            ;;
    esac
else
    exit 0
fi

case "$TARGET_PATH" in
    *.rs) ;;
    *) exit 0 ;;
esac

case "$TARGET_PATH" in
    /*) ABS_PATH="$TARGET_PATH" ;;
    *)  ABS_PATH="$ROOT/$TARGET_PATH" ;;
esac

# Filter to enforce-mode entries in the registry that the proposed content
# would define. For each match, check whether ABS_PATH is the canonical owner.
# Registry rows are whitespace-separated, not tab-separated — let read split
# on default IFS so any run of spaces or tabs works.
violations=()
while read -r name kind owner mode notes; do
    case "$name" in ''|'#'*) continue ;; esac
    [ "$mode" = "enforce" ] || continue
    pattern="^[[:space:]]*pub(\([^)]*\))?[[:space:]]+${kind}[[:space:]]+${name}\b"
    if echo "$PROPOSED_CONTENT" | grep -Eq "$pattern"; then
        case "$owner" in
            /*) OWNER_ABS="$owner" ;;
            *)  OWNER_ABS="$ROOT/$owner" ;;
        esac
        OWNER_REAL=$(cd "$(dirname "$OWNER_ABS")" 2>/dev/null && pwd)/$(basename "$OWNER_ABS")
        ABS_REAL=$(cd "$(dirname "$ABS_PATH")" 2>/dev/null && pwd)/$(basename "$ABS_PATH") 2>/dev/null || ABS_REAL="$ABS_PATH"
        if [ "$ABS_REAL" != "$OWNER_REAL" ]; then
            violations+=("$kind $name (canonical owner: $owner)")
        fi
    fi
done < "$REGISTRY"

if [ "${#violations[@]}" -gt 0 ]; then
    echo "[pretooluse-type-vocab] BLOCK: $TARGET_PATH would introduce vocabulary violation(s):" >&2
    for v in "${violations[@]}"; do
        echo "  - $v" >&2
    done
    echo "" >&2
    echo "Fix: import the canonical type via 'pub use <owner_crate>::<path>::<TypeName>;'" >&2
    echo "If your crate doesn't depend on the owner crate yet, add it to your Cargo.toml under [dependencies]." >&2
    echo "If duplication is genuinely intentional (test helper, etc.), have an architect update harness/type-vocabulary.tsv with mode=audit." >&2
    exit 2
fi

exit 0
