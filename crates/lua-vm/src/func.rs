//! Auxiliary functions to manipulate prototypes and closures.
//!
//! Ported from `lfunc.c`/`lfunc.h`.
//!
//! # Design notes
//!
//! The C implementation uses two intrusive linked lists managed through pointer
//! fields embedded in stack slots and upvalue objects:
//!
//! - **`openupval`**: a singly-linked list of `UpVal`s sorted by stack level
//!   (highest first), threaded through `UpVal.u.open.next / .previous`.
//! - **`tbclist`**: a to-be-closed variable list encoded as `unsigned short` delta
//!   offsets stored inside `StackValue.tbclist.delta`.
//!
//! Both are replaced in the Rust port:
//! - `openupval` → `LuaState.openupval: Vec<GcRef<UpVal>>` (descending by StackIdx).
//! - `tbclist`   → `LuaState.tbclist: Vec<StackIdx>` (back = most recent entry).
//!
//! The delta-encoding machinery (MAXDELTA, dummy nodes) is an artifact of the u16
//! delta field and is entirely superseded by the `Vec<StackIdx>` model.

#[allow(unused_imports)]
use crate::prelude::*;

use crate::state::{GcRef, LuaState, LuaValue, UpVal};
use lua_types::error::LuaError;
pub use lua_types::{CallInfoIdx, StackIdx};

// ── lfunc.h constants ─────────────────────────────────────────────────────────

/// Sentinel status meaning "close upvalues but preserve the stack top."
/// Passed as `status` to `close` / `prep_call_close_mth`.
pub(crate) const CLOSE_K_TOP: i32 = -1;

// ── Closure allocation ────────────────────────────────────────────────────────

// C's `luaF_initupvals` (called from `ldo.c::f_parser`) fills a freshly
// loaded or parsed main closure's upvalue slots, which `luaU_undump` /
// `luaY_parser` leave NULL. This port has no equivalent: a
// `Cell<GcRef<UpVal>>` slot is non-nullable, so every closure-producing path
// (`undump` via `state.new_lclosure`, the text parser hook) fills its slots
// with fresh closed nil upvalues at construction rather than deferring the
// fill. The deferred-fill helper was therefore removed as dead code (issue
// #276); see the note in `do_.rs::f_parser`.

// ── Open-upvalue management ───────────────────────────────────────────────────

/// Creates a new open upvalue for stack slot `level`, inserts it into
/// `state.openupval` at `insert_pos`, and registers the thread in the
/// global `twups` list if necessary.
///
fn new_open_upval(state: &mut LuaState, level: StackIdx, insert_pos: usize) -> GcRef<UpVal> {
    // C's intrusive next/previous fields are gone; Vec insertion replaces the
    // pointer-threading, and the `prev` parameter (UpVal **) becomes
    // `insert_pos`.
    //
    // The home thread of the upvalue is whichever thread is currently
    // executing `find_upval` — it captures one of that thread's stack slots.
    // `GlobalState::cross_thread_upvals` is what a coroutine actually reads
    // or writes through when accessing an upvalue belonging to its parent.
    let owner_tid = state.global().current_thread_id as usize;
    let uv: GcRef<UpVal> = state.new_upval_open(owner_tid, level);
    // Vec insert maintains descending StackIdx order (highest first),
    // mirroring the C intrusive list where the head is always the topmost slot.
    state.openupval.insert(insert_pos, uv.clone());
    if !state_in_twups(state) {
    }
    uv
}

