//! `global_shadow_kit` — issue #304, Lua 5.5 per-frame `global`/local name
//! resolution (the `global x` barrier done right).
//!
//! The rung-2 inner loop for the name-resolution refactor that moves the 5.5
//! `global` barrier decision INTO `singlevaraux`, at each recursive function
//! frame, BEFORE any capture side effect (`markupval` / `mark_vararg_table_needed`
//! / `search_upvalue` / `new_upvalue`) — exactly where reference 5.5 makes it
//! (`lparser.c` `searchvar`/`singlevaraux`). Deterministic, in-process, no
//! reference binary and no subprocess.
//!
//! Each case asserts BOTH the evaluated value AND the upvalue list of the
//! relevant nested function (via `debug.getupvalue`), because the whole point of
//! the refactor is that a `global`-shadowed capture leaves NO phantom upvalue —
//! a post-hoc barrier check could set the resolved kind to `Void` but could not
//! un-mint the upvalue `new_upvalue` already created. The oracle
//! (`specs/oracle/diff_one.sh 5.5`) is the truth-teller for exact reference
//! parity; this kit is the fast structural gate.
//!
//! Ground truth for every assertion below was captured from
//! `/tmp/lua-refs/bin/lua5.5.0`. Case letters mirror the spec §7 plan.

use omnilua::{Lua, LuaVersion};

const V53: LuaVersion = LuaVersion::V53;
const V54: LuaVersion = LuaVersion::V54;
const V55: LuaVersion = LuaVersion::V55;

/// Run `code` under `version` and return the single string it returns.
fn eval_str(version: LuaVersion, code: &str) -> String {
    let lua = Lua::new_versioned(version);
    lua.load(code).eval::<String>().unwrap_or_else(|e| {
        panic!("global_shadow_kit eval failure ({version:?}): {e:?}\ncode:\n{code}")
    })
}

/// Run `code` under `version`, expecting it to FAIL to compile/run, and return
/// the error's display string.
fn eval_err(version: LuaVersion, code: &str) -> String {
    let lua = Lua::new_versioned(version);
    match lua.load(code).exec() {
        Ok(()) => panic!("global_shadow_kit expected an error ({version:?}) but it succeeded\ncode:\n{code}"),
        Err(e) => format!("{e}"),
    }
}

