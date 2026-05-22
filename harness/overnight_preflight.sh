#!/usr/bin/env bash
# overnight_preflight.sh — sanity-check everything before launching overnight.sh.
#
# Run before sleep:
#   ./harness/overnight_preflight.sh
#
# Exits 0 if all green; non-zero if anything looks wrong.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0
warn=0

ok()   { printf "  \033[32mOK\033[0m   %s\n" "$1"; }
bad()  { printf "  \033[31mFAIL\033[0m %s\n" "$1"; fail=$((fail + 1)); }
note() { printf "  \033[33mWARN\033[0m %s\n" "$1"; warn=$((warn + 1)); }

echo "Overnight preflight check"
echo "═════════════════════════"

# 1. Auth: personal account, no API keys leaking.
if [ -d "$HOME/.claude-personal" ]; then
    ok "personal claude config dir exists"
else
    bad "missing $HOME/.claude-personal — overnight.sh requires subscription auth"
fi
if [ -n "${ANTHROPIC_API_KEY:-}" ] || [ -n "${ANTHROPIC_AUTH_TOKEN:-}" ]; then
    note "ANTHROPIC_API_KEY/AUTH_TOKEN is set in shell — overnight.sh unsets it internally before each claude -p, but if you want to be extra safe, 'unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN' before launch"
else
    ok "no API key in env (subscription path)"
fi

# 2. Tools.
for tool in claude jq awk git cargo rustc; do
    if command -v "$tool" >/dev/null; then
        ok "$tool found"
    else
        bad "$tool not in PATH"
    fi
done

# 3. Scripts exist + executable.
for s in harness/overnight.sh harness/fanout.sh harness/dispatch_compiler_fixer.sh; do
    if [ -x "$s" ]; then
        ok "$s executable"
    else
        bad "$s missing or not executable"
    fi
done

# 4. Output token cap.
if [ "${CLAUDE_CODE_MAX_OUTPUT_TOKENS:-32000}" -lt 64000 ]; then
    note "CLAUDE_CODE_MAX_OUTPUT_TOKENS=${CLAUDE_CODE_MAX_OUTPUT_TOKENS:-32000} (overnight.sh sets 64000 internally)"
else
    ok "CLAUDE_CODE_MAX_OUTPUT_TOKENS already 64000+"
fi

# 5. Git state.
dirty=$(git status --porcelain | wc -l | tr -d ' ')
if [ "$dirty" -gt 0 ]; then
    note "$dirty files dirty in working tree — overnight uses --allow-dirty for fanout, but consider committing first"
else
    ok "working tree clean"
fi

# 6. Phase A is complete (all 18 files real-ported).
missing=0
for f in llex.c lparser.c lcode.c lopcodes.c lvm.c ldo.c lstate.c ltm.c lobject.c lapi.c ldebug.c ldump.c lundump.c lfunc.c ltable.c lstring.c lctype.c lzio.c; do
    crate=$(awk -v c="$f" '!/^#/ && $1==c {print $2; exit}' ANALYSES/file_deps.txt 2>/dev/null)
    rel=$(awk -v c="$f" '!/^#/ && $1==c {print $3; exit}' ANALYSES/file_deps.txt 2>/dev/null)
    if [ -z "$crate" ] || [ -z "$rel" ]; then continue; fi
    target="crates/$crate/$rel"
    if [ ! -f "$target" ] || ! grep -qE '^//\s*source:.*\.[ch]\b' "$target" 2>/dev/null; then
        missing=$((missing + 1))
        note "Phase A target $target appears unported"
    fi
done
if [ "$missing" -eq 0 ]; then
    ok "all 18 Phase A files are real-ported"
fi

# 7. lua-types and lua-lex compile clean.
for c in lua-types lua-lex lua-parse lua-code; do
    n=$(cargo check -p "$c" 2>&1 | awk '/^error\[/ {c++} END {print c+0}')
    if [ "$n" -eq 0 ]; then
        ok "$c: 0 errors"
    else
        note "$c: $n errors (overnight will try to drive these down)"
    fi
done

# 8. lua-vm baseline.
n=$(cargo check -p lua-vm 2>&1 | awk '/^error\[/ {c++} END {print c+0}')
note "lua-vm baseline: $n errors (B_finish phase will target this)"

# 9. Existing pilot.jsonl size warning.
if [ -f harness/oracle/results/pilot.jsonl ]; then
    rows=$(wc -l < harness/oracle/results/pilot.jsonl | tr -d ' ')
    note "pilot.jsonl has $rows rows of prior runs (fanout appends, so this is fine)"
fi

# 10. Network reachable.
if curl -fsS -m 5 https://api.anthropic.com >/dev/null 2>&1; then
    ok "api.anthropic.com reachable"
else
    note "api.anthropic.com unreachable from preflight (may still work via Claude Code's transport)"
fi

echo ""
echo "═════════════════════════"
echo "Result: $fail fail, $warn warn"
echo ""
if [ "$fail" -gt 0 ]; then
    echo "DO NOT LAUNCH — fix the FAILs above first."
    exit 1
elif [ "$warn" -gt 0 ]; then
    echo "Acceptable. WARNs are informational; you can launch."
    exit 0
else
    echo "All green. Safe to launch:"
    echo "  nohup ./harness/overnight.sh > /tmp/overnight.log 2>&1 &"
    echo "  disown"
fi
