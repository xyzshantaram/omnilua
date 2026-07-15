# #113 size-class candidate ranking (measure-first, read-only) — 2026-07-15

Analysis-only packet for issue #113 (RSS object diet). **No collector or
runtime source was modified and no object layout was changed.** The
deliverables are a new measurement tool and this ranked go/no-go verdict.

## Why this packet exists

The #113 arc's two banked lessons (`docs/PERF_EVIDENCE_113_W2_OWNERVEC_20260713.md`,
`docs/PERFORMANCE_PRINCIPLES.md` "Patterns from the owner-vector negative"):

1. **Relocation is not removal.** Wave 2 moved the 16 B intrusive link into an
   owner vector; on 64-bit that slot is a 16 B fat pointer — the same 16 B —
   plus slack, so RSS went *up* on binarytrees. Closed unmerged.
2. **Allocator size-class crossings are the real RSS mechanism.** Every RSS win
   in the arc came from a malloc bucket boundary (UpVal 104→72, GcBox<UpVal>
   56→40 in the W2 branch), never from raw byte counts. A 16 B shrink that
   stays inside one malloc bucket buys nothing; an 8 B shrink that crosses a
   boundary buys the whole step × the live population.

So before any more layout surgery: dump the live-object size histogram against
the platform allocator's real class table and rank shrink candidates by
**(crosses-a-bucket) × (bytes-saved-per-crossing) × (peak population)**.

## The tool

`crates/lua-rs-runtime/examples/size_class_histogram.rs`, driven by
`harness/size_class/run.sh`. Per bench workload, in its own process:

- A `#[global_allocator]` maintains a live size-histogram (8-byte buckets) and
  snapshots the whole histogram at the instant of **peak live bytes** (a
  peak-RSS proxy). The hook allocates nothing, so it is reentrancy-safe.
- The workload runs through the real omniLua VM (`Lua::load().exec()`).
- It prints: the **authoritative** macOS libmalloc class table via
  `malloc_good_size` (probed live, not hardcoded); `size_of::<GcBox<T>>()` for
  every core GC-boxed payload type with the class it lands in, the next-smaller
  class cap, the bytes to remove to cross, and the RSS bytes reclaimed by
  crossing; and the peak-moment histogram annotated with populations.

Raw output for every workload is under `harness/size_class/out/`.

## macOS libmalloc class table (this machine, `malloc_good_size`)

Contrary to the "16-byte quanta to 256, then 512" rule of thumb, this machine's
libmalloc uses **16-byte quanta continuously from 16 up to 1024** (the tiny
region), then **512-byte quanta above 1024** (small region: 1024, 1536, 2048,
…), then page-rounded. Verified by probing every 8 bytes:

```
request  good_size        request  good_size
     16         16              72         80
     24         32              88         96
     40         48             104        112
     56         64            1008       1008
     64         64            1016       1024
                              1536       1536
```

The tool re-derives this on whatever machine it runs, so the boundary math is
never a stale hardcode. All GcBox sizes below sit in the 16-quantum tiny
region, so **every crossing is worth exactly 16 bytes per object.**

## GcBox<T> at the current tree (GcHeader = 24 B, Wave 1 landed)

`GcHeader` is 24 B: `color`(1) + `age`(1) + `flags`(1) + pad(1) + `size:u32`(4)
+ `next`(16, a `dyn Trace` fat pointer). `GcBox<T> = GcHeader + T`, 8-aligned.
Measured `size_of` and class placement (identical across all workloads — layout
is workload-independent, only population varies):

| GcBox\<T\>          | payload | GcBox | class | next-smaller cap | slack | bytes to cross | peak pop (which workload) |
|---|---:|---:|---:|---:|---:|---:|---|
| **UpVal**           | 32 | **56** | **64** | 48 | **8** | **8** | **100,001 (closure_ops)** |
| LuaLClosure         | 24 | 48 | 48 | 32 | 0 | 16 | ~100,000 (closure_ops) |
| LuaString           | 24 | 48 | 48 | 32 | 0 | 16 | 165,753 (table_hash_pressure) |
| **LuaTable**        | 72 | **96** | **96** | 80 | 0 | 16 | **131,331 (binarytrees)** |
| LuaCClosure         | 40 | 64 | 64 | 48 | 0 | 16 | ~19 |
| LuaUserData         | 88 | 112 | 112 | 96 | 0 | 16 | 3 |
| LuaProto            | 208 | 232 | 240 | 224 | 8 | 8 | 3 |
| LuaState            | 176 | 200 | 208 | 192 | 8 | 8 | 1 |
| LuaThread           | 8 | 32 | 32 | 16 | 0 | 16 | 1 (real) |

`slack` = class cap − GcBox size (bytes already wasted to rounding).
`bytes to cross` = GcBox size − next-smaller cap (payload bytes to remove to
drop into the cheaper class). Crossing reclaims 16 B/object.

