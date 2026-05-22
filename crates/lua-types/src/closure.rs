//! `LuaClosure` — the function variant of `LuaValue`. Three sub-kinds:
//! Lua closure (compiled Proto + upvalues), C closure (function pointer +
//! upvalues), light C function (function pointer, no upvalues).

use crate::gc::GcRef;
use crate::proto::LuaProto;
use crate::upval::UpVal;
use crate::value::LuaValue;

/// Opaque registry index into `GlobalState.c_functions`, where the real
/// `lua_CFunction` (`fn(&mut LuaState) -> Result<usize, LuaError>`) is stored.
/// Lua-types can't reference `LuaState` without a circular dep, so we keep
/// the closure variant type-erased here and resolve through the registry at
/// call time.
pub type LuaCFnPtr = usize;

#[derive(Debug, Clone)]
pub enum LuaClosure {
    Lua(GcRef<LuaLClosure>),
    C(GcRef<LuaCClosure>),
    LightC(LuaCFnPtr),
}

#[derive(Debug)]
pub struct LuaLClosure {
    pub proto: GcRef<LuaProto>,
    pub upvals: Vec<GcRef<UpVal>>,
}

#[derive(Debug)]
pub struct LuaCClosure {
    pub func: LuaCFnPtr,
    pub upvalues: Vec<LuaValue>,
}

impl LuaLClosure {
    pub fn placeholder() -> Self {
        LuaLClosure {
            proto: GcRef::new(LuaProto::placeholder()),
            upvals: Vec::new(),
        }
    }
}
