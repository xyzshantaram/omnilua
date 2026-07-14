//! Buffered streams — Rust port of `lzio.c` + `lzio.h`.
//!
//! Provides two public types:
//! - [`ZIO`]: a read cursor wrapping an external chunk-supplier callback.
//! - [`LexBuffer`]: a growable `Vec<u8>` byte buffer with the named interface
//!   that C code accessed through the `luaZ_*buffer*` macro family.
//!
//! `lzio.h`'s macros are translated at their call sites and collected as
//! methods or constants in this module.

use crate::state::LuaState;
use lua_types::error::LuaError;

// ── Constants ──────────────────────────────────────────────────────────────────

/// End-of-stream sentinel returned by [`ZIO::getc`] and [`ZIO::fill`].
pub(crate) const EOZ: i32 = -1;

/// Reentrant chunk supplier for a [`ZIO`].
///
/// Mirrors C's `lua_Reader`: the reader is invoked with the live `lua_State`
/// each time more bytes are needed, so a `load` reader written in Lua can call
/// back into the interpreter mid-parse. `Ok(None)` signals end-of-stream; an
/// `Err` aborts the parse with that error (the C reader equivalent is a
/// longjmp). Bytes are owned (`Vec<u8>`) rather than borrowed because a `dyn`
/// trait object cannot name the reader's internal-buffer lifetime.
pub type ChunkReader = Box<dyn FnMut(&mut LuaState) -> Result<Option<Vec<u8>>, LuaError>>;

// ── LexBuffer (was Mbuffer in C) ───────────────────────────────────────────────

/// Growable byte buffer used by the lexer for token text accumulation.
///
/// Corresponds to `Mbuffer` in `lzio.h`.  The C struct tracked `buffer`,
/// `n` (used length), and `buffsize` (allocated capacity) as three separate
/// fields with manual realloc.  In Rust all three are implicit in `Vec<u8>`.
pub struct LexBuffer {
    buffer: Vec<u8>,
}

impl LexBuffer {
    /// Construct an empty `LexBuffer`.  Corresponds to the `luaZ_initbuffer` macro.
    pub fn new() -> Self {
        LexBuffer { buffer: Vec::new() }
    }

    /// Return the buffer contents as a mutable byte slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    /// Return the buffer's current allocation capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }

    /// Return the number of valid bytes currently stored in the buffer.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Shorten the live contents by `i` bytes without releasing capacity.
    pub fn truncate_by(&mut self, i: usize) {
        let new_len = self.buffer.len().saturating_sub(i);
        self.buffer.truncate(new_len);
    }

    /// Reset the live length to zero without releasing capacity.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Resize the buffer to exactly `size` bytes, filling new bytes with `0`.
    ///
    /// Returns `Err(LuaError::Memory)` on allocation failure.
    ///
    /// C's `luaZ_resizebuffer` macro routes through `luaM_reallocvchar` and
    /// Lua's custom allocator; here `Vec::resize` uses Rust's global allocator,
    /// and OOM propagation via a custom allocator is not implemented.
    pub fn resize(&mut self, _state: &mut LuaState, size: usize) -> Result<(), LuaError> {
        self.buffer.resize(size, 0u8);
        Ok(())
    }

    // `Drop for Vec` releases the heap allocation automatically. Call sites
    // that use `luaZ_freebuffer` can simply let the `LexBuffer` drop.
}

impl Default for LexBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ── ZIO (buffered input stream) ────────────────────────────────────────────────

/// Buffered input stream wrapping an external chunk-reader callback.
///
/// Corresponds to `struct Zio` / `ZIO` in `lzio.h`.  The C struct stored a
/// `lua_State *L` back-pointer and a `void *data` opaque pointer alongside a
/// raw `lua_Reader` function pointer.  In Rust:
///
/// - `lua_State *L` is removed from the struct; callers hold `&mut LuaState`
///   directly and pass it to fallible methods.
/// - `void *data` is folded into the reader closure.
/// - `const char *p` (raw pointer into the reader's internal buffer) becomes a
///   `usize` index into the owned `current_chunk` field.
pub struct ZIO {
    n: usize,
    // Raw pointer replaced by index into `current_chunk`.
    p: usize,
    // C's reader function pointer + void *data collapsed into one closure. C
    // stored `lua_State *L` in the ZIO; here it's threaded through
    // fill/getc/read instead so the borrow checker sees the access.
    reader: ChunkReader,
    // Owned current chunk returned by the reader.  Not present as a separate
    // field in C (C held a raw pointer into the reader's own internal buffer).
    current_chunk: Vec<u8>,
}

