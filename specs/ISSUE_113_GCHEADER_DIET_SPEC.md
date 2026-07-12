# Issue #113 — GcHeader diet without unsafe: kill both intrusive fat links

Status: SPEC (not started). Feeds the deep-spec → codex-review → execute
pipeline. Companion to the parked closing comment on #113, which stopped at
"the remaining overhead is the GcHeader — shrinking it needs thin-pointer +
vtable-recovery unsafe work." This spec's claim: **it does not.** Both
16-byte links can be removed with safe Rust, taking the header from 40 B to
8 B — parity with C's ~10 B CommonHeader — and taking `GcBox<UpVal>` from
72 B to 40 B (C: ~40 B).

## Where the 40 bytes go today

`crates/lua-gc/src/heap.rs`, `GcHeader`:

| field | size | role |
|---|---|---|
| `color: Cell<Color>` | 1 | tri-color mark state |
| `age: Cell<GcAge>` | 1 | generational age |
| `flags: Cell<u8>` | 1 | finalized / collected / gray_listed / freed / heap_owned |
| `size: Cell<u32>` | 4 | pacer bytes sweep will refund |
| `next: Cell<Option<NonNull<GcBox<dyn Trace>>>>` | 16 | intrusive owner-list link (allgc/finobj/tobefnz/quarantined/uncollected) |
| `gray_next: Cell<Option<NonNull<GcBox<dyn Trace>>>>` | 16 | grayagain revisit link |
| padding | ~1 | |

The two links are **fat pointers**: every object carries two vtable copies
purely to participate in linked lists. The vtable belongs with the object
(it is already reachable through any fat pointer *to* the object); it never
needed to be duplicated per-link. The unsafe route recovers thinness by
splitting fat pointers manually. The safe route below removes the links
instead.

## Wave 1 — grayagain becomes a heap-owned Vec (−16 B, small, landable alone)

`gray_next` + the `HDR_GRAY_LISTED` dedup flag implement one list:
`grayagain` (`remember_minor_revisit`, `clear_grayagain`, drain during
atomic). Replace the intrusive link with:

```rust
grayagain: RefCell<Vec<NonNull<GcBox<dyn Trace>>>>,
```

`HDR_GRAY_LISTED` stays (dedup check), `gray_next` is deleted. The Vec is
bounded by the number of barrier-touched old objects per minor cycle and is
cleared each cycle — amortized allocation, no per-object cost. The
`Marker::gray_queue` is already exactly this shape (a scratch
`Vec<NonNull<...>>`), so this makes the revisit list consistent with the
mark queue rather than novel.

Risk: LOW. Touches ~4 functions. Gates: full battery (canaries ×2 modes,
quarantine, strict, official ×5, workspace) + Ir arbiter on the GC-heavy
rows (binarytrees, table_ops) with heap-diff before/after.

## Wave 2 — owner lists become per-segment Vecs (−16 B, the real redesign)

The single intrusive `next` chain serves two masters today:

1. **List membership** — which owner list a box is on
   (head/finobj/tobefnz/quarantined/uncollected).
2. **Generational segment order** — the allgc chain's *relative order* IS
   the age structure: `survival`/`old1`/`reallyold`/`firstold1` (and the
   finobj mirrors) are cursors into it, and minor sweep walks
   head→survival-boundary promoting ages by position.

The C design needs cursors because C has one intrusive list. A Vec-based
design does not need to imitate that: give each generation segment its own
vector —

```rust
nursery:   RefCell<Vec<NonNull<GcBox<dyn Trace>>>>,  // allocate() pushes here
survival:  RefCell<Vec<NonNull<GcBox<dyn Trace>>>>,
old:       RefCell<Vec<NonNull<GcBox<dyn Trace>>>>,
finobj_{nursery,survival,old}, tobefnz, quarantined, uncollected: RefCell<Vec<...>>,
```

