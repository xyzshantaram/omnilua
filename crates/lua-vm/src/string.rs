//! Bootstrap string table used only to construct the pre-allocated
//! out-of-memory message during VM startup.
//!
//! [`LuaStringImpl`]/[`StringPool`] mirror C's `TString`/`stringtable`
//! (`lstring.c`/`lstring.h`), but nothing outside this module ever calls
//! [`new_lstr`] again after [`init`] runs once at startup: general Lua
//! string values and their interning go through `lua_types::LuaString` and
//! `GlobalState::interned_lt` (see `state.rs`). `GlobalState::strt` — the
//! [`StringPool`] this module maintains — ends up holding exactly one entry,
//! the memory-error message, for the life of the process.

#[allow(unused_imports)]
use crate::prelude::*;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::state::LuaState;

use lua_types::GcRef;

/// Converts the local `LuaStringImpl` into the canonical `lua_types::LuaString`
/// used everywhere else in the VM. Only called once, on the bootstrap OOM
/// message in [`init`].
fn impl_to_lt(s: &GcRef<LuaStringImpl>) -> GcRef<lua_types::LuaString> {
    GcRef::new(lua_types::LuaString::from_bytes(s.as_bytes().to_vec()))
}

// ── Constants ─────────────────────────────────────────────────────────────────

/// Pre-allocated OOM error message.  Must be created before the allocator
/// can fail so that the GC can always hand back a valid error string.
pub(crate) const MEMERR_MSG: &[u8] = b"not enough memory";

const MIN_STR_TAB_SIZE: usize = 128;

const STRCACHE_N: usize = 53;

const STRCACHE_M: usize = 2;

pub(crate) const MAX_SHORT_LEN: usize = 40;

const MAX_SIZE: usize = if std::mem::size_of::<usize>() < std::mem::size_of::<i64>() {
    usize::MAX
} else {
    i64::MAX as usize
};

/// Upper bound on the number of hash buckets; derived from `i32::MAX` / pointer size.
const MAX_STR_TAB: usize = i32::MAX as usize / std::mem::size_of::<usize>();

// ── LuaStringImpl ────────────────────────────────────────────────────────────

/// Whether a Lua string is short (interned) or long (not interned).
///
/// Corresponds to the `LUA_VSHRSTR` / `LUA_VLNGSTR` tags from `lobject.h`;
/// C distinguishes them via a `shrlen` sentinel value (0xFF) rather than a
/// separate enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringKind {
    Short,
    Long,
}

/// A Lua string: an immutable, reference-counted byte sequence. Corresponds
/// to C's `TString`.
///
/// Short strings (`<= MAX_SHORT_LEN = 40` bytes) are interned in the
/// [`StringPool`] on `GlobalState`; two short strings with the same bytes
/// are guaranteed to be the same `GcRef` (pointer equality via `Rc::ptr_eq`).
/// In practice the only `LuaStringImpl` ever created is the bootstrap OOM
/// message — see the module doc.
///
/// Long strings are heap-allocated independently and never interned. `hash`
/// is set once at construction in [`create_str_obj`], not computed lazily.
pub struct LuaStringImpl {
    bytes: Rc<[u8]>,

    // Replaced by the StringKind enum; length is implicit in bytes.len().
    kind: StringKind,

    // Using Cell<u32> so that `hash_long_str` can cache the hash through a
    // shared `&LuaStringImpl` reference (interior mutability, single-threaded).
    #[allow(dead_code)]
    hash: Cell<u32>,

    // Short strings: reserved-word token index (0 = not a keyword).
    // Long strings:  0 = hash not yet computed; 1 = hash is valid.
    extra: Cell<u8>,
}

impl LuaStringImpl {
    /// Returns the string's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the byte length of the string.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns `true` if this is a long (non-interned) string.
    pub fn is_long(&self) -> bool {
        self.kind == StringKind::Long
    }

    /// Returns `true` if this is a short (interned) string.
    pub fn is_short(&self) -> bool {
        self.kind == StringKind::Short
    }

    /// Returns `true` if this short string is a Lua reserved word.
    pub fn is_reserved_word(&self) -> bool {
        self.kind == StringKind::Short && self.extra.get() > 0
    }

    /// GC color predicate. `LuaStringImpl` values are never registered with
    /// the tracing collector (see the module doc), so this always returns
    /// `false`.
    pub fn is_white(&self) -> bool {
        false
    }

    /// Flip GC color from white to the current non-white (resurrect a dead
    /// object). No-op; see [`Self::is_white`].
    pub fn flip_white(&self) {
    }
}

impl PartialEq for LuaStringImpl {
    /// Equality for Lua strings.
    ///
    /// For short strings (interned), pointer equality via `Rc::ptr_eq` is sufficient
    /// and matches `eqshrstr` in C.  For long strings, we fall back to byte
    /// comparison, matching `luaS_eqlngstr` in C.
    fn eq(&self, other: &Self) -> bool {
        if self.kind == StringKind::Short && other.kind == StringKind::Short {
            Rc::ptr_eq(&self.bytes, &other.bytes)
        } else {
            self.bytes == other.bytes
        }
    }
}