/// Finds or creates an open upvalue for stack slot `level`.
///
/// Searches `state.openupval` (sorted descending by StackIdx) for an existing
/// open upvalue at exactly `level`. If found, returns it. Otherwise, inserts a
/// new one at the correct sorted position and returns it.
///
pub(crate) fn find_upval(state: &mut LuaState, level: StackIdx) -> GcRef<UpVal> {
    debug_assert!(
        state_in_twups(state) || state.openupval.is_empty(),
        "thread must be in twups if it has open upvalues"
    );
    // The list is sorted descending. We scan from index 0 (highest) downward.
    // When we find an entry with idx < level we've passed the insertion point.
    let mut insert_pos = state.openupval.len(); // default: append at end
    for (i, uv_ref) in state.openupval.iter().enumerate() {
        let uv_idx = match uv_ref.try_open_payload() {
            Some((_thread_id, thread_stack_idx)) => thread_stack_idx,
            None => {
                debug_assert!(false, "closed upvalue found in openupval list");
                continue;
            }
        };
        if uv_idx.0 >= level.0 {
            if uv_idx == level {
                return uv_ref.clone();
            }
            // uv_idx.0 > level.0: this entry is higher on the stack; keep searching.
        } else {
            // uv_idx.0 < level.0: correct insertion point reached.
            insert_pos = i;
            break;
        }
    }
    new_open_upval(state, level, insert_pos)
}

// ── Close-method call helpers ─────────────────────────────────────────────────

/// Calls the `__close` metamethod on `obj` with error argument `err`.
/// `yy` controls whether the call is yieldable (true) or non-yieldable (false).
///
/// This function assumes EXTRA_STACK free slots are available.
///
fn call_close_method(
    state: &mut LuaState,
    obj: LuaValue,
    err: Option<LuaValue>,
    yy: bool,
) -> Result<(), LuaError> {
    // state.push() manages the top pointer; no pointer arithmetic needed.
    let tm = state.get_tm_by_obj(&obj, lua_types::tagmethod::TagMethod::Close);
    let top = state.top;
    state.push(tm);
    state.push(obj);
    if let Some(err) = err {
        state.push(err);
    }
    if yy {
        state.lua_call(top, 0)?;
    } else {
        state.lua_callnoyield(top, 0)?;
    }
    Ok(())
}

/// Checks that the value at `level` has a `__close` metamethod, raising a
/// runtime error if it does not.
///
fn check_close_mth(state: &mut LuaState, level: StackIdx) -> Result<(), LuaError> {
    let val = state.get_stack_value(level).clone();
    let tm = state.get_tm_by_obj(&val, lua_types::tagmethod::TagMethod::Close);
    if matches!(tm, LuaValue::Nil) {
        // CallInfo.func is the StackIdx of the function on the stack.
        let func_idx = state.current_ci().func;
        let idx = (level.0 as i32) - (func_idx.0 as i32);
        let vname_owned: Vec<u8> = state
            .debug_find_local(state.ci, idx)
            .unwrap_or_else(|| b"?".to_vec());
        // Lua variable names are ASCII identifiers; `escape_ascii` produces a
        // Display-compatible wrapper for the byte slice.
        return Err(LuaError::runtime(format_args!(
            "variable '{}' got a non-closable value",
            vname_owned.escape_ascii()
        )));
    }
    Ok(())
}

/// Prepares and calls the closing method for the variable at `level`.
///
/// If `status == CLOSE_K_TOP`, the error argument passed to `__close` is nil.
/// Otherwise, `set_error_obj` is called to materialise the error at `level + 1`
/// before the close method is invoked.
///
fn prep_call_close_mth(
    state: &mut LuaState,
    level: StackIdx,
    status: i32,
    yy: bool,
) -> Result<(), LuaError> {
    // Clone before any mutable operations to avoid borrow conflicts.
    let uv = state.get_stack_value(level).clone();
    let err = if state.global().lua_version == lua_types::LuaVersion::V55 {
        if status == CLOSE_K_TOP || status == lua_types::LuaStatus::Ok as i32 {
            None
        } else {
            state.set_error_obj(status, StackIdx(level.0 + 1))?;
            Some(state.get_stack_value(StackIdx(level.0 + 1)).clone())
        }
    } else if status == CLOSE_K_TOP {
        Some(LuaValue::Nil)
    } else {
        state.set_error_obj(status, StackIdx(level.0 + 1))?;
        Some(state.get_stack_value(StackIdx(level.0 + 1)).clone())
    };
    call_close_method(state, uv, err, yy)
}

// ── To-be-closed variable management ─────────────────────────────────────────

