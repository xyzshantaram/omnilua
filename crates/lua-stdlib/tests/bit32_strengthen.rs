//! Reference-pinned behavioral net for the `bit32` library (Lua 5.2 home).
//!
//! `bit32` was added in Lua 5.2 and removed in 5.4 (native bitwise operators
//! arrived in 5.3). In a stock build it is present in **both** 5.2 and 5.3
//! (5.3 keeps it under the default-on `LUA_COMPAT_BITLIB`), so the library's
//! registration gate is `V52 | V53`, not 5.2-only — verified against the
//! reference binaries (`type(bit32)`: nil/table/table/nil for 5.1/5.2/5.3/5.4).
//!
//! These cases pin `bit32`'s **mod-2^32** arithmetic, the logical/arithmetic
//! shifts and rotates, the `extract`/`replace` field+width bounds and their
//! exact error wording, and — the one behavior that genuinely differs between
//! the two host versions — how a **fractional float argument** is coerced. The
//! library predates the multiversion oracle's behavioral coverage of its
//! non-`band` surface, so this file is the net that lets that surface be
//! idiomatized safely.
//!
//! Every expected value was captured from `/tmp/lua-refs/bin/lua5.2.4` (the
//! canonical `bit32` home); the version-coercion contrast is captured from
//! `/tmp/lua-refs/bin/lua5.3.6`. The snippet is `load`+`pcall`ed *inside* Lua
//! so the VM renders values and error messages faithfully — a `LuaError`'s
//! Rust `Display` can't reach the heap to render an interned message string.
//! Modelled on `crates/lua-rs-runtime/tests/multiversion_oracle.rs`.
//!
//! `omnilua` is a dev-dependency here (it depends on `lua-stdlib`, so it can
//! only be a dev-dep — see `Cargo.toml`).

use omnilua::{Lua, LuaVersion};

/// Run `code` under `version`, returning `Ok(tostring(result))` or
/// `Err(error message)`. The chunk is `load`+`pcall`ed inside Lua so both
/// values and error messages are the VM's own rendering.
fn run(version: LuaVersion, code: &str) -> Result<String, String> {
    let lua = Lua::new_versioned(version);
    let wrapper = format!(
        "local f, e = load([==[\n{code}\n]==])\n\
         if not f then return 'E\\0' .. e end\n\
         local ok, r = pcall(f)\n\
         if not ok then return 'E\\0' .. tostring(r) end\n\
         return 'V\\0' .. tostring(r)"
    );
    let out: String = lua
        .load(&wrapper)
        .eval()
        .unwrap_or_else(|e| panic!("harness failure for `{code}`: {e:?}"));
    if let Some(v) = out.strip_prefix("V\0") {
        Ok(v.to_string())
    } else if let Some(e) = out.strip_prefix("E\0") {
        Err(e.to_string())
    } else {
        panic!("harness: unexpected output `{out}` for `{code}`")
    }
}

/// Assert `code` returns exactly `expected` under `version`.
fn eq(version: LuaVersion, code: &str, expected: &str) {
    match run(version, code) {
        Ok(got) => assert_eq!(got, expected, "code: {code}"),
        Err(e) => panic!("code `{code}` errored (`{e}`), expected `{expected}`"),
    }
}

/// Assert `code` errors under `version` with a message containing `needle`.
fn err_contains(version: LuaVersion, code: &str, needle: &str) {
    match run(version, code) {
        Ok(got) => {
            panic!("code `{code}` returned `{got}`, expected error containing `{needle}`")
        }
        Err(e) => assert!(e.contains(needle), "code `{code}` error `{e}` lacked `{needle}`"),
    }
}

const V52: LuaVersion = LuaVersion::V52;

