//! `LuaString` — Lua's byte-string (NOT UTF-8). PORT_STRATEGY §3.3.
//!
//! Phase A-C: a simple `Box<[u8]>`-backed struct with a short/long flag.
//! Phase D may revisit for interning + content-hash equality.

/// Lua's immutable byte-string value.
///
/// The byte payload is a `Box<[u8]>`, NOT an `Rc<[u8]>`. Strings are immutable
/// and GC-owned: every live `LuaString` is reached through a `GcRef<LuaString>`
/// (the interner stores `GcRef`s, `LuaValue::Str` holds a `GcRef`), and all
/// value-level sharing happens at that `GcRef` layer. An `Rc<[u8]>` would
/// co-locate a 16-byte refcount header (strong + weak counts) with the payload
/// in the string's heap allocation, so every string allocation paid those 16
/// bytes on top of its `GcBox<LuaString>` for a refcount machinery nothing
/// uses. Switching to `Box<[u8]>` drops the 16-byte header per string and the
/// refcount inc/dec traffic; the win is in the heap allocation, not the struct
/// field (both `Rc<[u8]>` and `Box<[u8]>` are 16-byte fat pointers).
///
/// The `#[derive(Clone)]` is retained, but a by-value `LuaString` clone is now
/// a deep copy (alloc + memcpy) rather than a refcount bump. This is acceptable
/// because no hot path clones a `LuaString` by value — hot sharing goes through
/// the `Copy` `GcRef<LuaString>` handle. The only by-value clones are cold
/// (error-message construction, `GlobalState` init).
#[derive(Debug, Clone)]
pub struct LuaString {
    bytes: Box<[u8]>,
    is_short: bool,
    hash: u32,
}

impl LuaString {
    pub fn from_bytes(b: Vec<u8>) -> Self {
        let is_short = b.len() <= 40;
        let hash = Self::hash_bytes(&b, 0);
        LuaString {
            bytes: b.into_boxed_slice(),
            is_short,
            hash,
        }
    }

    /// Construct directly from a borrowed slice with a single allocating copy.
    ///
    /// `from_bytes` takes an owned `Vec<u8>`, but `Vec<u8> -> Box<[u8]>` via
    /// `into_boxed_slice` only adopts the existing buffer when it is exactly
    /// full, otherwise it reallocates; a caller holding only a slice would copy
    /// twice (once into a `Vec`, once into the `Box`). `Box::from(&[u8])` copies
    /// the slice straight into the final allocation, matching C's single
    /// `luaS_newlstr` allocation per string. Hash is computed with the same
    /// algorithm as `from_bytes`.
    pub fn from_slice(b: &[u8]) -> Self {
        let is_short = b.len() <= 40;
        let hash = Self::hash_bytes(b, 0);
        LuaString {
            bytes: Box::from(b),
            is_short,
            hash,
        }
    }

    pub fn placeholder() -> Self {
        Self::from_bytes(Vec::new())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn is_short(&self) -> bool {
        self.is_short
    }
    pub fn is_long(&self) -> bool {
        !self.is_short
    }
    pub fn hash(&self) -> u32 {
        self.hash
    }
    pub fn buffer_bytes(&self) -> usize {
        self.bytes.len() + 2 * std::mem::size_of::<usize>()
    }

    pub fn is_reserved_word(&self) -> bool {
        // TODO(port): proper reserved-word check via lexer's token enum.
        false
    }

    pub fn hash_bytes(bytes: &[u8], seed: u32) -> u32 {
        // Stub WyHash. Real impl ports bun_wyhash. Stable for now so
        // intern-table equality works.
        let mut h: u32 = seed.wrapping_add(0x9e3779b9);
        for &b in bytes {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        h
    }

    pub fn hash_long(&mut self) -> u32 {
        self.hash
    }
}

impl PartialEq for LuaString {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}
impl Eq for LuaString {}