impl Eq for LuaStringImpl {}

// ── StringPool ───────────────────────────────────────────────────────────────
//
// Corresponds to C's `stringtable`, which used an open-addressing hash table
// where each bucket was the head of an intrusive singly-linked list threaded
// through `TString.u.hnext`. The `HashMap` here replaces both the bucket
// array and the chain: it provides O(1) average-case lookup, automatic
// rehashing, and eliminates the need for `tablerehash`.
//
// `nuse` is redundant with `map.len()`; kept for parity with the C
// invariants that other code in this module checks (e.g. `growstrtab` tests
// `nuse >= size`).

/// Intern table for short Lua strings.  Lives on `GlobalState`.
pub struct StringPool {
    // Keyed by owned byte slice; lookup by `&[u8]` via Borrow<[u8]>.
    map: HashMap<Box<[u8]>, GcRef<LuaStringImpl>>,

    nuse: usize,

    // In Rust, HashMap manages its own capacity; this tracks the last requested size.
    size: usize,
}

impl StringPool {
    /// Create an empty pool with `MIN_STR_TAB_SIZE` preallocated capacity.
    pub fn new() -> Self {
        StringPool {
            map: HashMap::with_capacity(MIN_STR_TAB_SIZE),
            nuse: 0,
            size: MIN_STR_TAB_SIZE,
        }
    }
}

impl Default for StringPool {
    fn default() -> Self {
        Self::new()
    }
}

// ── LuaUserData ──────────────────────────────────────────────────────────────

/// Corresponds to C's `Udata`: a GC-tracked object carrying a raw byte
/// payload plus optional Lua user values and an optional metatable.
///
/// Never constructed: `metatable`/`uv` are still placeholder `()` types
/// rather than `GcRef<LuaTable>`/`LuaValue`, and no call site builds a
/// `LuaUserDataImpl`. The userdata type actually used throughout the VM is
/// `lua_types::userdata::LuaUserData`.
pub struct LuaUserDataImpl {
    pub len: usize,
    pub nuvalue: u16,
    pub metatable: Option<()>,
    pub uv: Vec<()>,
    // The raw byte payload; C accessed the equivalent via udatamemoffset
    // pointer arithmetic on a flexible array member.
    pub data: Box<[u8]>,
}

// ── Public functions ───────────────────────────────────────────────────────────

// lstring.h: LUAI_FUNC → pub(crate)
/// Hash a byte string with a seed using Lua's FNV-style hash.
///
/// This is a pure function with no allocations.  The algorithm XORs shifts and
/// additions over each byte in reverse order, seeded by `seed ^ len`. Mirrors
/// C's `luaS_hash`.
///
/// C parenthesises `(h<<5)` and `(h>>2)` explicitly, so the outer additions
/// are unambiguous despite C's `<<`/`>>` having lower precedence than `+`.
/// In Rust `<<` and `>>` have higher precedence than `+`, so the same
/// expression is computed without extra parentheses; `wrapping_add` is used
/// to match C's unsigned wrap-around arithmetic.
pub(crate) fn hash_bytes(bytes: &[u8], seed: u32) -> u32 {
    let mut h: u32 = seed ^ (bytes.len() as u32);

    let mut l = bytes.len();
    while l > 0 {
        l -= 1;
        h ^= (h << 5).wrapping_add(h >> 2).wrapping_add(bytes[l] as u32);
    }

    h
}

/// Resize the string intern table to approximately `nsize` buckets.
///
/// C's `tablerehash` walked the intrusive `hnext` chain in each bucket and
/// redistributed `TString *` pointers into new bucket slots; that entire
/// mechanism is unneeded here since `HashMap` rehashes itself automatically.
/// When growing, `HashMap::reserve` hints the desired capacity. When
/// shrinking, `HashMap::shrink_to` is a hint rather than a guarantee (C
/// freed exact memory), used as an approximation of the C logic that would
/// rehash entries out of the shrinking tail. The C function's graceful
/// degradation on allocation failure (keep the current size) is preserved:
/// `HashMap` will simply retain its existing capacity if memory is tight.
pub(crate) fn resize(state: &mut LuaState, nsize: usize) {
    let strt = &mut state.global_mut().strt;
    let osize = strt.size;

    if nsize > osize {
        let additional = nsize.saturating_sub(strt.map.len());
        strt.map.reserve(additional);
    } else if nsize < osize {
        strt.map.shrink_to(nsize);
    }

    strt.size = nsize;
}