// ─────────────────────────────────────────────────────────────────────────
// band / bor / bxor / bnot — the fold ops, including the empty-fold identity
// and mod-2^32 masking of out-of-range and negative inputs.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn fold_ops_basic_and_identities() {
    eq(V52, "return bit32.band(6, 3)", "2");
    eq(V52, "return bit32.bor(1, 2, 4)", "7");
    eq(V52, "return bit32.bxor(0xF, 0x3)", "12");
    eq(V52, "return bit32.bnot(0)", "4294967295");
    eq(V52, "return bit32.bnot(0xFFFFFFFF)", "0");
    // Empty fold returns the operator's identity element.
    eq(V52, "return bit32.band()", "4294967295");
    eq(V52, "return bit32.bor()", "0");
    eq(V52, "return bit32.bxor()", "0");
}

#[test]
fn fold_ops_mod_2_32_and_negative_inputs() {
    // Inputs above 2^32 are reduced mod 2^32 before the op.
    eq(V52, "return bit32.band(0x1FFFFFFFF)", "4294967295");
    eq(V52, "return bit32.bor(0x1FFFFFFFF, 0)", "4294967295");
    // Negatives are coerced to their unsigned 32-bit two's-complement image.
    eq(V52, "return bit32.band(-1)", "4294967295");
    eq(V52, "return bit32.bor(-1, 0)", "4294967295");
    eq(V52, "return bit32.bnot(-1)", "0");
    eq(V52, "return bit32.bxor(-2, 0)", "4294967294");
}

// ─────────────────────────────────────────────────────────────────────────
// btest — true iff the AND of all arguments is non-zero (empty AND is all-ones).
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn btest_truth_table() {
    eq(V52, "return bit32.btest(6, 3)", "true");
    eq(V52, "return bit32.btest(4, 3)", "false");
    eq(V52, "return bit32.btest(0)", "false");
    // No args: the AND identity is all-ones, which is non-zero -> true.
    eq(V52, "return bit32.btest()", "true");
}

// ─────────────────────────────────────────────────────────────────────────
// lshift / rshift — logical shifts; magnitude >= 32 yields 0, negative
// displacement shifts the other way.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn lshift_rshift_logical() {
    eq(V52, "return bit32.lshift(1, 4)", "16");
    eq(V52, "return bit32.lshift(1, 31)", "2147483648");
    eq(V52, "return bit32.lshift(0xFFFFFFFF, 4)", "4294967280");
    eq(V52, "return bit32.rshift(0x80000000, 4)", "134217728");
    eq(V52, "return bit32.rshift(1, 1)", "0");
}

#[test]
fn lshift_rshift_displacement_bounds() {
    // |disp| >= 32 -> 0 (logical shift falls off the 32-bit window).
    eq(V52, "return bit32.lshift(1, 32)", "0");
    eq(V52, "return bit32.rshift(1, 32)", "0");
    eq(V52, "return bit32.lshift(1, 100)", "0");
    eq(V52, "return bit32.rshift(0xFFFFFFFF, 100)", "0");
    // Negative displacement reverses the direction.
    eq(V52, "return bit32.lshift(1, -1)", "0");
    eq(V52, "return bit32.rshift(1, -1)", "2");
}

// ─────────────────────────────────────────────────────────────────────────
// arshift — arithmetic (sign-propagating) right shift; negative disp shifts
// left; disp >= 32 saturates to the sign fill.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn arshift_sign_propagation() {
    eq(V52, "return bit32.arshift(-8, 1)", "4294967292");
    eq(V52, "return bit32.arshift(0x80000000, 1)", "3221225472");
    eq(V52, "return bit32.arshift(0x40000000, 1)", "536870912");
    // disp >= 32 with the sign bit set saturates to all-ones; clear saturates 0.
    eq(V52, "return bit32.arshift(0x80000000, 32)", "4294967295");
    eq(V52, "return bit32.arshift(0x80000000, 100)", "4294967295");
    // Negative disp is a left shift.
    eq(V52, "return bit32.arshift(0x80000000, -1)", "0");
    eq(V52, "return bit32.arshift(0x40000000, -1)", "2147483648");
}

