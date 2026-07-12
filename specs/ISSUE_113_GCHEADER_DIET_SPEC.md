# Issue #113 — GcHeader diet without unsafe: kill both intrusive fat links (rev 2)

Status: SPEC rev 2 (not started). Answers every finding of the round-1
adversarial review (`ISSUE_113_GCHEADER_DIET_SPEC_REVIEW_R1.md`, VERDICT:
REVISE). Rev-1 claimed both fat links can be removed in safe Rust; that
claim survives review, but rev-1's W2 design (per-age segment vectors) did
not — this revision replaces it with owner-class vectors plus deferred
cohort maintenance, drops Wave 3, and replaces the RSS projections with a
measurement plan.

## Review R1 disposition table

| # | Finding (one line) | Disposition |
|---|---|---|
| 1 | W2 relocates the fat pointer (owner-vector slot), does not eliminate it | TODO |
| 2 | W3 saves zero bytes and repeats a measured +4% Ir packing regression | TODO |
| 3 | "Only sweep removes during sweep" is false; `unlink_from_list` rewrites the live cursor | TODO |
| 4 | `retain` + incremental cursor is not a complete algorithm | TODO |
| 5 | Wrong mid-minor hazard named; young sweep is STW, real hazards are full-sweep pauses + `release_box` reentrancy under RefCell | TODO |
| 6 | Three segments do not replace seven `GcAge` states | TODO |
| 7 | Order is load-bearing (newest-first sweep, tobefnz FIFO, head-vs-tail inserts); `swap_remove` unsuitable | TODO |
| 8 | W1 is bigger than "4 functions"; grayagain persists across minors and has deletion paths | TODO |
| 9 | Pacer accounting must address owner-vector capacity | TODO |
| 10 | Quarantine/uncollected need explicit unique-ownership invariants; flags can't identify a vector | TODO |
| 11 | 64-bit layout claims do not generalize to wasm32 | TODO |
| R | RSS projection (2.96→2.2 etc.) unsupported by the stated arithmetic | TODO |

## Where the bytes go today (pointer-width qualified)

`crates/lua-gc/src/heap.rs`, `GcHeader` (`#[repr(C)]`). A
`NonNull<GcBox<dyn Trace>>` is a fat pointer: 16 B on 64-bit targets, 8 B on
wasm32 (4 B data + 4 B vtable). `Option<NonNull<...>>` is the same size via
the null-pointer niche. Every layout figure in this spec is therefore stated
for both widths.

| field | 64-bit | wasm32 | role |
|---|---|---|---|
| `color: Cell<Color>` | 1 | 1 | tri-color mark state |
| `age: Cell<GcAge>` | 1 | 1 | generational age (seven states, authoritative — see W2) |
| `flags: Cell<u8>` | 1 | 1 | finalized / collected / gray_listed / freed / heap_owned |
| padding | 1 | 1 | |
| `size: Cell<u32>` | 4 | 4 | pacer bytes sweep will refund |
| `next: Cell<Option<NonNull<GcBox<dyn Trace>>>>` | 16 | 8 | intrusive owner-list link (head/finobj/tobefnz/quarantined/uncollected) |
| `gray_next: Cell<Option<NonNull<GcBox<dyn Trace>>>>` | 16 | 8 | grayagain revisit link |
| **total** | **40** | **24** | |

Removing both links leaves `color + age + flags + pad + size` = **8 B on both
widths** (alignment 4 from the `u32`). Rev-1's "40 → 8" and "`GcBox<UpVal>`
72 → 40" were 64-bit-only claims; on wasm32 the header goes 24 → 8 and each
removed link saves 8 B, not 16.

The honest accounting (R1 finding 1): removing a link from the header does
not always remove its bytes from the process. The owner-list link is
*relocated* — every live heap-owned object still needs exactly one
`NonNull<GcBox<dyn Trace>>` somewhere so the heap can trace, sweep, and
destroy it (safety-model invariant 2 in the heap.rs module docs). A W2 owner
vector holds that pointer at 16 B/slot (64-bit) or 8 B/slot (wasm32), plus
`Vec` excess capacity and tombstone slack. So:

- **W1 removes bytes outright**: `gray_next` dies in every object; the
  replacement Vec slot exists only while an object is on the revisit list
  (transient, bounded by the barrier-touched set).