impl ZIO {
    /// Initialise a `ZIO` with the given reentrant reader callback.
    ///
    /// Corresponds to `luaZ_init` in `lzio.c`.  The C parameters `reader` and
    /// `data` are combined into a single closure; `L` is threaded through the
    /// fallible methods rather than stored on the struct.
    pub fn new(reader: ChunkReader) -> Self {
        ZIO {
            n: 0,
            p: 0,
            current_chunk: Vec::new(),
            reader,
        }
    }

    /// Construct a `ZIO` that yields the supplied bytes once and then EOZ.
    ///
    /// Used for in-memory sources (a string chunk, or the lexer's own unit
    /// tests) where there is no reader to call back into Lua. The state passed
    /// to `getc`/`fill` is ignored by this reader.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut once = Some(bytes);
        ZIO::new(Box::new(move |_state| Ok(once.take())))
    }

    /// Move this stream out, leaving an exhausted (empty) `ZIO` in its place.
    ///
    /// The parser owns the lexer's `LexState`, which owns its `ZIO`; the loader
    /// only holds a `&mut ZIO`. This hands the live stream — its reader and any
    /// bytes already buffered by [`getc`] — to the parser by value so the lexer
    /// can keep pulling from the same reader (and the same `&mut LuaState`)
    /// on demand. The original slot becomes an immediately-EOZ stream.
    pub fn take(&mut self) -> ZIO {
        std::mem::replace(self, ZIO::from_bytes(Vec::new()))
    }

    /// Refill the internal buffer by invoking the reader callback; return the
    /// first byte of the new chunk as an `i32`, or [`EOZ`] if no more data is
    /// available.
    ///
    /// `lua_unlock`/`lua_lock` are no-ops in the default C build and have no
    /// equivalent call here. A reader error propagates as `Err` (C longjmps
    /// out of the reader).
    pub(crate) fn fill(&mut self, state: &mut LuaState) -> Result<i32, LuaError> {
        let chunk_opt = (self.reader)(state)?;

        match chunk_opt {
            None => Ok(EOZ),
            Some(chunk) if chunk.is_empty() => Ok(EOZ),
            Some(chunk) => {
                self.n = chunk.len() - 1;
                self.current_chunk = chunk;
                self.p = 0;
                let byte = self.current_chunk[self.p] as u8;
                self.p += 1;
                Ok(byte as i32)
            }
        }
    }

    /// Return the next byte from the stream as an `i32`, or [`EOZ`] at
    /// end-of-stream.
    ///
    /// This is the hot-path inline method corresponding to the `zgetc` macro.
    /// When bytes remain in the current chunk no allocation occurs.
    ///
    /// C's macro uses `(z)->n-- > 0`, which reads n, tests it, then
    /// decrements; when n == 0 the test is false (0 > 0) so fill is called
    /// without decrementing. This preserves that: `if self.n > 0` followed by
    /// an explicit `self.n -= 1`.
    #[inline]
    pub fn getc(&mut self, state: &mut LuaState) -> Result<i32, LuaError> {
        if self.n > 0 {
            self.n -= 1;
            let byte = self.current_chunk[self.p] as u8;
            self.p += 1;
            Ok(byte as i32)
        } else {
            self.fill(state)
        }
    }

    /// Read exactly `buf.len()` bytes into `buf`.
    ///
    /// Returns the number of bytes that could **not** be read: `0` means
    /// complete success; a non-zero value means end-of-stream was reached with
    /// that many bytes still outstanding.
    ///
    /// C's `void *b` + explicit `n` become Rust's `&mut [u8]`, whose
    /// length encodes the requested byte count.  `memcpy` becomes
    /// `copy_from_slice`.  The advancing pointer `b = (char *)b + m` is
    /// replaced by a `dst` index into `buf`.
    pub(crate) fn read(&mut self, state: &mut LuaState, buf: &mut [u8]) -> Result<usize, LuaError> {
        let mut remaining = buf.len();
        let mut dst: usize = 0;

        while remaining > 0 {
            if self.n == 0 {
                if self.fill(state)? == EOZ {
                    return Ok(remaining);
                } else {
                    // fill() advanced p by 1 and set n = chunk.len() - 1.
                    // Undoing that makes the whole chunk available to the
                    // copy loop below.
                    self.n += 1;
                    self.p -= 1;
                }
            }

            let m = remaining.min(self.n);

            buf[dst..dst + m].copy_from_slice(&self.current_chunk[self.p..self.p + m]);

            self.n -= m;
            self.p += m;

            dst += m;
            remaining -= m;
        }

        Ok(0)
    }
}