- **Allocation**: push to `nursery` — replaces two Cell writes with one
  amortized push; no per-object regression expected (verify by Ir).
- **Minor sweep**: drain `nursery`/`survival` linearly (cache-friendly
  compared to pointer chasing), retaining survivors into the next segment
  up. Age promotion becomes *segment membership* — the `survival`/`old1`/
  `reallyold` cursor arithmetic and `firstold1` special case are deleted
  outright, which is a correctness simplification (that cursor code is
  where #113's original pacing bug lived).
- **Full sweep**: walk each segment Vec with `retain`-style compaction.
  Incremental sweep holds a per-segment index; indices stay valid because
  only the sweep itself removes during a sweep phase (moves triggered by
  barriers/finalizers during sweep append, never remove — matching today's
  "sweep owns unlinking" invariant).
- **Cross-list moves** (`move_allgc_to_finobj`, `move_finobj_to_tobefnz`,
  `move_tobefnz_to_allgc`): O(n) search today (chain walk) → O(n) search
  (position scan) + `swap_remove` + push. Same complexity, and these are
  cold paths (setmetatable-with-__gc, finalizer scheduling).
- **`for_each_header` / `type_name_count` / drop_all**: trivial Vec walks.

`HDR_COLLECTED`/`HDR_HEAP_OWNED` flag semantics are unchanged (membership
flags, not links). Quarantine parking becomes a push onto the quarantined
Vec — identical semantics, and `release_box` keeps being the single free
point.

Risk: MEDIUM-HIGH — this rewires the collector's spine. The two invariants
that need adversarial attention (codex-review focus):

- **Barrier-driven age transitions during an in-progress minor sweep**: an
  object appended to `old` by a back-barrier while the sweep iterates
  `nursery` must not be visited twice or skipped. Today's chain splices
  have the same hazard class; enumerate each barrier → segment-move pair
  against the sweep cursor.
- **Sweep-order dependence**: upstream Lua sweeps newest-first (prepend
  order). Per-segment Vecs sweep in push order (oldest-first within a
  segment). Nothing in the collector *should* depend on intra-segment
  order beyond age boundaries — but "should" is exactly what the canary
  suite (36 GC canaries × incremental+generational), quarantine mode, and
  gengc.lua/gc.lua officials exist to falsify.

## Wave 3 — pack the scalars (40→8 total)

After the links go: `color` (2 bits) + `age` (3 bits) + `flags` (5 bits →
now 5 used) fit one `Cell<u16>` with room to spare; `size: Cell<u32>`
stays. Header: 8 B. `GcBox<T>` layout: 8 B header + payload.

## Expected effect (to be measured, not asserted)

Per-object savings: 32 B header + allocator bucket effects. Object counts
at peak (heap-diff, closure_ops): ~100k upvalues + closures → ~3.2 MB
direct. Projection: closure_ops RSS 2.96× → ~2.2×, binarytrees 2.22× →
~1.9×; both would still miss the ≤2.0×/meet-it boundary respectively, so
re-measure against #113's done-condition per `docs/MEASUREMENT_PROTOCOL.md`
(frozen-baseline interleaved A/B, Ir + heap-diff arbiters, wall distrusted).

## Sequencing & gates

1. Wave 1 PR alone (small, reversible, its own battery + Ir/heap-diff).
2. Wave 2 spec section gets codex adversarial review FIRST (this document),
   then a supervised branch with the full battery per commit; no mixing
   with other GC work. Wave 3 rides with Wave 2's PR (it is mechanical once
   links are gone).
3. Abort criteria: any canary/quarantine flip that needs more than a
   localized fix, or Ir regression >2% on non-GC rows (fibonacci,
   mandelbrot) — the diet must be free for compute paths.

## Relation to other open work

Independent of #252 (Rc<Heap> ownership) and #253 (LuaError bytes) at the
code level, but sequence AFTER them: both touch `heap.rs`/`gc.rs` surfaces
and are small; landing them first keeps this branch's diff pure.
