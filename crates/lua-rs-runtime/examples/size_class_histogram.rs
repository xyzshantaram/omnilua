//! size_class_histogram — #113 measure-first analysis tool (READ-ONLY).
//!
//! Banked lesson from the #113 arc (see `docs/PERF_EVIDENCE_113_W2_OWNERVEC`
//! and the "owner-vector negative" section of `docs/PERFORMANCE_PRINCIPLES`):
//! **allocator size-class crossings are the real RSS mechanism**, not raw
//! byte counts. A 16-byte shrink that stays inside one malloc bucket buys
//! nothing; an 8-byte shrink that crosses a bucket boundary buys the whole
//! step × the live population. This tool turns that lesson into numbers.
//!
//! What it does, for one bench workload per process invocation:
//!   1. Installs a global allocator that maintains a live size-histogram
//!      (per 8-byte bucket) plus a snapshot taken at the moment of peak live
//!      bytes (a peak-RSS proxy). No allocation happens inside the hook, so
//!      it is reentrancy-safe.
//!   2. Runs the workload through the real omniLua VM (`Lua::load().exec()`).
//!   3. Prints:
//!      - `malloc_good_size` probes documenting the macOS libmalloc class
//!        table on THIS machine (authoritative rounding, not a hardcode).
//!      - `size_of::<GcBox<T>>()` for every core GC-boxed payload type, the
//!        libmalloc class it lands in, the next-smaller class boundary, and
//!        the bytes that must be removed to cross it.
//!      - The peak-moment live histogram, so each GcBox size is annotated
//!        with its measured peak population.
//!
//! Usage: `cargo run -q --release -p omnilua --example size_class_histogram \
//!         -- harness/bench/workloads/closure_ops.lua`


