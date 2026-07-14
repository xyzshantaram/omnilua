//! Shared string constants and hashing utility.
//!
//! This module used to host a standalone `LuaStringImpl`/`StringPool`
//! bootstrap string table (`TString`/`stringtable` from `lstring.c`), built
//! solely to construct the pre-allocated out-of-memory message during VM
//! startup. That subsystem was verified dead (issue #274): `GlobalState`
//! bootstrap now calls `LuaState::intern_str` — the same interning path
//! every other Lua string in the VM uses, backed by
//! `GlobalState::interned_lt` — directly (see `state.rs::init_memerrmsg`).
//! What remains here is the pure hash function and the short/long-string
//! length threshold, both still shared with that real path and with the
//! VM's seed generation.

/// Upper bound (in bytes) on a "short" (interned) Lua string; longer strings
/// are always long strings and are never interned. Mirrors C's
/// `LUAI_MAXSHORTLEN`.
pub(crate) const MAX_SHORT_LEN: usize = 40;

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
