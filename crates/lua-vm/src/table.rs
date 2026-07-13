//! Lua table — canonical implementation now lives in `lua-types::table`.
//!
//! This file is a thin re-export for compatibility with workspace
//! consumers (`lua-lex`, `lua-vm::trace_impls`) that previously
//! imported `lua_vm::table::LuaTable`. The interesting code has moved
//! to `crates/lua-types/src/table.rs`; see the doc comment there.

pub use lua_types::table::{LuaTable, TableFlags, TableNode, TableSlotRef};
