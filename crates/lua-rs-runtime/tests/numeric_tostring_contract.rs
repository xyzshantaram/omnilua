//! Issue #278 fix-round finding 4: the pushed-string contract for a numeric
//! `__tostring` return.
//!
//! C's `luaL_tolstring` accepts a number from `__tostring` (via `lua_isstring`)
//! and coerces the result slot to a string in place, so `type(tostring(x))` is
//! `"string"` on 5.2-5.5. Lua 5.1's `tostring` (no `luaL_tolstring`) returns the
//! metamethod's raw value, so the result stays a `number`. Verified against the
//! reference binaries per version.

use omnilua::{Lua, LuaVersion};

const DEF: &str = "local t = setmetatable({}, {__tostring = function() return 42 end}); ";

fn result_type(v: LuaVersion) -> String {
    let lua = Lua::new_versioned(v);
    lua.load(&format!("{DEF} return type(tostring(t))"))
        .eval()
        .unwrap()
}

fn rendered(v: LuaVersion) -> String {
    let lua = Lua::new_versioned(v);
    lua.load(&format!("{DEF} return (tostring(t) .. '')"))
        .eval()
        .unwrap()
}

#[test]
fn numeric_tostring_type_is_number_on_51_string_on_52plus() {
    assert_eq!(result_type(LuaVersion::V51), "number", "5.1 keeps the raw number");
    for v in [
        LuaVersion::V52,
        LuaVersion::V53,
        LuaVersion::V54,
        LuaVersion::V55,
    ] {
        assert_eq!(result_type(v), "string", "{v:?} coerces the slot to a string");
    }
}

#[test]
fn numeric_tostring_renders_the_number_on_every_version() {
    for v in [
        LuaVersion::V51,
        LuaVersion::V52,
        LuaVersion::V53,
        LuaVersion::V54,
        LuaVersion::V55,
    ] {
        assert_eq!(rendered(v), "42", "{v:?} renders 42");
    }
}