// ─────────────────────────────────────────────────────────────────────────
// lrotate / rrotate — rotate by disp mod 32; the two are mirror images.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn rotate_ops() {
    eq(V52, "return bit32.lrotate(1, 1)", "2");
    eq(V52, "return bit32.rrotate(1, 1)", "2147483648");
    eq(V52, "return bit32.lrotate(0x80000000, 1)", "1");
    eq(V52, "return bit32.lrotate(0x12345678, 4)", "591751041");
    eq(V52, "return bit32.rrotate(0x12345678, 4)", "2166572391");
}

#[test]
fn rotate_displacement_is_mod_32() {
    // disp 0 and disp 32 are both the identity; 36 == 4, -36 == -4 (== rrotate 4).
    eq(V52, "return bit32.lrotate(0xDEADBEEF, 0)", "3735928559");
    eq(V52, "return bit32.lrotate(0xDEADBEEF, 32)", "3735928559");
    eq(V52, "return bit32.lrotate(0xDEADBEEF, 36)", "3940282109");
    eq(V52, "return bit32.lrotate(0xDEADBEEF, -36)", "4260027374");
    // Negative lrotate == positive rrotate of the same magnitude.
    eq(V52, "return bit32.lrotate(1, -1)", "2147483648");
}

// ─────────────────────────────────────────────────────────────────────────
// extract — the `width` bits of `n` starting at `field`; `width` defaults to 1.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn extract_values() {
    eq(V52, "return bit32.extract(0xF0, 4, 4)", "15");
    eq(V52, "return bit32.extract(0x12345678, 0, 32)", "305419896");
    eq(V52, "return bit32.extract(0xFFFFFFFF, 31, 1)", "1");
    // width omitted -> 1: low bit of 0xFF is 1.
    eq(V52, "return bit32.extract(0xFF, 0)", "1");
    eq(V52, "return bit32.extract(5, 0)", "1");
    eq(V52, "return bit32.extract(5, 0, 1)", "1");
}

#[test]
fn extract_bounds_errors() {
    err_contains(V52, "return bit32.extract(0, -1)", "field cannot be negative");
    err_contains(V52, "return bit32.extract(0, 0, 0)", "width must be positive");
    err_contains(V52, "return bit32.extract(0, 0, -1)", "width must be positive");
    err_contains(
        V52,
        "return bit32.extract(0, 32, 1)",
        "trying to access non-existent bits",
    );
    err_contains(
        V52,
        "return bit32.extract(0, 0, 33)",
        "trying to access non-existent bits",
    );
    err_contains(
        V52,
        "return bit32.extract(0, 30, 4)",
        "trying to access non-existent bits",
    );
}

// ─────────────────────────────────────────────────────────────────────────
// replace — `n` with its `width` bits at `field` overwritten by the low bits
// of `v`; `width` defaults to 1; same bounds checks as `extract`.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn replace_values() {
    eq(V52, "return bit32.replace(0, 5, 0, 4)", "5");
    eq(V52, "return bit32.replace(0xFFFFFFFF, 0, 0, 4)", "4294967280");
    // Only the low `width` bits of `v` are used (0xFF masked to 4 bits = 0xF).
    eq(V52, "return bit32.replace(0, 0xFF, 0, 4)", "15");
    eq(V52, "return bit32.replace(0, 1, 31)", "2147483648");
    eq(V52, "return bit32.replace(0x12345678, 0xAB, 8, 8)", "305441656");
}

