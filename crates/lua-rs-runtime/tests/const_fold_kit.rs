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

/// LOW-1a: real-local / CTC / real-local. The folded CTC must consume no
/// register, so the following real local takes the slot the CTC would have (no
/// gap). `debug.getlocal` enumerates active locals in register order, so the
/// sequence starting `a=10 b=20 ` (b immediately after a) proves the CTC `c`
/// occupies no register between them. Verified byte-for-byte against the 5.4/5.5
/// reference binary.
#[test]
fn low1a_watermark_real_ctc_real() {
    let code = r#"
        local a = 10
        local c <const> = 1
        local b = 20
        local out = ""
        local i = 1
        while true do
          local n, v = debug.getlocal(1, i)
          if not n then break end
          out = out .. n .. "=" .. tostring(v) .. " "
          i = i + 1
        end
        return out .. "|sum=" .. (a + b + c)
    "#;
    for v in [V54, V55] {
        let r = eval_str(v, code);
        assert!(r.starts_with("a=10 b=20 "), "register placement wrong ({v:?}): {r}");
        assert!(r.ends_with("|sum=31"), "value wrong ({v:?}): {r}");
    }
}

/// LOW-1b: a CTC declared in a nested block whose scope then exits, after which
/// a fresh local reuses the register. The post-block local `w` must land in
/// register 0 (the slot the block's temporaries used), enumerated first.
#[test]
fn low1b_block_ctc_register_reuse() {
    let code = r#"
        do
          local c <const> = 5
          local t = c * 2
          local d = t + 1
        end
        local w = 99
        local z = w + 1
        local out = ""
        local i = 1
        while true do
          local n, v = debug.getlocal(1, i)
          if not n then break end
          out = out .. n .. "=" .. tostring(v) .. " "
          i = i + 1
        end
        return out .. "|" .. w .. "," .. z
    "#;
    for v in [V54, V55] {
        let r = eval_str(v, code);
        assert!(r.starts_with("w=99 z=100 "), "register reuse wrong ({v:?}): {r}");
        assert!(r.ends_with("|99,100"), "value wrong ({v:?}): {r}");
    }
}

/// LOW-2: the folded bytecode SHAPE, checked against the canonical bare-literal
/// form rather than just the returned value. `string.dump(f, true)` (stripped of
/// debug info) leaves exactly the code + constant table, so a folded `<const>`
/// that emits identical stripped bytecode to the equivalent bare literal proves
/// the exact opcode/operand and constant-table shape:
/// - a small int folds to `LOADI 42` with an EMPTY constant table (equal to
///   `return 42`), so there is provably no `Int(42)` K entry;
/// - a large int / a string fold through the constant table (`LOADK`, equal to
///   the bare literal, whose K holds that value);
/// - a float folds to `LOADF`; `true`/`false`/`nil` to `LOADTRUE`/`LOADFALSE`/
///   `LOADNIL`.
/// The non-const control must NOT match the bare literal (it keeps a real
/// register-local), so the equality genuinely discriminates.
#[test]
fn low2_folded_bytecode_matches_bare_literal() {
    let code = r#"
        local function eq(a, b) return string.dump(a, true) == string.dump(b, true) end
        local parts = {}
        parts[#parts+1] = "int="   .. (eq(function() local x <const> = 42;          return x end, function() return 42 end)          and "1" or "0")
        parts[#parts+1] = "bigint=".. (eq(function() local x <const> = 1000000;      return x end, function() return 1000000 end)     and "1" or "0")
        parts[#parts+1] = "string=".. (eq(function() local s <const> = "kitmarker";  return s end, function() return "kitmarker" end) and "1" or "0")
        parts[#parts+1] = "float=" .. (eq(function() local x <const> = 3.0;           return x end, function() return 3.0 end)         and "1" or "0")
        parts[#parts+1] = "floatk=".. (eq(function() local x <const> = 3.5;           return x end, function() return 3.5 end)         and "1" or "0")
        parts[#parts+1] = "true="  .. (eq(function() local x <const> = true;          return x end, function() return true end)        and "1" or "0")
        parts[#parts+1] = "false=" .. (eq(function() local x <const> = false;         return x end, function() return false end)       and "1" or "0")
        parts[#parts+1] = "nil="   .. (eq(function() local x <const> = nil;           return x end, function() return nil end)         and "1" or "0")
        parts[#parts+1] = "noncst=".. (eq(function() local x = 42;                    return x end, function() return 42 end)          and "1" or "0")
        return table.concat(parts, " ")
    "#;
    for v in [V54, V55] {
        assert_eq!(
            eval_str(v, code),
            "int=1 bigint=1 string=1 float=1 floatk=1 true=1 false=1 nil=1 noncst=0",
            "folded bytecode must match the bare literal's shape ({v:?})"
        );
    }
}

/// HIGH (5.5 barrier, parent-after): a `global x` declared in the CTC's OWNER
/// function AFTER the constant shadows it when read from a nested function.
/// Reference 5.5 scans globals and locals together at every recursive level;
/// the later `global x` wins, so `g()` reads the (nil) global, not the constant.
#[test]
fn h55_barrier_owner_global_after_ctc_shadows() {
    let code = r#"
        global tostring
        local x <const> = 1
        global x
        local function g() return x end
        return tostring(g())
    "#;
    assert_eq!(eval_str(V55, code), "nil", "later owner-function global must shadow the CTC");
}

/// HIGH (5.5 barrier, parent-before): a `global x` declared BEFORE the CTC in
/// the owner function is itself shadowed by the newer constant, so the nested
/// read resolves to the constant.
#[test]
fn h55_barrier_ctc_after_owner_global_wins() {
    let code = r#"
        global tostring
        global x
        local x <const> = 1
        local function g() return x end
        return tostring(g())
    "#;
    assert_eq!(eval_str(V55, code), "1", "CTC declared after the global must win");
}

/// HIGH (5.5 barrier, intermediate function): a `global x` in a function nested
/// between the reference site and the CTC's owner shadows the constant outright.
#[test]
fn h55_barrier_intermediate_function_shadows() {
    let code = r#"
        global tostring
        local x <const> = 1
        local function mid()
          global x
          local function g() return x end
          return g()
        end
        return tostring(mid())
    "#;
    assert_eq!(eval_str(V55, code), "nil", "intermediate-function global must shadow the CTC");
}
