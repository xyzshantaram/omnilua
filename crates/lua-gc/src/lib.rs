//! Lua 5.4 garbage collector.
//!
//! Modules:
//!   heap — Phase-D production mark-sweep (Gc<T>, Trace, Heap)
//!   gc   — legacy partial port of lgc.c (reference; not used by runtime)
//!   mem  — legacy partial port of lmem.c (reference; not used by runtime)

pub mod heap;
pub mod gc;
pub mod mem;

pub use heap::{Color, Gc, GcBox, GcHeader, GcState, Heap, Marker, Trace};

// ──────────────────────────────────────────────────────────────────────────
// PORT STATUS
//   source:        (module aggregator)
//   target_crate:  lua-gc
//   confidence:    high
//   notes:         per-file ports own their own trailers
// ──────────────────────────────────────────────────────────────────────────
