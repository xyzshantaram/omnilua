//! Lua 5.4 garbage collector.
//!
//! Modules:
//!   heap — Phase-D production mark-sweep (Gc<T>, Trace, Heap)
//!
//! Historical Phase-A partial ports of C-Lua's `lgc.c`/`lmem.c` were removed
//! once `heap.rs` became the production collector. Keep this crate's source
//! tree limited to compiled modules so unsafe audits reflect the active build.

pub mod heap;

pub use heap::{
    detached_allocations, with_current_heap, BootstrapScope, Color,
    FinalizerEntry, FinalizerRegistry, FinalizerRegistryStats, Gc, GcAge, GcBox, GcHeader, GcState,
    Heap, HeapGuard, HeapRef, Marker, StepBudget, StepOutcome, Trace, Udata51Probe, WeakEntry,
    WeakListKind, WeakRegistry, WeakRegistrySnapshot, WeakRegistryStats,
};
