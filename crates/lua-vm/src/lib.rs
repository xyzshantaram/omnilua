//! Lua 5.4 virtual machine — runtime crate.
//!
//! Modules map to the canonical C source files per `ANALYSES/file_deps.txt`.

pub mod api;
pub mod debug;
pub mod do_;
pub mod dump;
pub mod func;
pub mod object;
#[cfg(feature = "opcode-profile")]
pub mod opcode_profile;
pub mod state;
pub mod string;
pub mod table;
pub mod tagmethods;
pub mod trace_impls;
pub mod undump;
pub mod vm;
pub mod zio;

/// Glob-imported by every module so the extension traits below resolve
/// without each module having to list them individually.
pub mod prelude {
    pub use crate::state::{
        LuaClosureExt, LuaLClosureRefExt, LuaProtoExt, LuaStringRefExt, LuaTableRefExt, LuaTypeExt,
        LuaUserDataRefExt, LuaValueExt, StackIdxExt,
    };
    pub(crate) use crate::tagmethods::TagMethod;
    pub use crate::vm::{InstructionExt, OpCode};
}
