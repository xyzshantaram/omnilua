# Phase E-2: Pure Lua Resume/Yield

**Order: do this third (after 01-gc-budget and 02a-coroutine-thread-identity).**

Authoritative design: `docs/LUA_PHASE_E_RUNTIME_SPEC.md` Part 1, especially "Yield Propagation In Rust" and "Coroutine Implementation Slices" §2-§3.

## Task

Wire `coroutine.resume` / `coroutine.yield` for **pure Lua coroutines** using existing `do_.rs` `lua_resume` / `lua_yieldk` skeletons. No native stack switching — yield unwinds Rust frames via `LuaError::Yield` back to the `lua_resume` boundary.

## Prerequisites

- Slice 02a (`02a-coroutine-thread-identity.md`) must be merged. `ThreadId` indirection must exist.
- Optionally slice 01 (GC budget) — independent but doing it first reduces noise.

## Scope

- `crates/lua-vm/src/do_.rs` — `lua_resume`, `lua_yieldk` boundary handling
- `crates/lua-stdlib/src/coro_lib.rs` — `aux_resume`, `co_resume`, `aux_wrap`, `co_yield`
- `crates/lua-vm/src/api.rs` and `crates/lua-vm/src/vm.rs` — audit `Err(LuaError::Yield)` propagation through `state.call`, `call_no_yield`, `pcall_k`, metamethod-call, hook-call, stdlib-call paths
- Tests: targeted Lua repros for resume returning values, yield/resume exchange, wrap-error, nested resume

## Requirements

- `LuaError::Yield` is a **suspension signal** at `lua_resume` boundaries, NOT a user-visible runtime error.
- `lua_resume` translates `Err(LuaError::Yield)` from inner call → `LuaStatus::Yield` + values on the coroutine's stack.
- `coroutine.resume(co, args...)` returns:
  - `true, results...` on normal return or yield
  - `false, errobj` on coroutine error
- `coroutine.wrap(f)` returns a function that, when called, behaves like `select(2, assert(resume(co, ...)))` — i.e. **raises** errors instead of returning `(false, err)`. Replaces today's direct-call emulation in `aux_wrap`.
- `coroutine.yield(values...)` from inside a coroutine body suspends and yields the values to the resumer.
- `pcall` / `pcallk` must treat yield according to Lua's non-yieldable counter and continuation rules. `call_no_yield` paths must continue rejecting yields with the C-Lua-equivalent error.

## Critical audit: yield propagation through `?`

`Err(LuaError::Yield)` must NOT silently propagate through outer APIs that treat every `Err` as failure. Walk every `state.call`, `state.call_no_yield`, `pcall_k`, metamethod-call site, hook-call site, and stdlib `?` propagation, and make sure:

1. If the call is **not yieldable** → `LuaError::Yield` becomes a runtime error matching C-Lua's "attempt to yield across C-call boundary" message.
2. If the call is **yieldable** → `LuaError::Yield` bubbles up to the `lua_resume` boundary with the yield values intact on the stack.

Grep these for likely audit sites:
- `crates/lua-vm/src/api.rs` — `pcall_k`, `call`, `call_no_yield`
- `crates/lua-vm/src/do_.rs` — every `?` after an inner `state.call(...)`
- `crates/lua-vm/src/vm.rs` — the bytecode dispatch's CALL/TAILCALL/RETURN handling
- `crates/lua-stdlib/src/auxlib.rs` — `pcall_k` callers

## What to NOT do

- **No corosensei.** Slice 02e adds it only if tests prove it's required.
- Don't change `LuaError`'s variants. `LuaError::Yield` already exists.
- Don't try to handle yielding across a Rust/C call boundary (continuations). That's slice 02d.
- Don't fix all of `coroutine.lua` in one shot — get the core flow green first.

## Acceptance

```lua
-- Basic resume returning values
local co = coroutine.create(function(x) return x + 1, x * 2 end)
local ok, a, b = coroutine.resume(co, 10)
assert(ok and a == 11 and b == 20)
assert(coroutine.status(co) == "dead")

-- Basic yield/resume exchange
local co2 = coroutine.create(function()
  local y = coroutine.yield(1, 2)
  return y + 100
end)
local ok, a, b = coroutine.resume(co2)
assert(ok and a == 1 and b == 2)
assert(coroutine.status(co2) == "suspended")
local ok, c = coroutine.resume(co2, 5)
assert(ok and c == 105)
assert(coroutine.status(co2) == "dead")

-- wrap raises errors
local g = coroutine.wrap(function() error("boom") end)
local ok, err = pcall(g)
assert(not ok and string.find(err, "boom"))

print("ok")
```

## What this unlocks

- `coroutine.lua` becomes runnable for the first time. Most of its first ~200 lines should pass.
- The `aux_wrap` generator-emulation hack in `coro_lib.rs:153-251` (`wrap_iter_state`) becomes redundant and should be removed in this slice.
- Several stdlib functions that use coroutines internally (`string.gmatch`?) may stabilize.
