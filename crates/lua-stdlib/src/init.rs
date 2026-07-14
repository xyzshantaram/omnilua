//! Initialization of standard libraries for Lua.
//!
//! Opens all standard libraries via `require`-style loading and registers
//! them into the global table.
//!
//! C source: `reference/lua-5.4.7/src/linit.c`.

use crate::state_stub::{LuaState, LuaStateStubExt as _};
use lua_types::error::LuaError;

type LuaCFunction = fn(&mut LuaState) -> Result<usize, LuaError>;

/// Opener function names are inconsistent across stdlib modules: `base`
/// exports the canonical `open`, while `string_lib`/`table_lib`/`math_lib`/
/// `io_lib`/`os_lib` still export their original `luaopen_*`/`open_*` names
/// (as do `utf8_lib`/`debug_lib`/`coro_lib`/`loadlib`). Unifying every
/// opener to `pub fn open` would let this table be written more uniformly.
static LOADED_LIBS: &[(&[u8], LuaCFunction)] = &[
    (b"_G", crate::base::open),
    #[cfg(feature = "package")]
    (b"package", crate::loadlib::luaopen_package),
    #[cfg(feature = "coroutine")]
    (b"coroutine", crate::coro_lib::open_coroutine),
    (b"table", crate::table_lib::open_table),
    #[cfg(feature = "io")]
    (b"io", crate::io_lib::luaopen_io),
    #[cfg(feature = "os")]
    (b"os", crate::os_lib::open_os),
    (b"string", crate::string_lib::luaopen_string),
    (b"math", crate::math_lib::luaopen_math),
    #[cfg(feature = "utf8")]
    (b"utf8", crate::utf8_lib::open_utf8),
    #[cfg(feature = "debug")]
    (b"debug", crate::debug_lib::open_debug),
];

/// Open all standard Lua libraries into `state`, registering each into the
/// global table.
///
/// Corresponds to `luaL_openlibs` in `linit.c`. The `true` argument to
/// `require_lib` means "set global": the loaded module value is assigned to
/// the global table under `name`, and the value left on the stack is then
/// discarded.
pub fn open_libs(state: &mut LuaState) -> Result<(), LuaError> {
    // Whether this version ships `utf8` (a 5.3 addition) is the #234 capability
    // matrix — the reference-backed single source — not a second inline version
    // check. The `#[cfg(feature = "utf8")]`-gated registration entries in
    // LOADED_LIBS still control compile-time availability.
    let has_utf8 = state
        .global()
        .lua_version
        .supports(lua_types::Feature::Utf8Lib);
    for &(name, func) in LOADED_LIBS {
        if name == b"utf8".as_slice() && !has_utf8 {
            continue;
        }
        state.require_lib(name, func, true)?;
        state.pop_n(1);
    }
    // `bit32` is present on 5.2 and 5.3 and removed in 5.4; the version dimension
    // comes from the #234 matrix, the compile-time dimension from the feature
    // gate (registration = `cfg(feature) && version.supports(Bit32Lib)`).
    #[cfg(feature = "bit32")]
    if state
        .global()
        .lua_version
        .supports(lua_types::Feature::Bit32Lib)
    {
        state.require_lib(b"bit32", crate::bit32_lib::open_bit32, true)?;
        state.pop_n(1);
    }
    Ok(())
}
