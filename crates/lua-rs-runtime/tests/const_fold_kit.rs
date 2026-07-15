//! `const_fold_kit` — issue #300, `<const>` compile-time-constant folding.
//!
//! The rung-2 inner loop for RDKCTC folding: deterministic, in-process, no
//! reference binary and no subprocess. Each case compiles a chunk under a
//! selected version and inspects the resulting function structurally via the
//! `debug` library (does a local / upvalue named `x` exist?) and via
//! `string.dump` (is a folded string materialized into the constant table?).
//!
//! A folded `<const>` emits NO register-local and NO upvalue; every reference
//! materializes the value inline. These cases assert exactly that, mirroring
//! spec §7 (a)-(f) with the codex MEDIUM-6 shapes: a small int folds to `LOADI`
//! (never a K entry), a string const folds through the constant table, and
//! bool/nil fold to `LOADFALSE`/`LOADTRUE`/`LOADNIL`. The oracle
//! (`specs/oracle/diff_one.sh`) remains the truth-teller for exact reference
//! parity; this kit is the fast structural gate.

use omnilua::{Lua, LuaVersion};

/// Run `code` under `version` and return the single string it returns.
fn eval_str(version: LuaVersion, code: &str) -> String {
    let lua = Lua::new_versioned(version);
    lua.load(code)
        .eval::<String>()
        .unwrap_or_else(|e| panic!("const_fold_kit eval failure ({version:?}): {e:?}\ncode:\n{code}"))
}

const V54: LuaVersion = LuaVersion::V54;
const V55: LuaVersion = LuaVersion::V55;

/// (a) A folded `<const>` int leaves no register-local named `x`.
#[test]
fn a_no_phantom_local_for_folded_int() {
    // Probe the active locals at a fixed pc; report whether any is named "x".
    // `x` is folded away, so the answer must be "absent" on 5.4 and 5.5.
    let code = r#"
        local x <const> = 42
        local a = 10
        local i = 1
        local found = "absent"
        while true do
          local nm = debug.getlocal(1, i)
          if not nm then break end
          if nm == "x" then found = "present" end
          i = i + 1
        end
        return found
    "#;
    for v in [V54, V55] {
        assert_eq!(eval_str(v, code), "absent", "folded <const> must emit no local x ({v:?})");
    }
}

/// (b) A nested function reading a folded `<const>` captures NO upvalue for it,
/// and still sees the value.
#[test]
fn b_no_phantom_upvalue_for_folded_const() {
    let code = r#"
        local x <const> = 7
        local function g() return x end
        local up = debug.getupvalue(g, 1)
        return tostring(up) .. "|" .. tostring(g())
    "#;
    for v in [V54, V55] {
        // No upvalue at index 1 => getupvalue returns nil; g() materializes 7.
        assert_eq!(eval_str(v, code), "nil|7", "folded <const> must not be captured as an upvalue ({v:?})");
    }
}

/// (c) The folded value is correct for int / float / bool / nil.
#[test]
fn c_folded_scalar_values_are_correct() {
    let cases = [
        ("local x <const> = 42; return tostring(x)", "42"),
        ("local x <const> = 3.5; return tostring(x)", "3.5"),
        ("local x <const> = true; return tostring(x)", "true"),
        ("local x <const> = false; return tostring(x)", "false"),
        ("local x <const> = nil; return tostring(x)", "nil"),
        ("local x <const> = 1000000; return tostring(x)", "1000000"),
    ];
    for v in [V54, V55] {
        for (code, want) in cases {
            assert_eq!(eval_str(v, code), want, "case {code:?} ({v:?})");
        }
    }
}

/// (c-string, MEDIUM-6) A folded string const is materialized THROUGH the
/// constant table: the dumped chunk contains the string bytes, and the value is
/// correct — with no upvalue in a nested reader.
#[test]
fn c_folded_string_goes_through_k_table() {
    let code = r#"
        local s <const> = "kitmarker"
        local function g() return s end
        local dump = string.dump(g)
        local has_k = dump:find("kitmarker", 1, true) ~= nil
        local up = debug.getupvalue(g, 1)
        return tostring(has_k) .. "|" .. tostring(up) .. "|" .. g()
    "#;
    for v in [V54, V55] {
        assert_eq!(
            eval_str(v, code),
            "true|nil|kitmarker",
            "folded string const must enter K and not be an upvalue ({v:?})"
        );
    }
}

/// (d) A `<const>` whose initializer is NOT a compile-time constant is NOT
/// folded — it stays a real register-local.
#[test]
fn d_non_constant_const_stays_a_local() {
    let code = r#"
        local x <const> = (function() return 5 end)()
        local i = 1
        local found = "absent"
        while true do
          local nm = debug.getlocal(1, i)
          if not nm then break end
          if nm == "x" then found = "present" end
          i = i + 1
        end
        return found .. "|" .. tostring(x)
    "#;
    for v in [V54, V55] {
        assert_eq!(eval_str(v, code), "present|5", "non-constant <const> must remain a local ({v:?})");
    }
}

/// (e) Left-operand fold (codex HIGH-3): `local b <const> = a + 2` where `a` is
/// itself a folded const must fold `b` too — neither `a` nor `b` is a local.
#[test]
fn e_left_operand_fold() {
    let code = r#"
        local a <const> = 1
        local b <const> = a + 2
        local i = 1
        local names = ""
        while true do
          local nm = debug.getlocal(1, i)
          if not nm then break end
          names = names .. nm .. ","
          i = i + 1
        end
        local has_a = names:find("a,", 1, true) ~= nil
        local has_b = names:find("b,", 1, true) ~= nil
        return tostring(b) .. "|" .. tostring(has_a) .. "|" .. tostring(has_b)
    "#;
    for v in [V54, V55] {
        assert_eq!(eval_str(v, code), "3|false|false", "a+2 must fold b, no locals a/b ({v:?})");
    }
}

/// (f) Two-level nested capture: an inner function two levels down reads the
/// folded const with NO upvalue at either level, and sees the value.
#[test]
fn f_two_level_nested_capture() {
    let code = r#"
        local x <const> = 99
        local function outer()
          local function inner() return x end
          return inner
        end
        local inner = outer()
        local up_outer = debug.getupvalue(outer, 1)
        local up_inner = debug.getupvalue(inner, 1)
        return tostring(up_outer) .. "|" .. tostring(up_inner) .. "|" .. tostring(inner())
    "#;
    for v in [V54, V55] {
        assert_eq!(eval_str(v, code), "nil|nil|99", "no upvalue at either nesting level ({v:?})");
    }
}

/// Const-of-const: a `<const>` initialized from another `<const>` folds.
#[test]
fn g_const_of_const() {
    let code = r#"
        local a <const> = 5
        local b <const> = a
        local function reader() return b end
        local up = debug.getupvalue(reader, 1)
        return tostring(up) .. "|" .. tostring(reader())
    "#;
    for v in [V54, V55] {
        assert_eq!(eval_str(v, code), "nil|5", "const-of-const must fold ({v:?})");
    }
}