/// Inserts the variable at `level` into the to-be-closed (`tbc`) list.
///
/// If the value is falsy (nil or false) it does not need closing and the
/// function returns immediately. Otherwise it verifies that the value has a
/// `__close` metamethod, then records it in `state.tbclist`.
///
pub(crate) fn new_tbc_upval(state: &mut LuaState, level: StackIdx) -> Result<(), LuaError> {
    // In Rust: tbclist is Vec<StackIdx>, "current head" = last element.
    debug_assert!(
        state.tbclist.last().map_or(true, |&top| level.0 > top.0),
        "new tbc entry must be above current tbclist head"
    );
    // Clone before borrow to avoid aliasing with later mutable calls.
    let val = state.get_stack_value(level).clone();
    if matches!(val, LuaValue::Nil | LuaValue::Bool(false)) {
        return Ok(());
    }
    check_close_mth(state, level)?;
    // The MAXDELTA / dummy-node mechanism in C is an optimisation required
    // because `StackValue.tbclist.delta` is a `u16` (max 65535). With
    // `Vec<StackIdx>` the index fits a u32 and no dummy nodes are ever needed.
    state.tbclist.push(level);
    Ok(())
}

/// Closes all open upvalues whose stack index is ≥ `level`, transitioning each
/// from `UpVal::Open { thread_id: _, idx: thread_stack_idx }` to `UpVal::Closed(value)` by copying
/// the current stack value into the upvalue's own storage.
///
pub(crate) fn close_upval(state: &mut LuaState, level: StackIdx) {
    // openupval is sorted descending; front element is the topmost open upvalue.
    loop {
        let uv = match state.openupval.first() {
            Some(uv) => uv.clone(),
            None => break,
        };
        let uv_idx = match uv.try_open_payload() {
            Some((_thread_id, thread_stack_idx)) => thread_stack_idx,
            None => {
                // Cross-thread close/reset paths can leave a stale closed
                // upvalue in this Vec-backed open list. The C intrusive list
                // cannot represent that state; in Rust, unlink it and keep
                // closing the remaining open entries.
                state.openupval.remove(0);
                continue;
            }
        };
        if uv_idx.0 < level.0 {
            break;
        }
        // C asserts `uplevel(uv) < L->top.p` because the C stack is a
        // contiguous block where slots above top are undefined. The Rust stack is
        // a `Vec<StackValue>` whose backing storage outlives any top movement, so
        // reading `stack[uv_idx]` is always valid here even when `state.top` has
        // been rolled back below the upvalue (which is exactly what happens on
        // pcall error unwind, e.g. when `assert_fn` calls `set_top(L, 1)` before
        // raising). Dropping the C-style assertion lets close_upval correctly
        // close upvalues during error unwind regardless of top position.
        state.openupval.remove(0);
        let stack_val = state.get_stack_value(uv_idx).clone();
        uv.close_with(stack_val);
    }
}

/// Removes the most-recent entry from `state.tbclist`.
///
/// The C version must also skip over any delta==0 "dummy" nodes inserted to
/// bridge gaps larger than MAXDELTA. In Rust no dummy nodes are ever inserted,
/// so this is a straight `Vec::pop`.
///
fn pop_tbc_list(state: &mut LuaState) {
    // Delta-encoding dropped (see new_tbc_upval). Just pop.
    state.tbclist.pop();
}

/// Closes all upvalues and to-be-closed variables down to `level`, invoking
/// `__close` metamethods as needed. Returns the (stable) `level` index.
///
/// `status` is passed to `prep_call_close_mth` to determine the error argument:
/// `CLOSE_K_TOP` means nil; other statuses produce the appropriate error object.
/// `yy` controls yieldability of the close-method calls.
///
pub(crate) fn close(
    state: &mut LuaState,
    level: StackIdx,
    status: i32,
    yy: bool,
) -> Result<StackIdx, LuaError> {
    // savestack / restorestack are no-ops here. In C they save/restore a
    // pointer as a byte-offset because the stack may reallocate during close-method
    // calls. Here, StackIdx is an index into Vec and remains valid after any resize.

    close_upval(state, level);
    while state
        .tbclist
        .last()
        .copied()
        .map_or(false, |tbc| tbc.0 >= level.0)
    {
        let tbc = state
            .tbclist
            .last()
            .copied()
            .expect("tbclist non-empty (just checked)");
        pop_tbc_list(state);
        prep_call_close_mth(state, tbc, status, yy)?;
    }
    Ok(level)
}

