//! `UpVal` — closure upvalues. PORT_STRATEGY §3.8.

use crate::value::LuaValue;
use crate::StackIdx;
use std::cell::Cell;

/// Open/closed state of an [`UpVal`], stored in a single [`Cell`].
///
/// The open and closed states are mutually exclusive, so overlapping their
/// storage in one enum keeps the payload at 24 bytes — `GcBox<UpVal>` = 48,
/// one libmalloc size-class below the 56-byte three-`Cell` layout it replaces
/// (#113 candidate 1: the 56→48 crossing drops the box from the 64-byte class
/// into the 48-byte class). `Open` carries the owning thread id as a full
/// `u64`; thread ids come from a globally monotonic, never-reused `u64`
/// counter, so the domain must not be narrowed. `idx` is the stack slot the
/// open upvalue refers to (the stack reallocates, so it is held by index).
/// `Closed` owns the value.
#[derive(Debug, Clone, Copy)]
enum UpValState {
    Open { thread_id: u64, idx: u32 },
    Closed(LuaValue),
}

/// A closure upvalue. Open upvalues point at a slot on a thread's stack
/// (referred to by index, since the stack reallocates). Closed upvalues
/// own the value.
///
/// State lives entirely in one [`Cell<UpValState>`] and is
/// single-source-of-truth. Closing is terminal — there is no re-open path — so
/// the state never reverts from `Closed` back to `Open`.
///
/// Read the open shape with [`try_open_payload`](UpVal::try_open_payload)
/// (`None` once closed) and the closed payload with
/// [`closed_value`](UpVal::closed_value) / [`try_closed_value`](UpVal::try_closed_value).
/// The `Cell` layout lets `state.rs::upvalue_get` / `upvalue_set` short-circuit
/// the Open path with zero borrow-guard overhead, which is the dominant cost in
/// fibonacci-class recursion benchmarks.
#[derive(Debug)]
pub struct UpVal {
    state: Cell<UpValState>,
}

/// `UpVal` is a GC-boxed hot object; every byte multiplies across the live
/// upvalue population (≈100k on closure_ops). The tagged-`Cell` layout keeps
/// the payload at 24 bytes so `GcBox<UpVal>` is 48 bytes — one libmalloc class
/// below the previous 56. Gated to 64-bit because the byte count is a
/// pointer-width claim (the wasm32 build has a 32-bit layout).
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<UpVal>() == 24);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<lua_gc::GcBox<UpVal>>() == 48);

impl UpVal {
    pub fn open(thread_id: u64, idx: StackIdx) -> Self {
        UpVal {
            state: Cell::new(UpValState::Open {
                thread_id,
                idx: idx.0,
            }),
        }
    }

    pub fn closed(v: LuaValue) -> Self {
        UpVal {
            state: Cell::new(UpValState::Closed(v)),
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.state.get(), UpValState::Open { .. })
    }
    pub fn is_closed(&self) -> bool {
        matches!(self.state.get(), UpValState::Closed(_))
    }

    /// Zero-overhead read of the open shape used by `upvalue_get` /
    /// `upvalue_set` and every out-of-crate consumer that inspects an open
    /// upvalue's `(thread_id, idx)`. Returns `Some((thread_id, idx))` when the
    /// upvalue is still open, `None` once it has been closed. `thread_id` is a
    /// `u64` end-to-end so the never-reused monotonic id domain is preserved on
    /// every target, including 32-bit `usize` ones (wasm32).
    #[inline(always)]
    pub fn try_open_payload(&self) -> Option<(u64, StackIdx)> {
        match self.state.get() {
            UpValState::Open { thread_id, idx } => Some((thread_id, StackIdx(idx))),
            UpValState::Closed(_) => None,
        }
    }

    /// Returns the closed-side value. Callers must have confirmed the
    /// upvalue is closed (`try_open_payload` returned `None`); an open upvalue
    /// reports [`LuaValue::Nil`], matching the value its closed slot held under
    /// the previous layout.
    #[inline(always)]
    pub fn closed_value(&self) -> LuaValue {
        match self.state.get() {
            UpValState::Closed(v) => v,
            UpValState::Open { .. } => LuaValue::Nil,
        }
    }

    pub fn close_with(&self, v: LuaValue) {
        self.state.set(UpValState::Closed(v));
    }

    pub fn set_closed_value(&self, v: LuaValue) {
        self.state.set(UpValState::Closed(v));
    }

    pub fn try_closed_value(&self) -> Option<LuaValue> {
        match self.state.get() {
            UpValState::Closed(v) => Some(v),
            UpValState::Open { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_scalar_write_updates_canonical_value() {
        let uv = UpVal::closed(LuaValue::Int(1));

        uv.set_closed_value(LuaValue::Int(2));

        assert_eq!(uv.closed_value(), LuaValue::Int(2));
        assert_eq!(uv.try_closed_value(), Some(LuaValue::Int(2)));
        assert!(uv.is_closed());
        assert_eq!(uv.try_open_payload(), None);
    }

    #[test]
    fn close_with_sets_cell_closed_state() {
        let uv = UpVal::open(7, StackIdx(3));
        assert_eq!(uv.try_open_payload(), Some((7, StackIdx(3))));

        uv.close_with(LuaValue::Bool(true));

        assert_eq!(uv.closed_value(), LuaValue::Bool(true));
        assert_eq!(uv.try_closed_value(), Some(LuaValue::Bool(true)));
        assert!(uv.is_closed());
        assert_eq!(uv.try_open_payload(), None);
    }

    /// A thread id above `i64::MAX` (and far above `u32::MAX`) must round-trip
    /// exactly — thread ids come from a never-reused monotonic `u64` counter.
    /// It would read back negative under an `i64` discriminant and truncate
    /// under a `u32`/32-bit-`usize` id, so this guards the full domain
    /// end-to-end.
    #[test]
    fn open_upvalue_preserves_full_u64_thread_id() {
        const BIG_TID: u64 = 0xFEDC_BA98_7654_3210;
        assert!(BIG_TID > i64::MAX as u64);
        let uv = UpVal::open(BIG_TID, StackIdx(9));
        assert_eq!(uv.try_open_payload(), Some((BIG_TID, StackIdx(9))));
        assert!(uv.is_open());
        assert_eq!(uv.try_closed_value(), None);
    }

    /// `closed_value()` on an open upvalue reports `Nil`, matching the value
    /// its closed slot held under the previous three-`Cell` layout (the
    /// byte-identical edge preserved by the tagged-`Cell` representation).
    #[test]
    fn open_upvalue_closed_value_reports_nil() {
        let uv = UpVal::open(3, StackIdx(1));
        assert!(uv.is_open());
        assert_eq!(uv.closed_value(), LuaValue::Nil);
    }
}
