# Phase D-2.5: Budgeted Incremental GC Step

**Order: do this first.** Small, unlocks gc.lua progress, reduces harness ambiguity.

Authoritative design: `docs/LUA_PHASE_E_RUNTIME_SPEC.md` Part 2. Read sections "Recommended Budget Semantics" and "GC Implementation Slices" before writing code.

## Task

Implement budgeted incremental `collectgarbage("step", n)` semantics that match C-Lua's public pacing contract, without generational GC.

## Scope

- `crates/lua-gc/src/heap.rs`
- `crates/lua-vm/src/api.rs`
- `crates/lua-vm/src/state.rs`
- Rust unit tests in `lua-gc` and a small Lua repro

## Requirements

- `GcArgs::Step { data }` adds `data * 1024` bytes of debt, matching C-Lua.
- `data == 0` clears debt and runs one basic GC step.
- Larger `data` performs more collector work in one call than smaller `data`.
- `collectgarbage("step", n)` returns `true` (1) only when a cycle reaches the pause state.
- Preserve existing weak-table pruning + finalizer post-mark hook behavior — the atomic phase invokes `collect_via_heap`'s post-mark logic at fixed point.
- Do **not** implement generational GC. That stays Phase D-3.
- Do **not** add `unsafe` outside `lua-gc`.

## Sketch (from the spec)

```rust
pub struct StepBudget {
    remaining_work: isize,
    max_credit: isize,
}

pub enum StepOutcome { Paused, InProgress, SkippedStopped }

impl Heap {
    pub fn incremental_step_with_post_mark(
        &self,
        roots: &dyn Trace,
        budget: StepBudget,
        post_mark: impl FnMut(&mut Marker),
    ) -> StepOutcome;
}
```

Critical structural change: `Marker` state (gray queue, visited set) must move from stack-local (inside `full_collect_with_post_mark`) to **heap-owned** so a propagation phase can pause and resume across calls. Today the gray queue disappears when `full_collect_with_post_mark` returns.

State machine: `Pause` → `Propagate` → `Atomic` → `Sweep` → `Finalize` → `Pause`. Each phase respects the budget; atomic and finalize are bounded but allowed to run inside one step if cheap.

## Acceptance

Run this Lua program and it must pass:

```lua
collectgarbage("collect")
local function dosteps(n)
  local i = 0
  repeat
    i = i + 1
  until collectgarbage("step", n)
  return i
end
assert(dosteps(10) < dosteps(2))
```

Also add Rust unit tests covering:

- budget 0 performs ≥1 unit of work
- larger budget drains more gray objects than smaller budget
- sweep can pause mid-list and resume
- weak table pruning runs once per atomic phase
- full collection ≡ running incremental until pause

## What this unlocks

- `reference/lua-c/testes/gc.lua` line 201 — `assert(dosteps(10) < dosteps(2))`. May not get the whole test passing (gc.lua has 14 distinct subsystems) but moves the next failure point and removes the architectural plateau described in `docs/stuck-tests/gc.lua.md`.

## What to NOT do

- Don't try to fix gc.lua's "weak table prune" or "finalizer counts" sub-tests in this slice. Those depend on the budget infra first.
- Don't add generational mode. Phase D-3.
- Don't put unsafe in lua-vm or lua-stdlib.
- Don't move `Marker` into `lua-types` — keep it in `lua-gc`.
