# Phase D-3.5: Dynamic Library Loading Hook

**Order: do this fourth (after 01-GC and 02a/02b coroutines).** Operator-facing, doesn't block official Lua language tests.

Authoritative design: `docs/LUA_PHASE_E_RUNTIME_SPEC.md` Part 3.

## Task

Move `loadlib.c` platform dynamic loading behind embedder hooks. `lua-stdlib` stays free of `unsafe`. `lua-cli` installs a `libloading`-backed backend. Stock Lua C ABI modules return a clear "init" failure until a C-ABI facade exists (separate compatibility project).

## Scope

- `crates/lua-vm/src/state.rs` — add three optional hook fields to `GlobalState`
- `crates/lua-stdlib/src/loadlib.rs` — replace `lsys_load` / `lsys_sym` / `lsys_unloadlib` stubs with hook dispatch
- `crates/lua-cli/src/main.rs` — install the `libloading`-backed implementation (new dep)
- `crates/lua-cli/Cargo.toml` — add `libloading = "0.9"`
- Tests: smoke test for `package.loadlib(path, "*")` with hook present + absent

## Requirements

- `lua-stdlib` stays safe. No `dlopen`/`dlsym`/`libloading` references; no `unsafe`.
- `GlobalState` gains:
  ```rust
  pub dynlib_load_hook:   Option<DynLibLoadHook>,
  pub dynlib_symbol_hook: Option<DynLibSymbolHook>,
  pub dynlib_unload_hook: Option<DynLibUnloadHook>,
  ```
  with concrete trait/type aliases. Handle type: `DynLibId(u64)`.
- `package.loadlib(path, sym)` preserves C-Lua return shape:
  - success: pushes function (or `true` for `sym == "*"`) and returns 1.
  - failure: `(false, errmsg, "open" | "init")` and returns 3.
- If a hook is absent, behavior matches today's fallback (`LIB_FAIL = "absent"`).
- If a hook returns a stock Lua C ABI symbol, return a clear `"init"` failure:
  > "dynamic library loaded, but Lua C ABI modules are not supported by this build"
- Libraries stay alive for the lifetime of the Lua state (libloading's safety model demands it). Don't try to `dlclose` mid-state.

## DynamicSymbol enum (from the spec)

```rust
pub enum DynamicSymbol {
    RustNative(fn(&mut LuaState) -> Result<usize, LuaError>),
    LuaCAbi(unsafe extern "C" fn(*mut LuaCState) -> libc::c_int),
    Unsupported { reason: Vec<u8> },
}
```

For this slice, only `RustNative` is callable. `LuaCAbi` resolves the symbol but reports unsupported. `Unsupported` returns the reason verbatim.

## CLI backend

`lua-cli/src/main.rs` registers all three hooks before booting the VM. The backend stores `libloading::Library` values in a `Vec` (or `HashMap<u64, libloading::Library>`) inside CLI state, returns `DynLibId(idx)` from load, resolves symbols via `library.get::<T>(sym)`.

All `unsafe` lives inside the backend functions. Each block gets a `// SAFETY: ...` comment per the workspace policy. Update `harness/unsafe-budgets.toml` to grant `lua-cli` a small budget (1-3 blocks).

## What to NOT do

- **Don't expose a Lua C ABI shim.** That's a major compatibility project. Symbols compiled against `lua_State *` from upstream Lua 5.4 still error here — by design.
- Don't `dlclose` libraries mid-state. Drop happens at state close.
- Don't move `loadlib.rs` to a different crate. Keep its shape; just swap stubs for hook calls.
- Don't add `unsafe` to `lua-stdlib`. Hook-pattern only.

## Acceptance

Absent-hook test:

```lua
local ok, err, why = package.loadlib("./nonexistent.so", "luaopen_x")
assert(not ok)
assert(why == "absent")
print("ok absent")
```

Present-hook test (CLI backend):

```bash
# build a tiny Rust dylib exposing a RustNative module
# (test harness adds a helper for this)
target/debug/lua-rs -e '
  local ok, msg, why = package.loadlib("./libtest_rust_module.so", "luaopen_test")
  assert(ok, tostring(msg))
  print(ok())
'
```

Stock C-Lua module test (must clearly refuse):

```bash
# point at a known-good Lua 5.4 module from upstream
target/debug/lua-rs -e '
  local ok, msg, why = package.loadlib("./libcompat.so", "luaopen_compat")
  assert(not ok)
  assert(why == "init")
  print(msg)
'
```

## What this unlocks

- `package.loadlib` becomes operator-callable.
- Pure-Lua `require` of `.lua` files (already working as of `42ff10f` cherry-pick) is unaffected.
- No official-test pass-count movement on its own — but unlocks future C-ABI compat work without rearchitecting the safe boundary.