/// The probe target (`malloc_good_size`) is macOS libmalloc — the class
/// table this tool documents does not exist elsewhere, so every other
/// platform gets a stub entry point instead of a link error (`cargo test`
/// builds examples on all platforms; see the Windows release gate).
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("size_class_histogram probes the macOS libmalloc class table; run it on macOS.");
}

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicUsize, Ordering};

    use lua_gc::GcBox;
    use lua_types::closure::{LuaCClosure, LuaLClosure};
    use lua_types::proto::LuaProto;
    use lua_types::string::LuaString;
    use lua_types::table::{LuaTable, TableInner, TableNode};
    use lua_types::upval::UpVal;
    use lua_types::userdata::LuaUserData;
    use lua_types::value::{LuaThread, LuaValue};
    use lua_vm::state::LuaState;
    use omnilua::Lua;

    extern "C" {
        /// macOS libmalloc: returns the actual allocation size (rounded up to the
        /// size class) that a `malloc(size)` would reserve. This is the ground
        /// truth for size-class rounding on this platform.
        fn malloc_good_size(size: usize) -> usize;
    }

    fn good(size: usize) -> usize {
        if size == 0 {
            return 0;
        }
        // SAFETY: pure query into libSystem, no aliasing/lifetime concerns.
        unsafe { malloc_good_size(size) }
    }

    /// Lower edge of the size class that `size` lands in: the smallest request
    /// whose `good` size equals `good(size)`. Everything strictly below that edge
    /// falls into a smaller (cheaper) class.
    fn class_lower_edge(size: usize) -> usize {
        let cap = good(size);
        let (mut lo, mut hi) = (1usize, size);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if good(mid) < cap {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// The cap of the next-smaller class below `size`'s class (0 if none).
    fn prev_class_cap(size: usize) -> usize {
        let edge = class_lower_edge(size);
        if edge <= 1 {
            0
        } else {
            good(edge - 1)
        }
    }

    /// Exact-byte histogram: one bucket per request size. Keying on the exact
    /// `Layout::size()` (not an 8-byte range) means "bucket 56" is *exactly* the
    /// 56-byte requests, so a `GcBox<T>` whose `size_of` is 56 shares its bucket
    /// only with other exactly-56-byte allocations — not the [56,63] range. This
    /// removes the range-aggregation ambiguity (codex R1 finding #2). Attribution
    /// is still by size, not by type; workload structure supplies the type.
    const GRANULARITY: usize = 1;
    const NUM_BUCKETS: usize = 8192; // exact sizes 0..8192 B; larger -> overflow

    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO_U32: AtomicU32 = AtomicU32::new(0);

    static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);
    static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
    static LIVE: [AtomicU32; NUM_BUCKETS] = [ZERO_U32; NUM_BUCKETS];
    static SNAP: [AtomicU32; NUM_BUCKETS] = [ZERO_U32; NUM_BUCKETS];
    /// Live count of allocations larger than the tracked range.
    static LIVE_OVERFLOW: AtomicU32 = AtomicU32::new(0);
    static SNAP_OVERFLOW: AtomicU32 = AtomicU32::new(0);
    /// Peak-bytes value at which the last snapshot was taken; used to throttle
    /// snapshotting to once per `SNAP_DELTA` growth so the exact-size copy stays
    /// cheap without missing the true peak by more than the delta.
    static SNAP_AT: AtomicUsize = AtomicUsize::new(0);
    const SNAP_DELTA: usize = 4096;
    /// Set true while `main` is emitting the report, so its own allocations are
    /// not folded into the workload histogram.
    static REPORTING: AtomicUsize = AtomicUsize::new(0);

    fn bucket_of(size: usize) -> Option<usize> {
        let b = size / GRANULARITY;
        if b < NUM_BUCKETS {
            Some(b)
        } else {
            None
        }
    }

    fn record_alloc(size: usize) {
        if REPORTING.load(Ordering::Relaxed) != 0 {
            return;
        }
        match bucket_of(size) {
            Some(b) => {
                LIVE[b].fetch_add(1, Ordering::Relaxed);
            }
            None => {
                LIVE_OVERFLOW.fetch_add(1, Ordering::Relaxed);
            }
        }
        let now = (LIVE_BYTES.fetch_add(size as isize, Ordering::Relaxed) + size as isize) as usize;
        if now > PEAK_BYTES.load(Ordering::Relaxed) {
            PEAK_BYTES.store(now, Ordering::Relaxed);
            // Snapshot the whole live histogram at this new peak, throttled to once
            // per SNAP_DELTA of growth. No allocation happens here (fixed static
            // arrays), so this is safe inside the allocator hook.
            if now >= SNAP_AT.load(Ordering::Relaxed) + SNAP_DELTA {
                SNAP_AT.store(now, Ordering::Relaxed);
                for i in 0..NUM_BUCKETS {
                    SNAP[i].store(LIVE[i].load(Ordering::Relaxed), Ordering::Relaxed);
                }
                SNAP_OVERFLOW.store(LIVE_OVERFLOW.load(Ordering::Relaxed), Ordering::Relaxed);
            }
        }
    }

    fn record_free(size: usize) {
        if REPORTING.load(Ordering::Relaxed) != 0 {
            return;
        }
        match bucket_of(size) {
            Some(b) => {
                LIVE[b].fetch_sub(1, Ordering::Relaxed);
            }
            None => {
                LIVE_OVERFLOW.fetch_sub(1, Ordering::Relaxed);
            }
        }
        LIVE_BYTES.fetch_sub(size as isize, Ordering::Relaxed);
    }

    struct HistAllocator;

    // SAFETY: every operation delegates to `System` unchanged; the added
    // bookkeeping touches only fixed static atomics and never allocates, so it
    // cannot re-enter the allocator.
    unsafe impl GlobalAlloc for HistAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(layout) };
            if !p.is_null() {
                record_alloc(layout.size());
            }
            p
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) };
            record_free(layout.size());
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc_zeroed(layout) };
            if !p.is_null() {
                record_alloc(layout.size());
            }
            p
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let p = unsafe { System.realloc(ptr, layout, new_size) };
            if !p.is_null() {
                record_free(layout.size());
                record_alloc(new_size);
            }
            p
        }
    }

    #[global_allocator]
    static ALLOC: HistAllocator = HistAllocator;

    /// One row in the size-of/size-class table.
    struct TypeRow {
        name: &'static str,
        payload: usize,
        boxed: usize,
    }

    fn row<T>(name: &'static str) -> TypeRow {
        TypeRow {
            name,
            payload: std::mem::size_of::<T>(),
            boxed: std::mem::size_of::<GcBox<T>>(),
        }
    }

    fn snap_pop(size: usize) -> u32 {
        match bucket_of(size) {
            Some(b) => SNAP[b].load(Ordering::Relaxed),
            None => 0,
        }
    }

    pub(super) fn run() {
        let path = std::env::args().nth(1).expect("usage: size_class_histogram <workload.lua>");
        let source = std::fs::read(&path).expect("read workload");

        {
            let lua = Lua::new();
            lua.load(&source).exec().expect("workload exec failed");
        }

        // Freeze the histogram: from here on, only the reporter allocates.
        REPORTING.store(1, Ordering::Relaxed);

        let rows = [
            row::<LuaTable>("LuaTable"),
            row::<LuaString>("LuaString"),
            row::<UpVal>("UpVal"),
            row::<LuaLClosure>("LuaLClosure"),
            row::<LuaCClosure>("LuaCClosure"),
            row::<LuaProto>("LuaProto"),
            row::<LuaUserData>("LuaUserData"),
            row::<LuaThread>("LuaThread"),
            row::<LuaState>("LuaState"),
        ];

        println!("# size_class_histogram");
        println!("workload : {path}");
        println!("peak live bytes (proxy for peak RSS) : {} bytes ({:.2} MiB)",
                 PEAK_BYTES.load(Ordering::Relaxed),
                 PEAK_BYTES.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0));
        println!("pointer width : {} bits", usize::BITS);
        println!();

        println!("## macOS libmalloc size-class probes (malloc_good_size, authoritative)");
        println!("{:>8}  {:>10}", "request", "good_size");
        let mut probe = 16usize;
        let mut last = 0;
        while probe <= 1024 {
            let g = good(probe);
            if g != last {
                println!("{probe:>8}  {g:>10}");
                last = g;
            }
            probe += 8;
        }
        for &p in &[1536usize, 2048, 3072, 4096, 8192, 16384] {
            println!("{p:>8}  {:>10}", good(p));
        }
        println!();

        println!("## GcBox<T> sizes vs the size-class table (GcHeader = {} B)",
                 std::mem::size_of::<lua_gc::GcHeader>());
        println!(
            "{:<14} {:>7} {:>7} {:>7} {:>9} {:>10} {:>10} {:>10}",
            "type", "payld", "GcBox", "class", "prevcls", "cross(B)", "step(B)", "peakpop"
        );
        for r in &rows {
            let cls = good(r.boxed);
            let prev = prev_class_cap(r.boxed);
            let cross = r.boxed.saturating_sub(prev);
            let step = cls.saturating_sub(prev);
            println!(
                "{:<14} {:>7} {:>7} {:>7} {:>9} {:>10} {:>10} {:>10}",
                r.name, r.payload, r.boxed, cls, prev, cross, step, snap_pop(r.boxed)
            );
        }
        println!();
        println!("legend: class=libmalloc class cap for GcBox; prevcls=next-smaller class cap;");
        println!("        cross(B)=bytes to remove from GcBox to drop below prevcls (enter cheaper class);");
        println!("        step(B)=RSS bytes/object reclaimed by crossing; peakpop=live count at peak.");
        println!();

        println!("## helper-type sizes (context)");
        println!("LuaValue = {} B, TableNode = {} B, TableInner = {} B",
                 std::mem::size_of::<LuaValue>(),
                 std::mem::size_of::<TableNode>(),
                 std::mem::size_of::<TableInner>());
        println!();

        println!("## peak-moment live histogram (size buckets with >=8 live objects)");
        println!("{:>8} {:>10} {:>12} {:>16}", "size", "good_size", "peak_count", "peak_bytes(good)");
        let mut total_tracked_bytes = 0usize;
        for b in 0..NUM_BUCKETS {
            let c = SNAP[b].load(Ordering::Relaxed);
            if c >= 8 {
                let size = b * GRANULARITY;
                let g = good(size.max(1));
                let bytes = g * c as usize;
                total_tracked_bytes += bytes;
                println!("{size:>8} {g:>10} {c:>12} {bytes:>16}");
            }
        }
        let of = SNAP_OVERFLOW.load(Ordering::Relaxed);
        if of > 0 {
            println!("(>{} B overflow bucket: {} live allocations)", NUM_BUCKETS * GRANULARITY, of);
        }
        println!();
        println!("tracked good-size bytes across printed buckets: {total_tracked_bytes} \
                  ({:.2} MiB)", total_tracked_bytes as f64 / (1024.0 * 1024.0));
    }
}
