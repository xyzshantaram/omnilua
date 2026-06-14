//! Net for the `__name` metafield version gate (VM finding F2).
//!
//! `__name` — a metatable field that overrides an object's type name in
//! `tostring` and in type-error messages — is a **5.3 addition**. Lua 5.1 and
//! 5.2 ignore it entirely (`tostring` of a `__name`'d table is `"table: 0x.."`,
//! a type error names it `"table"`). 5.3+ honor it. This seam is invisible to
//! the 5.4-only official suite, so it is pinned here against the reference:
//! 5.2 `tostring(setmetatable({},{__name="X"}))` starts `"table"`; 5.3 starts
//! `"X"` (verified vs `/tmp/lua-refs/bin/lua5.{2.4,3.6}`).
//!
//! `omnilua` is a dev-dependency (it depends on `lua-stdlib`, so it can only be
//! a dev-dep — see `Cargo.toml`).

use omnilua::{Lua, LuaVersion};

/// Evaluate `code` (which must `return` a boolean) under `version`.
fn eval_bool(version: LuaVersion, code: &str) -> bool {
    let lua = Lua::new_versioned(version);
    lua.load(code)
        .eval()
        .unwrap_or_else(|e| panic!("eval of `{code}` failed under {version:?}: {e:?}"))
}

/// Does `tostring` of a `__name`'d table use the custom name?
fn tostring_honors_name(version: LuaVersion) -> bool {
    eval_bool(
        version,
        r#"return tostring(setmetatable({}, {__name = "ZZname"})):sub(1, 6) == "ZZname""#,
    )
}

/// Does a type-error message on a `__name`'d table use the custom name?
fn typeerror_honors_name(version: LuaVersion) -> bool {
    eval_bool(
        version,
        r#"local ok, e = pcall(math.abs, setmetatable({}, {__name = "ZZname"}))
           return (not ok) and e:find("ZZname") ~= nil"#,
    )
}

#[test]
fn name_metafield_in_tostring_is_5_3_plus_only() {
    assert!(!tostring_honors_name(LuaVersion::V51), "5.1 must ignore __name in tostring");
    assert!(!tostring_honors_name(LuaVersion::V52), "5.2 must ignore __name in tostring");
    assert!(tostring_honors_name(LuaVersion::V53), "5.3 must honor __name in tostring");
    assert!(tostring_honors_name(LuaVersion::V54), "5.4 must honor __name in tostring");
    assert!(tostring_honors_name(LuaVersion::V55), "5.5 must honor __name in tostring");
}

#[test]
fn name_metafield_in_typeerror_is_5_3_plus_only() {
    assert!(!typeerror_honors_name(LuaVersion::V51), "5.1 type error must say 'table', not __name");
    assert!(!typeerror_honors_name(LuaVersion::V52), "5.2 type error must say 'table', not __name");
    assert!(typeerror_honors_name(LuaVersion::V53), "5.3 type error must use __name");
    assert!(typeerror_honors_name(LuaVersion::V54), "5.4 type error must use __name");
    assert!(typeerror_honors_name(LuaVersion::V55), "5.5 type error must use __name");
}