// ── Debug helpers ─────────────────────────────────────────────────────────────

/// Returns the byte-string name of the `local_number`-th local variable that is
/// active at bytecode position `pc` in prototype `f`, or `None` if no such
/// variable exists.
///
/// Variables are scanned in order. A variable is active when
/// `startpc <= pc < endpc`. The first active variable is numbered 1.
///
pub(crate) fn get_local_name(
    f: &crate::state::LuaProto,
    local_number: i32,
    pc: i32,
) -> Option<&[u8]> {
    let mut remaining = local_number;
    // We break early once startpc > pc (variables are ordered by startpc).
    for lv in f.locvars.iter() {
        if lv.startpc > pc {
            break;
        }
        if pc < lv.endpc {
            remaining -= 1;
            if remaining == 0 {
                return Some(lv.varname.as_bytes());
            }
        }
    }
    None
}

// ── Private helpers (Rust-only) ───────────────────────────────────────────────

/// Returns `true` if this thread is already registered in `global.twups`.
///
/// Always returns `true`. This module never inserts into `GlobalState.twups`
/// (see `new_open_upval`); the check exists to satisfy the invariant
/// `state_in_twups || openupval.is_empty()` asserted by `find_upval`.
fn state_in_twups(state: &LuaState) -> bool {
    let _ = state;
    true
}

// ── LuaState methods this module depends on ──────────────────────────────────

/// `LuaState` methods this module depends on, implemented here by delegating
/// to their home modules (do_.rs, debug.rs).
impl LuaState {
    /// Returns the `LuaValue` at stack index `idx`.
    pub(crate) fn get_stack_value(&self, idx: StackIdx) -> &LuaValue {
        &self.stack[idx.0 as usize].val
    }

    /// Returns the current CallInfo (active call frame).
    ///
    pub(crate) fn current_ci(&self) -> &crate::state::CallInfo {
        &self.call_info[self.ci.0 as usize]
    }

    /// Looks up the `__close` (or other) metamethod for a value.
    ///
    pub(crate) fn get_tm_by_obj(
        &mut self,
        val: &LuaValue,
        tm: lua_types::tagmethod::TagMethod,
    ) -> LuaValue {
        let mt: Option<GcRef<lua_types::value::LuaTable>> = match val {
            LuaValue::Table(t) => t.metatable(),
            LuaValue::UserData(u) => u.metatable(),
            other => {
                let type_idx = other.base_type() as usize;
                self.global().mt[type_idx].clone()
            }
        };
        match mt {
            Some(mt_ref) => {
                let ename = self.global().tmname[tm as usize].clone();
                mt_ref.get_short_str(&ename)
            }
            None => LuaValue::Nil,
        }
    }

    /// Calls a Lua or C function (yieldable).
    ///
    pub(crate) fn lua_call(&mut self, top: StackIdx, nresults: i32) -> Result<(), LuaError> {
        crate::do_::call(self, top, nresults)
    }

    /// Calls a Lua or C function (non-yieldable).
    ///
    pub(crate) fn lua_callnoyield(&mut self, top: StackIdx, nresults: i32) -> Result<(), LuaError> {
        crate::do_::callnoyield(self, top, nresults)
    }

    /// Sets the error object at a given stack index for a given status code.
    ///
    pub(crate) fn set_error_obj(&mut self, status: i32, idx: StackIdx) -> Result<(), LuaError> {
        let s = lua_types::status::LuaStatus::from_raw(status);
        crate::do_::set_error_obj(self, s, idx);
        Ok(())
    }

    /// Returns the local-variable name at frame position `n` for CallInfo `ci`.
    ///
    pub(crate) fn debug_find_local(&self, ci: CallInfoIdx, n: i32) -> Option<Vec<u8>> {
        crate::debug::find_local(self, ci, n, None)
    }
}