#[test]
fn replace_bounds_errors() {
    err_contains(V52, "return bit32.replace(0, 1, -1)", "field cannot be negative");
    err_contains(V52, "return bit32.replace(0, 1, 0, 0)", "width must be positive");
    err_contains(
        V52,
        "return bit32.replace(0, 1, 0, 33)",
        "trying to access non-existent bits",
    );
    err_contains(
        V52,
        "return bit32.replace(0, 1, 32, 1)",
        "trying to access non-existent bits",
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Non-number argument errors — the `bad argument #N (number expected, got X)`
// wording, including the field argument of extract.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn non_number_argument_errors() {
    err_contains(V52, "return bit32.band('x')", "number expected, got string");
    err_contains(V52, "return bit32.band(nil)", "number expected, got nil");
    err_contains(V52, "return bit32.band({})", "number expected, got table");
    err_contains(V52, "return bit32.extract(0, 'x')", "number expected, got string");
}

// ─────────────────────────────────────────────────────────────────────────
// Float-argument coercion — THE version-specific behavior.
//
// Under 5.2 (the FloatOnly number model) `bit32` coerces a number argument via
// `lua_tounsigned`: round-to-nearest-even, then take mod 2^32. A fractional
// float is therefore accepted, not rejected. Captured from lua5.2.4.
//
// Under 5.3 (the dual int/float model, `LUA_COMPAT_BITLIB`) the 5.3 `lbitlib.c`
// uses `luaL_checkinteger`, which REJECTS a non-integer-valued float with
// "number has no integer representation". Captured from lua5.3.6. The contrast
// pins that the 5.2 rounding path is genuinely version-gated, not an accident.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn v52_fractional_float_args_round_ties_even() {
    // Round to nearest, ties to even.
    eq(V52, "return bit32.band(1.5)", "2");
    eq(V52, "return bit32.band(2.5)", "2");
    eq(V52, "return bit32.band(3.5)", "4");
    eq(V52, "return bit32.band(0.5)", "0");
    eq(V52, "return bit32.band(0.4)", "0");
    eq(V52, "return bit32.band(0.6)", "1");
    eq(V52, "return bit32.band(2.49)", "2");
    eq(V52, "return bit32.band(2.51)", "3");
    // Negative fractional floats round then wrap mod 2^32.
    eq(V52, "return bit32.band(-1.5)", "4294967294");
    eq(V52, "return bit32.band(-2.5)", "4294967294");
    eq(V52, "return bit32.band(-0.5)", "0");
    // Integer-valued floats and out-of-range floats reduce mod 2^32.
    eq(V52, "return bit32.band(3.0)", "3");
    eq(V52, "return bit32.band(2^31)", "2147483648");
    eq(V52, "return bit32.band(2^32)", "0");
    eq(V52, "return bit32.band(4294967296.0)", "0");
}

#[test]
fn v52_fractional_count_args_truncate_toward_zero() {
    // The COUNT arguments (shift/rotate displacement, extract/replace field &
    // width) go through `luaL_checkint`, which TRUNCATES toward zero rather than
    // rounding — distinct from the operand path above. Captured from lua5.2.4.
    eq(V52, "return bit32.lshift(1, 1.5)", "2");
    eq(V52, "return bit32.lshift(1, 2.5)", "4");
    eq(V52, "return bit32.rshift(256, 2.5)", "64");
    eq(V52, "return bit32.lshift(256, -1.5)", "128");
    eq(V52, "return bit32.rshift(256, -1.5)", "512");
    eq(V52, "return bit32.lrotate(1, -1.5)", "2147483648");
    eq(V52, "return bit32.lrotate(1, -0.5)", "1");
    // extract/replace field & width truncate too: bit 1 of 0xAA is 1.
    eq(V52, "return bit32.extract(0xAA, 1.5)", "1");
    eq(V52, "return bit32.extract(0xAA, 2.5)", "0");
    eq(V52, "return bit32.extract(0xFF, 1.5, 2)", "3");
}

#[test]
fn v53_fractional_float_args_are_rejected() {
    // The version contrast: 5.3's bit32 (LUA_COMPAT_BITLIB) requires an integer
    // representation and rejects a fractional float, where 5.2 rounds/truncates.
    err_contains(
        LuaVersion::V53,
        "return bit32.band(1.5)",
        "number has no integer representation",
    );
    err_contains(
        LuaVersion::V53,
        "return bit32.lshift(1, 1.5)",
        "number has no integer representation",
    );
    err_contains(
        LuaVersion::V53,
        "return bit32.extract(0xFF, 1.5)",
        "number has no integer representation",
    );
    // Integer-valued floats are fine in 5.3.
    eq(LuaVersion::V53, "return bit32.band(3.0)", "3");
    eq(LuaVersion::V53, "return bit32.lshift(1, 4.0)", "16");
}
