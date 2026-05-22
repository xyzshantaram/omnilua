# Manual-dispatch agent prompts

Dispatch-ready prompt files for agent runs that don't go through the mega_loop's per-test track. Each file is self-contained and can be pasted as the user message of a `claude -p` invocation.

Authoritative design references: `docs/LUA_PHASE_E_RUNTIME_SPEC.md`, `PORT_STRATEGY.md` §3.6/3.9/3.10.

## Order (per the spec)

1. **[01-gc-budget.md](01-gc-budget.md)** — Phase D-2.5. Smallest, unblocks gc.lua plateau, reduces harness ambiguity.
2. **[02a-coroutine-thread-identity.md](02a-coroutine-thread-identity.md)** — Phase E-1. Real `ThreadId` indirection, no native stack switch.
3. **[02b-coroutine-resume-yield.md](02b-coroutine-resume-yield.md)** — Phase E-2. Pure-Lua resume/yield via `LuaError::Yield` unwind. Requires 02a.
4. **[03-dynlib-hook.md](03-dynlib-hook.md)** — Phase D-3.5 / Phase E adjacent. Operator-facing, no language-test impact alone.

Future slices (sketched in the spec, not yet drafted as prompts):

- 02c — `xmove` between threads sharing one `GlobalState`
- 02d — `close` and to-be-closed variables
- 02e — corosensei backend, only if 02b+02d aren't enough for `coroutine.lua`

## Dispatch pattern

These don't auto-fire from the mega_loop (which routes per-test). Dispatch manually:

```bash
claude -p \
  --model opus \
  --append-system-prompt "$(cat PORTING.md)" \
  --allowedTools "Read,Write,Edit,Glob,Grep,Bash(cargo build*),Bash(cargo check*),Bash(cargo test*),Bash(grep *),Bash(rg *),Bash(cat *),Bash(head *),Bash(tail *),Bash(wc *),Bash(find *),Bash(target/debug/lua-rs *)" \
  --permission-mode dontAsk \
  --max-budget-usd 30 \
  "$(cat harness/prompts/manual/01-gc-budget.md)"
```

Opus is the right tier for all four — these are architectural slices, not surface fixes.

## Why these are separate from `harness/prompts/<basename>.txt`

The per-test prompt files (e.g. `harness/prompts/files.lua.txt`) auto-inject into mega_loop's debug-agent dispatch when the matching test fails. They piggyback on the existing harness.

The manual prompts here implement net-new architecture that isn't surfaced by any single failing test (or, in 01's case, a SKIP'd test). They need explicit dispatch.
