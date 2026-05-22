# Phase E-1: Coroutine Thread Identity

**Order: do this second.** First slice of Phase E. No native stack switching yet.

Authoritative design: `docs/LUA_PHASE_E_RUNTIME_SPEC.md` Part 1, especially "Recommended Coroutine Architecture" and "Coroutine Implementation Slices" §1.

## Task

Implement real Lua thread identity and coroutine status without native stack switching. Eliminate the current placeholder behavior where `new_thread` constructs a real `LuaState` and discards it while pushing a `LuaThread::placeholder()`.

## Scope

- `crates/lua-types/src/value.rs` — adjust `LuaValue::Thread` payload if needed
- `crates/lua-vm/src/state.rs` — `GlobalState::threads`, `current_thread_id`, lookup helpers
- `crates/lua-stdlib/src/coro_lib.rs` — `coroutine.create`, `coroutine.status`, `coroutine.running`, `coroutine.isyieldable`, `coroutine.close` skeleton
- Tests: small Lua repros for create+status, running, isyieldable

## Requirements

- Introduce `ThreadId(u64)` (or similar) as the lightweight identity payload for `LuaValue::Thread`. Real `LuaState` storage stays in `lua-vm`.
- `GlobalState` owns `threads: Vec<GcRef<LuaState>>` (or equivalent map keyed by `ThreadId`), plus `current_thread_id` and `main_thread_id`.
- `new_thread` allocates a real `LuaState`, registers it under a fresh `ThreadId`, and returns a `LuaValue::Thread(ThreadId)`. No placeholder.
- `coroutine.create(f)` allocates a new thread, pushes `f` onto that thread's stack (uses `xmove` from the caller's stack — see slice 02b for xmove), and returns the thread value.
- `coroutine.status(thread)` implements the real state machine:
  ```text
  if target == current → "running"
  else if target.status == Yield → "suspended"
  else if target.status == Ok:
      if target has active frames → "normal"
      else if stack top == 0 → "dead"
      else → "suspended"
  else → "dead"
  ```
- `coroutine.running()` returns `(current_thread, current_thread == main_thread)`.
- `coroutine.isyieldable()` checks `current.is_yieldable` (already exists on `LuaState`).
- `coroutine.close(thread)` is allowed to be a stub that errors on non-dead/non-suspended threads — full close runs in slice 02d.

## What to NOT do

- **Do not add corosensei yet.** No native stack switching in this slice.
- **Do not create a duplicate `LuaState` type.** Keep the existing one; just add identity indirection.
- **Do not implement `resume` or `yield`** here. Those are slice 02b.
- **Do not put thread storage in `lua-types`** — would create a crate cycle. `lua-types` only knows `ThreadId`.

## Acceptance

```lua
local co = coroutine.create(function() end)
assert(coroutine.status(co) == "suspended")
assert(type(co) == "thread")
local main, ismain = coroutine.running()
assert(ismain == true)
assert(coroutine.isyieldable() == false)
print("ok")
```

This must run cleanly. Resume/yield are out of scope; calling `coroutine.resume(co)` may still error.

## What this unlocks

- Removes the placeholder thread hack in `new_thread`.
- Makes the rest of Phase E (slices 02b–02e) implementable.
- Doesn't move official-test pass count yet on its own — full coroutine.lua needs 02b + 02c.