// lstring.h: LUAI_FUNC → pub(crate)
/// Initialise the string intern table and the API string cache.
///
/// Must be called exactly once during VM startup, before any strings are created.
/// Pre-creates the memory-error message, then fills every cache slot with
/// that same string.
///
/// C fixes the message in the GC (`luaC_fix`, marking it non-collectable);
/// there is no equivalent call here (`state.gc().fix_object` is a no-op —
/// see its doc in state.rs). The message instead stays alive for the life
/// of the process simply because `GlobalState::memerrmsg` holds a
/// permanent strong reference to it.
pub(crate) fn init(state: &mut LuaState) -> Result<(), LuaError> {
    // StringPool::new() sets the initial capacity to MIN_STR_TAB_SIZE,
    // replacing both the C allocation and the tablerehash clear pass.
    state.global_mut().strt = StringPool::new();

    let memerrmsg = new_lstr(state, MEMERR_MSG)?;

    let memerrmsg_lt = impl_to_lt(&memerrmsg);
    state.global_mut().memerrmsg = memerrmsg_lt.clone();

    for i in 0..STRCACHE_N {
        for j in 0..STRCACHE_M {
            state.global_mut().strcache[i][j] = memerrmsg_lt.clone();
        }
    }

    Ok(())
}

/// Create or retrieve a Lua string from `bytes`.
///
/// If `bytes.len() <= MAX_SHORT_LEN` (40), the string is interned: an existing
/// identical short string is returned if found, otherwise a new one is created
/// and inserted into the intern table.
///
/// If `bytes.len() > MAX_SHORT_LEN`, a new long string is allocated each time
/// (long strings are never interned).
pub(crate) fn new_lstr(
    state: &mut LuaState,
    bytes: &[u8],
) -> Result<GcRef<LuaStringImpl>, LuaError> {
    if bytes.len() <= MAX_SHORT_LEN {
        intern_short_str(state, bytes)
    } else {
        // `sizeof(TString)` is a C-specific per-object overhead; here we
        // just check that the byte count fits within MAX_SIZE.
        if bytes.len() >= MAX_SIZE {
            return Err(LuaError::Memory);
        }

        let seed = state.global().seed;
        let h = hash_bytes(bytes, seed);
        let ts = create_str_obj(state, bytes, StringKind::Long, h);
        Ok(ts)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Allocate and initialise a new `LuaStringImpl` with the given bytes, kind, and hash.
///
/// In C, `createstrobj` allocated uninitialised memory via `luaC_newobj` and set
/// the header fields; the caller then filled the content via `memcpy`. Here the
/// string is constructed directly from the provided `bytes`, eliminating the
/// two-step pattern. `Rc<[u8]>` stores the bytes without C's nul terminator;
/// callers that need a nul-terminated `*const u8` for FFI must use a temporary
/// `CString` or equivalent.
fn create_str_obj(
    state: &mut LuaState,
    bytes: &[u8],
    kind: StringKind,
    hash: u32,
) -> GcRef<LuaStringImpl> {
    // Creates a bare Rc<...>, never registered with any GC tracking list;
    // harmless in practice since every caller of this module keeps its
    // result alive permanently anyway (see the module doc).
    let _ = state; // state needed for GC registration in Phase D
    GcRef::new(LuaStringImpl {
        hash: Cell::new(hash),
        extra: Cell::new(0),
        bytes: Rc::from(bytes),
        kind,
    })
}

/// Grow the string intern table.
///
/// C first attempts a full GC collection (`luaC_fullgc`) when the table is
/// at its absolute maximum size, in case that frees up some short strings;
/// this does not, and goes straight to the OOM error. In practice `nuse`
/// never approaches `i32::MAX` here, since this pool only ever holds the
/// one bootstrap OOM message (see the module doc).
fn grow_str_tab(state: &mut LuaState) -> Result<(), LuaError> {
    let nuse = state.global().strt.nuse;
    if nuse == i32::MAX as usize {
        if state.global().strt.nuse == i32::MAX as usize {
            return Err(LuaError::Memory);
        }
    }

    let size = state.global().strt.size;
    if size <= MAX_STR_TAB / 2 {
        resize(state, size * 2);
    }

    Ok(())
}

/// Look up `bytes` in the intern table; create and insert a new short string if
/// not found.
///
/// C's bucket lookup walks an intrusive `hnext` chain and, on a hit, checks
/// `isdead`/`changewhite` to resurrect a dead-but-not-yet-swept entry.
/// `HashMap::get` replaces the chain walk; the resurrection check has no
/// equivalent because `LuaStringImpl` values are plain `Rc`-held and kept
/// alive by reference count, not tracked by the collector (see the module
/// doc), so there is no dead-but-not-collected state to resurrect from.
fn intern_short_str(state: &mut LuaState, bytes: &[u8]) -> Result<GcRef<LuaStringImpl>, LuaError> {
    let seed = state.global().seed;
    let h = hash_bytes(bytes, seed);

    // Clone the existing GcRef<LuaStringImpl> so the immutable borrow on
    // `state` ends before any mutable access below.
    let existing = state.global().strt.map.get(bytes).cloned();
    if let Some(ts) = existing {
        return Ok(ts);
    }

    let needs_grow = {
        let strt = &state.global().strt;
        strt.nuse >= strt.size
    };
    if needs_grow {
        grow_str_tab(state)?;
    }

    let ts = create_str_obj(state, bytes, StringKind::Short, h);

    state
        .global_mut()
        .strt
        .map
        .insert(bytes.to_vec().into_boxed_slice(), ts.clone());
    state.global_mut().strt.nuse += 1;

    Ok(ts)
}

use lua_types::LuaError;