### Population-attribution caveat (important for reading the table)

The tool's peak-population column keys on allocation size, so **same-size
GcBox types collide in one bucket** and the raw count is the bucket total. Two
collisions matter:

- **48-byte bucket = GcBox\<LuaLClosure\> AND GcBox\<LuaString\>.** In
  closure_ops it is LClosure-dominated (100k closures created); in
  table_hash_pressure it is LuaString-dominated (165k distinct keys). Populations
  above are attributed by workload structure, not by the raw bucket number.
- **32-byte bucket ≠ GcBox\<LuaThread\>.** The raw count (120–160k in
  table_hash_pressure) is non-GcBox 32-byte allocations (buffers). Real
  `LuaThread` population is 1 (no coroutines in these workloads); its table row
  is a size-of fact, not a live count.

The two rows that matter for ranking — UpVal (bucket 56) and LuaTable (bucket
96) — occupy their buckets **cleanly** (no other GcBox is that size, confirmed
against the histogram), so their populations are exact.

## Per-candidate verdicts

### 1. GcBox\<UpVal\>  →  **WORTH-IT**  (the only qualifying candidate)

- **State:** 56 B, sits in the 64-B class wasting **8 B of slack** — the *only*
  high-population GcBox with allocator slack. Peak population **100,001** on
  closure_ops (a named #113 tall pole, still above the ≤2.0× done-condition).
- **The crossing:** shrink UpVal 32 → 24 B ⇒ GcBox 56 → 48 B ⇒ crosses the
  64-B class into the 48-B class. This lands in the **same 48-B class** the
  killed Wave-2 header diet would have reached (its "GcBox<UpVal> 56→40", since
  `good(40)=48`) — but via a payload shrink, with **no header surgery and no
  owner-vector relocation cost.** It is the safe path to the exact win W2 chased.
- **The exact change (for the implement lane, not done here):** `UpVal`
  (`crates/lua-types/src/upval.rs`) is `open_thread_id: Cell<i64>`(8) +
  `open_idx: Cell<u32>`(4) + pad(4) + `closed_value: Cell<LuaValue>`(16) = 32 B.
  Change `open_thread_id` to `Cell<u32>` and use `u32::MAX` as the `CLOSED_TAG`
  sentinel instead of `-1`. New layout: `u32`(4) + `u32`(4) + `LuaValue`(16) =
  **24 B, no padding.** Thread ids are a small monotonic counter (main = 0);
  they cannot reach `u32::MAX` (that is 4 billion live coroutines), so the
  sentinel is unambiguous and the range is safe. `closed_value` (16 B) and
  `open_idx` are load-bearing and untouched.
- **RSS payoff:** 16 B/object × 100,001 = **1,600,016 B ≈ 1.53 MiB** on
  closure_ops (peak live ≈ 14.9 MiB tracked; process max-RSS historically
  25–37 MB, so ≈ 4–8 %). The precedent for a same-magnitude UpVal diet (June's
  104→72) translated ≈ 1:1 to −8.3 % process RSS.
- **Favorable asymmetry:** the pacer is charged only the payload delta (−8 B ×
  100k = −0.8 MB), but the allocator returns the full class step (−16 B × 100k
  = −1.5 MB). So cadence barely shifts (the "shrinking charged bytes shifts
  cadence" hazard is muted) while RSS drops more than the charged delta — this
  is precisely the "target the boundary" lesson paying off.
- **Risk:** low. One type, one field, safe-Rust, sentinel-preserving. Gate with
  the multiversion oracle + GC canaries + a `size_of::<GcBox<UpVal>>() == 48`
  compile-time assert, and **confirm the RSS number empirically** with heap-diff
  + interleaved max-RSS per `docs/MEASUREMENT_PROTOCOL.md` before landing (the
  1.53 MiB is a projection until measured).

### 2. GcBox\<LuaLClosure\>  →  **NOT-WORTH-IT**

48 B, **fills the 48-B class exactly (0 slack)**. Crossing to the 32-B class
needs payload 24 → 8 B (−16 B). Payload is `proto: GcRef`(8) + `upvals:
Box<[Cell<GcRef<UpVal>>]>`(16, ptr+len); both are irreducible (already
`Box<[_]>`, already thin `GcRef`). No 16 B is removable. Population is high on
closure_ops but the shrink is infeasible.

### 3. GcBox\<LuaString\>  →  **NOT-WORTH-IT**

48 B, fills the 48-B class exactly. Crossing needs payload 24 → 8 B. Payload is
`bytes: Box<[u8]>`(16, ptr+len — irreducible), `is_short: bool`, `hash: u32`.
The fat pointer alone is 16 B; the type cannot reach 8 B without giving up
owning its bytes. Dominant on table_hash_pressure, but that workload already
meets the ≤2.0× done-condition (1.75×). Not addressable by a size shrink.

### 4. GcBox\<LuaTable\>  →  **NOT-WORTH-IT** (biggest population, no feasible crossing)

96 B, **fills the 96-B class exactly (0 slack)**, peak population **131,331** on
binarytrees — the single largest GcBox population in the suite. But crossing to
the 80-B class needs payload 72 → 56 B (−16 B), and LuaTable is `inner:
RefCell<TableInner>`(56 = 48 + 8 B borrow flag) + `metatable: Cell<Option<GcRef>>`(8)
+ `weak_mode: Cell<u8>`(1, + 7 pad). The only 16 B available is **RefCell's
borrow flag (8) + weak_mode's slot (8)** — reclaiming both requires (a)
replacing `RefCell` with `UnsafeCell` (deleting the runtime borrow guard that
protects every table mutation — an unsafe correctness regression squarely
against omniLua's safe-Rust positioning) **and** (b) folding `weak_mode` into
`TableFlags`' spare bits. Removing only one gets to 64 B, whose GcBox (88 B)
still rounds up to the 96-B class — **partial removal buys nothing.** High risk,
all-or-nothing, and the "all" includes an unsafe change. Not worth it.

### 5. GcBox\<LuaCClosure / LuaUserData / LuaProto / LuaState / LuaThread\>  →  **NOT-WORTH-IT**

Populations of 1–19 at peak across every workload. Even a free crossing saves
< 320 B total. Below the noise floor; not rankable.

## Secondary findings (NOT GcBoxes — noted for completeness / future issues)

The tool surfaced that on the two tall poles, **non-GcBox buffer allocations
rival or exceed the GcBox bytes**. These are outside this packet's GcBox<T>
scope and each is a larger structural change, but they are where the remaining
RSS actually lives:

- **binarytrees is node-buffer-bound, not table-box-bound.** Table hash-part
  buffers total **≈ 13.6 MiB** (65,653 leaf buffers at good-size 48 B = 3.15
  MiB + 65,658 internal 4-node buffers at 160 B = 10.5 MiB) vs 12.6 MiB for the
  table boxes themselves. `TableNode` is **40 B** (full `LuaValue` key(16) +
  value(16) + `next:i32`(4) + `dead:bool`) vs C's 32 B packed
  `key_tt`/`key_val` node. Packing TableNode 40 → 32 B would cross **two** buffer
  classes: the 1-node buffer 48 → 32 (65k × 16 = 1 MiB) and the 4-node buffer
  160 → 128 (65k × 32 = 2 MiB) ≈ **3 MiB on binarytrees**. This is the known
  candidate-9 follow-on and is the single biggest remaining binarytrees lever —
  but it is a value-representation change, not a header/box shrink.
- **closure_ops pays 100k × 16 B (≈1.6 MiB) for len-1 upval-slot boxes.** Each
  `LuaLClosure` allocates a separate `Box<[Cell<GcRef<UpVal>>]>` of length 1
  (good-size 16 B). An inline-small-upvals optimization (store ≤N slots inline
  in the closure) would delete these, but it enlarges every closure box and is a
  bigger redesign; separate issue.
- **gc_pressure and fibonacci are not RSS tall poles** (peak live ≈ 0.02 MiB).
  Confirms the RSS gap lives specifically in closure_ops (closures/upvals) and
  binarytrees (tables/nodes), matching the issue.

## Verdict / recommendation to the supervisor

- **Implement candidate 1 (UpVal `open_thread_id` i64 → u32).** It is the *only*
  GcBox<T> shrink that (a) crosses a real libmalloc class boundary, (b) does so
  via a feasible, low-risk, safe-Rust field change, and (c) hits a large clean
  population (100k on a live tall pole). Projected ≈ 1.5 MiB / ~4–8 % RSS on
  closure_ops; confirm empirically per MEASUREMENT_PROTOCOL before landing.
- **Park the rest of #113's GcBox object-diet as evidenced NOT-WORTH-IT.** Every
  other high-population box (LuaLClosure, LuaString, LuaTable) fills its size
  class exactly, so nothing short of an infeasible or unsafe 16-byte removal
  changes its footprint. After candidate 1 lands, the remaining RSS gap is **not
  a GcBox-layout problem** — it is buffer representation (TableNode packing,
  inline upvals), which is a different, larger track and should be filed as
  such rather than pursued as more header/box surgery.

## Reproduce

```bash
harness/size_class/run.sh                       # default #113 workload set → harness/size_class/out/
harness/size_class/run.sh closure_ops binarytrees
cargo run -q --release -p omnilua --example size_class_histogram -- \
    harness/bench/workloads/closure_ops.lua
```