/// (a) Divergence 1 headline: a captured local whose owner declares `global x`
/// AFTER it — the later `global x` wins, so the nested reader resolves to the
/// (nil) global and captures NO upvalue `x` (its only upvalue is `_ENV`).
#[test]
fn a_captured_local_owner_global_after() {
    let code = r#"
        global tostring, debug, table
        local x = 1
        global x
        local function g() return x end
        local names = {}
        local i = 1
        while true do
          local n = debug.getupvalue(g, i)
          if not n then break end
          names[#names + 1] = n
          i = i + 1
        end
        return tostring(g()) .. "|" .. table.concat(names, ",")
    "#;
    assert_eq!(eval_str(V55, code), "nil|_ENV", "owner-after global must shadow the captured local, no phantom upvalue x");
}

/// (b) Owner-BEFORE: `global x` then `local x = 1` — the newer local wins, so the
/// nested reader captures the local as an upvalue and sees its value.
#[test]
fn b_captured_local_owner_global_before() {
    let code = r#"
        global tostring, debug, table
        global x
        local x = 1
        local function g() return x end
        local names = {}
        local i = 1
        while true do
          local n = debug.getupvalue(g, i)
          if not n then break end
          names[#names + 1] = n
          i = i + 1
        end
        return tostring(g()) .. "|" .. table.concat(names, ",")
    "#;
    assert_eq!(eval_str(V55, code), "1|x", "local declared after the global must win and be captured");
}

/// (c) A `global x` in an intermediate function (between the reference site and
/// the enclosing local's owner) shadows the local outright.
#[test]
fn c_intermediate_function_global() {
    let code = r#"
        global tostring
        local x = 1
        local function mid()
          global x
          local function g() return x end
          return g()
        end
        return tostring(mid())
    "#;
    assert_eq!(eval_str(V55, code), "nil", "intermediate-function global must shadow outright");
}

/// (d) Two-level capture chain, both orderings: a barrier in the outermost owner
/// (after the local) shadows a read captured two levels down; a barrier in the
/// innermost function shadows its own direct read.
#[test]
fn d_two_level_chain_both_orderings() {
    let outer_after = r#"
        global tostring
        local x = 1
        global x
        local function a()
          local function b() return x end
          return b()
        end
        return tostring(a())
    "#;
    assert_eq!(eval_str(V55, outer_after), "nil", "outermost-owner global shadows a two-level-down capture");

    let inner_barrier = r#"
        global tostring
        local x = 1
        local function a()
          local function b()
            global x
            return x
          end
          return b()
        end
        return tostring(a())
    "#;
    assert_eq!(eval_str(V55, inner_barrier), "nil", "innermost-function global shadows its own read");
}

/// (e) Existing-upvalue shortcut (codex HIGH-2): the enclosing function ALREADY
/// has an upvalue `x` (from `local y = x`), but a later `global x` in that
/// function must still shadow an inner read — the per-frame global check must
/// precede the `search_upvalue` reuse shortcut.
#[test]
fn e_existing_upvalue_shortcut() {
    let code = r#"
        global tostring
        local x = 5
        local function outer()
          local y = x
          global x
          local function g() return x end
          return tostring(y) .. "|" .. tostring(g())
        end
        return outer()
    "#;
    assert_eq!(eval_str(V55, code), "5|nil", "y captures the pre-barrier upvalue; g's read is shadowed to nil");
}

/// (f) Divergence 2: `global function x` whose body declares an inner `local x`;
/// a function nested in the body reading `x` must capture that inner local
/// (value 1) with only upvalue `x` — NOT the global function itself, and NO
/// phantom upvalue.
#[test]
fn f_global_function_inner_local() {
    let code = r#"
        global tostring, debug, table
        global function x() local x = 1; local function g() return x end; return g end
        local g = x()
        local names = {}
        local i = 1
        while true do
          local n = debug.getupvalue(g, i)
          if not n then break end
          names[#names + 1] = n
          i = i + 1
        end
        return tostring(g()) .. "|" .. table.concat(names, ",")
    "#;
    assert_eq!(eval_str(V55, code), "1|x", "inner local wins inside a global-function body, only upvalue x");
}

/// (g) Divergence 4: `global function x` with an ENCLOSING `local x` — the
/// function's body reads `x` as the global (its own name), so it captures only
/// `_ENV`, NOT a phantom `x` from the enclosing local.
#[test]
fn g_global_function_with_enclosing_local() {
    let code = r#"
        global tostring, debug, table, type
        local x = 99
        global function x() return type(x) end
        local names = {}
        local i = 1
        while true do
          local n = debug.getupvalue(x, i)
          if not n then break end
          names[#names + 1] = n
          i = i + 1
        end
        return x() .. "|" .. table.concat(names, ",")
    "#;
    assert_eq!(eval_str(V55, code), "function|_ENV", "global function reads its own name as global, only _ENV captured");
}

/// (h) Divergence 3: a DIRECT (base) reference to a named vararg shadowed by a
/// later `global x` in the same function resolves to the global (nil), NOT the
/// vararg table — the shadow check must run for VarArgVar in the base case too.
#[test]
fn h_direct_named_vararg_shadowed() {
    let code = r#"
        global tostring
        local function f(...x) global x; return x end
        return tostring(f(10))
    "#;
    assert_eq!(eval_str(V55, code), "nil", "direct named-vararg read shadowed by a later global");
}

/// (i) A CAPTURED named vararg shadowed by an enclosing `global x` resolves to
/// the global (nil) with no vararg-table capture.
#[test]
fn i_captured_named_vararg_shadowed() {
    let code = r#"
        global tostring
        local function f(...x)
          global x
          local function g() return x end
          return g()
        end
        return tostring(f(10))
    "#;
    assert_eq!(eval_str(V55, code), "nil", "captured named-vararg shadowed by an enclosing global");
}

/// (j) Same-function `global x` after a local — a direct read after the barrier
/// resolves to the global (nil).
#[test]
fn j_same_function_global_after_local() {
    let code = r#"
        global tostring
        local x = 7
        global x
        return tostring(x)
    "#;
    assert_eq!(eval_str(V55, code), "nil", "same-function later global shadows the earlier local");
}

/// (k) `_ENV` rejections (§5): once `_ENV` itself resolves to a shadowing global,
/// the reference raises `"_ENV is global when accessing variable '<name>'"`. The
/// three forms reference rejects and the port previously accepted silently.
#[test]
fn k_env_rejections() {
    let k1 = eval_err(V55, "global function _ENV() end");
    assert!(
        k1.contains("_ENV is global when accessing variable '_ENV'"),
        "k1 got: {k1}"
    );

    let k2 = eval_err(V55, "global _ENV\nglobal function x() end");
    assert!(
        k2.contains("_ENV is global when accessing variable 'x'"),
        "k2 got: {k2}"
    );

    let k3 = eval_err(V55, "global _ENV\nglobal x = 1");
    assert!(
        k3.contains("_ENV is global when accessing variable 'x'"),
        "k3 got: {k3}"
    );
}

/// (l) Non-regression control: a captured local with NO shadowing global is
/// captured as an upvalue and read correctly.
#[test]
fn l_control_captured_local_no_shadow() {
    let code = r#"
        global tostring, debug
        local x = 1
        local function g() return x end
        return tostring(debug.getupvalue(g, 1)) .. "|" .. tostring(g())
    "#;
    assert_eq!(eval_str(V55, code), "x|1", "an unshadowed captured local must still be an upvalue");
}

/// (m) ≤5.4 non-regression: `global` is an ordinary identifier and a captured
/// local resolves normally (upvalue present, correct value) — the new threading
/// must be inert when `scope_barriers` is empty. The byte-identity of the
/// emitted bytecode for these versions is proven separately by the string.dump
/// sha check in the PR (spec §6); this case guards the functional path.
#[test]
fn m_pre_55_captured_local_unchanged() {
    for v in [V53, V54] {
        let code = r#"
            local x = 1
            local function g() return x end
            return tostring(debug.getupvalue(g, 1)) .. "|" .. tostring(g())
        "#;
        assert_eq!(eval_str(v, code), "x|1", "pre-5.5 captured local must be unaffected ({v:?})");
        // `global` remains a plain identifier pre-5.5.
        let ident = "local global = 7; return tostring(global * 6)";
        assert_eq!(eval_str(v, ident), "42", "`global` must stay an identifier ({v:?})");
    }
}