- **W2 relocates bytes**: box shrinks by one link, owner vector gains one
  slot. Direct saving ≈ 0 ± capacity slack *before* allocator size-class
  effects. What W2 actually buys: (a) size-class crossings — e.g. on 64-bit
  `GcBox<UpVal>` 72 → 40 B moves macOS malloc buckets 80 → 48; (b) linear
  sweep over a dense pointer array instead of a pointer chase; (c) deletion
  of the seven generational cursor cells and the `unlink_from_list` /
  `correct_generation_pointers` cursor-patch machinery (the code region where
  #113's original pacing bug lived). All three are hypotheses to measure, not
  savings to assert — see the measurement plan.
- Total type-erased ownership metadata per live object after W1+W2 is 8 B
  header + one external fat slot: ≥ 24 B on 64-bit, ≥ 16 B on wasm32.
  "`GcBox<UpVal>` = 40 B" describes the box allocation, not the ownership
  footprint.

Vector slots hold pointers to `Box` allocations; a `Vec` reallocation moves
slots, never boxes, so no `Gc<T>` or `Marker::gray_queue` entry is ever
invalidated by owner-vector growth.

## Wave 1 — grayagain becomes a heap-owned Vec (lands alone, measured)

### What exists today (ground truth)

`gray_next` + `HDR_GRAY_LISTED` implement one list, `Heap::grayagain`, with
these operations:

- `remember_minor_revisit` — prepend with flag-based dedup. Callers:
  `generational_forward_barrier` (child aged `Old0`),
  `generational_backward_barrier` (parent aged `Touched1`), and
  `replace_grayagain`.
- `mark_minor_revisit_objects` — walked at the start of every minor mark
  (`minor_collect_with_post_mark`, before roots are traced).
- `take_grayagain` — drains to a `Vec`, clearing links and flags; used by
  `sweep_young` (the drained set feeds `OldRevisitTracker`) and by
  `unlink_grayagain`.
- `replace_grayagain` — `clear_grayagain` then re-prepend a new set in
  reverse, preserving the vec's order; called at the end of `sweep_young`
  with the `next_revisit` set.
- `clear_grayagain` — clears links + flags; called from
  `clear_generation_cursors` (so from `reset_all_ages` and `drop_all`) and
  `set_all_cursors_to_head` (from `promote_all_to_old`).
- `unlink_grayagain` — **the deletion path R1 flagged**: called from
  `correct_generation_pointers` whenever an object with `HDR_GRAY_LISTED`
  set is unlinked from an owner list for *any* reason — a full sweep freeing
  it (`sweep_budgeted` → `correct_generation_pointers`, pinned by the
  `full_sweep_unlinks_freed_grayagain_entries` test), a young sweep freeing
  it, or a cross-list move (`move_allgc_to_finobj` etc. via
  `unlink_from_list`). Implemented as take-filter-replace.
- `grayagain_count` — diagnostic (read by lua-cli telemetry).

Two rev-1 claims were wrong against this code (R1 finding 8, confirmed):
the list is **not** cleared each minor cycle — `sweep_young` *replaces* it
with the next cycle's revisit set via `push_next_revisit`/`replace_grayagain`,
and entries persist across minors until their object reaches `Old`
(`grayagain_list_carries_old1_until_old` and
`grayagain_list_carries_touched2_until_old` pin `Old1` and `Touched2`
persistence). And the surface is larger than "~4 functions": header
construction (`GcHeader::new_white`), `Heap::new`, all seven operations
above, `correct_generation_pointers`, `sweep_young`, `drop_all` ordering,
and the tests. Risk is re-rated **MEDIUM-LOW**: still a self-contained
packet, but it touches every sweep-free path via the deletion hook.

### The replacement

```rust
grayagain: RefCell<Vec<NonNull<GcBox<dyn Trace>>>>,
```

`gray_next` is deleted from `GcHeader`. `HDR_GRAY_LISTED` stays and keeps
exactly its current meaning: "this object has an entry in grayagain" — it is
the dedup check in `remember_minor_revisit` and the cheap guard that lets
`correct_generation_pointers` skip the deletion scan for the common
(non-listed) case. The Vec is **non-owning**: membership invariant is
`grayagain ⊆ live sweepable boxes` (every entry's object is currently in
allgc/finobj/tobefnz and not `HDR_FREED`), maintained by the deletion hook
below.

Operation mapping:

- `remember_minor_revisit`: flag check, then `borrow_mut().push(ptr)` + set
  flag. Push order replaces prepend order — see ordering note below.
- `mark_minor_revisit_objects`: iterate a short borrow, calling
  `Marker::mark_box` per entry. Marking is idempotent and deduped by
  `Marker::visited`, so iteration order affects only the order gray-queue
  entries are pushed, not the reachability fixed point.
- `take_grayagain`: `std::mem::take(&mut *borrow_mut())`, then clear each
  entry's flag *after* the borrow is released. Returns the Vec;
  `sweep_young`'s `OldRevisitTracker` consumes it unchanged.
- `replace_grayagain(objects)`: clear flags of any current entries, then
  install `objects` as the new buffer, setting flags. To reuse capacity
  across cycles, `sweep_young` builds `next_revisit` in the buffer returned
  by `take_grayagain` after draining it — one persistent buffer pair, the
  same recycling pattern as `marker_pool`.
- `clear_grayagain`: clear flags, `vec.clear()` (capacity retained).
- `unlink_grayagain(removed)`: `retain(|p| !addr_eq(p, removed))` + clear
  the removed object's flag. Same take-filter semantics as today, no longer
  needs the full relink.
- `grayagain_count`: `borrow().len()` — flag-dedup guarantees no duplicates,
  so len equals today's walk count (`grayagain_links_object_once` pins it).

**Ordering.** Today's intrusive list iterates newest-remembered-first
(prepend order); the Vec iterates oldest-first. The two consumers are
`mark_minor_revisit_objects` (order-insensitive per above) and
`sweep_young`'s revisit loop over the drained set, which processes each
entry independently (`was_processed` filtering by identity, age advance,
`push_next_revisit`) — no entry's handling reads another's. No behavior
depends on intra-list order; the canary battery ×2 modes plus
gc.lua/gengc.lua are the falsifiers, per the test matrix.

**Deletion cases** (each must clear the flag and remove the entry before the
box can be freed):

1. Full sweep frees a gray-listed object: `sweep_budgeted` →
   `correct_generation_pointers` → `unlink_grayagain`, *before*
   `release_box`. Unchanged call order; the Vec op must not hold its borrow
   into `release_box` (it doesn't — the retain completes first).
2. Young sweep frees a gray-listed object: entries were already drained by
   `take_grayagain` at `sweep_young` entry, flags cleared, so
   `correct_generation_pointers` sees `gray_listed() == false`. Unchanged.
3. Cross-list move of a gray-listed object (e.g. `move_allgc_to_finobj` on a
   `Touched1` table given a `__gc` metatable): under the vector design the
   move no longer unlinks `next`, but it must still *keep* the grayagain
   entry — the object remains live and still needs its revisit. Today's code
   **deletes** the entry on move (via `unlink_from_list` →
   `correct_generation_pointers`) and the object re-enters grayagain only if
   a barrier fires again. W1 must preserve today's observable behavior
   exactly: the move deletes the grayagain entry. A new kit test pins this
   (test matrix, row G3).
4. Teardown: `drop_all` → `clear_generation_cursors` → `clear_grayagain`
   runs before any `drop_list` — flags and entries are gone before boxes are
   freed. The Vec version keeps this order (R1 finding 10's last point).

**Quarantine mode**: `release_box` under `LUA_RS_GC_QUARANTINE=1` parks the
box on the quarantined list with `HDR_FREED`; case 1/2 above ran first, so no
grayagain entry can point at a parked box. The quarantine debug asserts in
`Gc::as_box`/`Marker::mark_box` would catch a violation deterministically —
run the canary battery under quarantine as part of the gate.

Gates: full battery (36 GC canaries × incremental+generational, quarantine
run, strict-guard run, officials ×5, workspace tests, wasm `cargo check`) +
Ir arbiter on binarytrees/table_ops + heap-diff, per the measurement plan.
W1 ships as its own PR and its measured result gates W2.

## Wave 2 — owner-class vectors with deferred cohort maintenance

TODO — chosen design, data structures, phase-order table, move-vs-cursor cases.

## Wave 3 — deleted

TODO — rationale.

## Measurement plan (replaces rev-1 RSS projections)

TODO

## Test matrix

TODO

## Sequencing & gates

TODO

## Relation to other open work

TODO
