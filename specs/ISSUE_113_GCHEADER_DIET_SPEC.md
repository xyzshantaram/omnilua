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

### Design decision

Rev-2 **adopts the R1 alternative** (owner-class vectors retaining header
age, deferred cohort maintenance) and **abandons rev-1's per-age segment
vectors**. Rev-1's design was wrong against the code in three ways R1
correctly identified:

1. Barriers never physically move objects today.
   `generational_forward_barrier` sets `age = Old0` and
   `generational_backward_barrier` sets `age = Touched1` *in place*, plus a
   `remember_minor_revisit` — the allgc chain order is pure allocation
   order. A design that moves objects between age vectors at barrier time
   invents a mutation the collector has never had, precisely in the
   hazard-heavy window (mid-sweep). Deferred cohort maintenance means: no
   physical move ever happens at barrier time; position changes only at
   sweep-completion points where the cursor state is known.
2. Three segments cannot represent seven `GcAge` states (`New`, `Survival`,
   `Old0`, `Old1`, `Old`, `Touched1`, `Touched2`). The minor marker's skip
   test is exact-`Old` (`Marker::should_trace_age`), barriers create `Old0`
   and `Touched1` at arbitrary positions, and `GcAge::next_after_minor`
   advances each state differently. **Header age stays authoritative.**
   Vectors are coarse *position cohorts* that bound which slice the young
   sweep scans — which is already how the cursor scheme behaves today
   (an `Old0`-aged object sits in the nursery region; `sweep_young_range`
   frees only `is_white() && !age.is_old()`, so age, not position, decides
   life or death).
3. "Only sweep removes during sweep" is false. `unlink_from_list` explicitly
   rewrites `sweep_prev_next` when the removed cell is the live cursor, and
   `move_allgc_to_finobj` / `move_tobefnz_to_allgc` recolor to
   `current_white()` when `GcState::is_sweep()` — cross-list moves during a
   paused incremental sweep are supported, load-bearing behavior.

The precedent that makes this design low-novelty: **`FinalizerRegistry`
already implements the exact cohort-prefix-counter scheme over a plain
`Vec`** — `pending_reallyold`/`pending_old1`/`pending_survival` prefix
counts with new entries appended at the tail, rotation in
`finish_minor_collection` (`reallyold += old1; old1 = survival; survival =
new`), stable order-preserving removal that decrements the right counter
(`retain_pending_not_in`), and whole-list promotion
(`promote_all_pending_to_old` sets `reallyold = len`). W2 applies the same
shape to the heap's owner lists.

### Data structures

```rust
struct OwnerVec {
    slots: Vec<Option<NonNull<GcBox<dyn Trace>>>>,
    tombstones: usize,
    reallyold: usize,
    old1: usize,
    survival: usize,
}

allgc:       RefCell<OwnerVec>,
finobj:      RefCell<OwnerVec>,
tobefnz:     RefCell<Vec<Option<NonNull<GcBox<dyn Trace>>>>>,
quarantined: RefCell<Vec<NonNull<GcBox<dyn Trace>>>>,
uncollected: RefCell<Vec<NonNull<GcBox<dyn Trace>>>>,
sweep_index:     Cell<usize>,
sweep_watermark: Cell<usize>,
```

Replaced outright: `head`, `finobj`, `tobefnz`, `quarantined`, `uncollected`
head cells; the seven cursor cells (`survival`, `old1`, `reallyold`,
`firstold1`, `finobjsur`, `finobjold1`, `finobjrold`); `sweep_prev_next`;
`GcHeader::next`. (`firstold1` is written and cursor-corrected today but
never read by any sweep decision — it exists for upstream parity and one
test assert. The cohort counters make it unnecessary; its deletion is part
of this wave and gets a changelog note.)

Slot orientation: **oldest at index 0, `push` appends newest at the tail.**
This maps today's prepend-at-`head` (newest first in traversal order) to
tail-append; cohort prefix ranges from index 0 are
`[0, reallyold)` = old/reallyold, `[reallyold, reallyold+old1)` = old1,
`[.., ..+survival)` = survival, remainder = nursery. `allocate` pushes
`Some(ptr)` onto `allgc` — one amortized push replacing two `Cell` writes.
`quarantined` and `uncollected` need no tombstones or cohorts: they are
append-only during life (`release_box` under quarantine;
`allocate_uncollected`) and drained only in `drop_all`. `tobefnz` needs
tombstone slots but no cohorts (its cohort mirrors live in
`FinalizerRegistry` already).

### The one mutation rule: tombstones, never shifts

Any removal from `allgc`/`finobj`/`tobefnz` outside a compaction point
writes `None` into the slot (a tombstone) and increments `tombstones`. No
slot index ever changes except at **compaction points**, which run only when
no sweep cursor is live: `finish_cycle`, the end of `sweep_young`,
`abort_cycle`, and `drop_all`. Compaction is an order-preserving
`retain`-style pass that drops `None` slots and recounts the cohort
boundaries by counting surviving slots below each old boundary index (the
same bookkeeping as `FinalizerRegistry::retain_pending_not_in`).
`start_cycle` may also compact opportunistically (no cursor exists at
`Pause → Propagate`) if `tombstones` exceeds a threshold.

Because indices are stable between compactions, the entire cursor-patch
apparatus — `unlink_from_list`'s `sweep_prev_next` rewrite and
`correct_generation_pointers`' seven cursor fixups — is deleted rather than
translated. The grayagain deletion hook (W1) survives as the only
`correct_generation_pointers` duty and is called directly by the sweep and
move paths.

This answers R1 finding 4 by choosing one explicit strategy from its menu:
stable slots + tombstones + fixed watermark + compaction only at
sweep-complete points. `Vec::retain` is used *only* at compaction points
where it is a complete (non-incremental) operation; the incremental sweep
never compacts.

### Full incremental sweep

At the `Atomic → SweepAllGc` transition (`run_atomic`), set
`sweep_index = 0` and `sweep_watermark = allgc.slots.len()`. Entering
`SweepFinObj` / `SweepToBeFnz` re-arms both for that vector. A budgeted
sweep step (`sweep_budgeted`'s replacement) is two-phase:

1. **Scan phase** (short `RefCell` borrow): examine up to `budget` slots in
   `slots[sweep_index .. sweep_watermark]`. Tombstone → skip (counts as one
   work unit, so `StepBudget::from_work(1)` still advances and terminates).
   Dead white (`color == other_white()`) → tombstone the slot, push the ptr
   onto a scratch dead-list, settle accounting (byte refund via
   `header.size()`, `allocation_tokens` removal, `objects` decrement),
   run the grayagain deletion hook if `gray_listed()`. Live → recolor
   Black/Gray to `current_white()`, exactly today's `sweep_budgeted` logic.
2. **Release phase** (borrow released): `release_box` each dead ptr.

The two-phase split answers R1 finding 5's reentrancy hazard: a payload
`Drop` running inside `release_box` can re-enter the heap (allocate, touch
registries) without hitting a live `RefCell` borrow. Invariant, stated once
and enforced everywhere: **`release_box` is never called while any owner-vec
borrow is held, and only after all bookkeeping for that step is complete.**
A `Drop`-triggered allocation during the release phase appends beyond the
watermark and survives the cycle (below).

Slots appended at or beyond `sweep_watermark` are never visited by the
in-progress sweep. This is the structural twin of today's behavior —
`allocate` prepends at `head`, behind a cursor that only moves away from
`head`, so mid-sweep allocations are never visited today either — and the
color rule (`allocate` paints `current_white()`, sweep frees only
`other_white()`) independently protects them, pinned by
`allocation_during_incremental_sweep_survives_current_cycle`. Both guards
stay.

### Every owner-list move vs. an in-progress sweep cursor

The three transitions keep their exact current semantics, including the
sweep-phase recolor that R1 finding 3/7 called load-bearing:

- `move_allgc_to_finobj` (from `luaC_checkfinalizer` path): requires
  `HDR_COLLECTED`; tombstone its `allgc` slot (linear scan, same O(n) as
  today's chain walk — cold path); **if `GcState::is_sweep()`, recolor to
  `current_white()`** (unchanged); push onto `finobj` tail. Tail = nursery
  cohort region, matching today's `link_to_head(finobj)` which also lands in
  the newest region; header age is untouched either way.
- `move_finobj_to_tobefnz`: tombstone in `finobj`, push onto `tobefnz` tail
  (today: `link_to_tail` — order-preserving append, identical). No recolor,
  matching today.
- `move_tobefnz_to_allgc`: tombstone in `tobefnz`; **if sweeping, recolor
  `current_white()`** (unchanged); push onto `allgc` tail. Today's
  `firstold1` special-case for `Old1`-aged objects is dropped with
  `firstold1` itself; the object's `Old1` header age keeps it out of the
  young sweep's free test regardless of its nursery position, and its
  grayagain entry (it holds one whenever `Old0/Old1/Touched1/Touched2`, via
  `push_next_revisit`) keeps it marked in minors.

Cursor interaction table, for a move that tombstones slot `j` in the vector
the sweep is currently walking (`i = sweep_index`, `w = sweep_watermark`):

| case | what the sweep sees | outcome |
|---|---|---|
| `j < i` (before cursor) | nothing — slot already visited | already recolored live by the scan; now leaves the list; no double visit |
| `j == i` (at cursor) | next scan reads `None`, skips | one wasted work unit; no cursor rewrite needed |
| `i < j < w` (ahead of cursor) | scan reads `None` when it arrives, skips | object is not swept from the source list; the `is_sweep()` recolor makes it `current_white()`, so whichever list it lands in treats it as this-cycle-live — same as today |
| `j >= w` (beyond watermark) | never visited | object was appended mid-sweep and moved again; both slots beyond watermark or tombstoned |

The destination push always lands at the tail. If the destination vector's
sweep phase has already completed (e.g. `allgc → finobj` while in
`SweepToBeFnz`), the object simply isn't visited there this cycle — the
recolor already made it live-white. If the destination phase hasn't started,
its watermark (taken at phase entry) includes the new slot; the recolor
means the scan sees a live object and skips it. Both cases reduce to "the
recolor rule does the semantic work; position does none," which is exactly
the C design's reasoning (`lgc.c` makes moved objects white during sweep so
they cannot be freed by a sweep that already passed them).

`barrier` / `barrier_back` / the generational barriers touch only colors,
ages, the marker's gray queue, and grayagain — no owner-vec access at all.
Barrier-during-sweep therefore cannot invalidate anything (R1 finding 5's
reframing of rev-1's wrongly-named hazard).

### Minor collection (`sweep_young`) in vector terms

`minor_collect_with_post_mark` remains stop-the-world, single-call (R1
finding 5: there is no mutator resumption inside `sweep_young` today, and
this design does not introduce one). Mapping:

- Scan range: today's `sweep_young_range(head → survival)` +
  `(survival → old1)` walks nursery + survival cohorts. Vector version scans
  `allgc.slots[reallyold + old1 ..]` — the same two cohorts. Traversal order
  flips from newest-first to oldest-first within the slice; per-element
  handling (free white non-old, `next_after_minor` aging, recolor `New` →
  `current_white` / `Touched*` → Black, `push_next_revisit`) reads no other
  element, so no order dependence exists in the logic. The battery is the
  falsifier (test matrix).
- `finobj` mirror: scan `finobj.slots[reallyold + old1 ..]`; `tobefnz`: scan
  all slots (today `sweep_young_range(tobefnz, None)`).
- Grayagain revisit pass: unchanged from today (`take_grayagain`,
  `OldRevisitTracker` positional filtering, age advance for unprocessed
  entries, `replace_grayagain(next_revisit)`) — W1 already converted the
  container.
- Frees are two-phase exactly like the incremental sweep: scan+tombstone
  under the borrow, compact + rotate, release after.
- Cohort rotation at the end (replacing today's cursor rotation in
  `sweep_young`): compact the scanned slice in place, then
  `reallyold += old1; old1 = survivors_of_survival_cohort;
  survival = survivors_of_nursery_cohort` — the `FinalizerRegistry::
  finish_minor_collection` rotation with death-adjusted counts. Today's
  `new_old1` boundary object and `survival = head.get()` cursor writes fall
  out as index arithmetic.
- `promote_all_to_old` → `reallyold = live_len, old1 = survival = 0` (the
  registry's `promote_all_pending_to_old`); `reset_all_ages` /
  `clear_generation_cursors` → all three counters to 0 + clear grayagain.

### Unique-ownership invariant (R1 finding 10)

Every heap-owned box is referenced by **exactly one** `Some` slot across:
`allgc.slots ∪ finobj.slots ∪ tobefnz ∪ quarantined ∪ uncollected`.
Structure membership is explicit — never inferred from flags. Flag semantics
are untouched and remain coarse invariant guards, not membership metadata:
`HDR_COLLECTED` = "in one of the three sweepable vectors" (checked by
`move_allgc_to_finobj`, `Gc::account_buffer`), `HDR_HEAP_OWNED` = "in any of
the five" (strict-guard checks), `HDR_FREED` = "parked in quarantined".
As today, `HDR_COLLECTED` is not cleared when a box moves to quarantined;
the invariant text for the sweepable set is therefore "`HDR_COLLECTED` set
and `HDR_FREED` clear." `grayagain` is **non-owning** and must be a subset
of the live sweepable set (W1 deletion cases keep it so). Accounting
asymmetries carry over verbatim: `allocate_uncollected` charges neither
`bytes` nor `objects`; quarantined boxes had bytes/token/object accounting
settled before `release_box` parked them.

Teardown (`drop_all`): clear grayagain (flags then entries) first, then for
each owner structure `std::mem::take` the vector *out* of its `RefCell` and
free the boxes from the local — no borrow is live while payload `Drop` runs,
and no box can be reached twice because each was in exactly one structure.
Replaces `drop_list`'s chain walk; same order (sweepable lists, then
quarantined, then uncollected).

### Phase-order table

| Phase (`GcState`) | Collector work | Mutator windows between steps | Owner-vec mutations permitted | Cursor validity |
|---|---|---|---|---|
| `Pause` | none | yes | append (`allocate`), tombstone+append (moves) | no cursor; compaction allowed |
| `Propagate` | `drain_gray_budgeted` | yes | append, moves (no recolor — not sweep) | no cursor |
| `EnterAtomic` | state hop | yes (budget can end here) | append, moves | no cursor |
| `Atomic` | `run_atomic`: final drain, post-mark hook, arm `sweep_index`/`sweep_watermark` | no (single step) | none (STW step) | cursor armed at end |
| `SweepAllGc` | budgeted two-phase scan of `allgc` | yes | append ≥ watermark; moves tombstone per the case table; sweep-phase recolor applies | `sweep_index` valid — tombstones never shift indices |
| `SweepFinObj` | same for `finobj` | yes | same | re-armed at phase entry |
| `SweepToBeFnz` | same for `tobefnz` | yes | same (incl. `move_tobefnz_to_allgc` from finalizer dispatch) | re-armed at phase entry |
| `SweepEnd` | state hop | yes | append, moves | cursor dead |
| `CallFin` | `finish_cycle`: **compaction of all three + boundary recount**, threshold calc | — | — | no cursor; compaction point |
| minor (STW inside `minor_collect_with_post_mark`) | mark revisits + roots → atomic hook → `sweep_young` scan/compact/rotate → release | none | payload-`Drop` allocations during release append at tail | no persistent cursor; compaction inside |
| `abort_cycle` | repaint colors, reset state | — | — | compaction point |

### Pacer accounting (R1 finding 9)

Definition change, stated explicitly: **pacer `bytes` counts box + charged
buffer bytes only, excluding ownership-metadata storage** (owner-vector
backing). Rationale: owner-slot bytes are proportional to *live* objects and
are reclaimed only at compaction, so charging them would feed capacity —
which does not shrink when objects die — into a threshold formula
(`finish_cycle`: `bytes * pause_multiplier / 100`) that exists to measure
collectable pressure. The mechanism is untouched: `allocate` charges
`size_of::<GcBox<T>>()`, `Gc::account_buffer` adjusts, sweep refunds
`header.size()`.

Consequences to measure, not hand-wave: after the diet every header charge
shrinks (−32 B/object 64-bit, −16 B wasm32), so thresholds derived from
`bytes` drop and collection cadence shifts even with zero behavior change —
R1's point that the representation change alters collection thresholds is
correct and applies to W1 too (−16/−8 B per object). The measurement plan
therefore records `collections()` / `minor_collections()` per workload
before/after, and a new diagnostic `Heap::owner_capacity_bytes()` (sum of
`capacity × slot size` across owner vectors) feeds heap-diff so total memory
claims include the relocated pointer and its slack. If cadence shift alone
regresses a GC-heavy row, the fallback is an explicit slack charge into
`bytes` at compaction points — a measured decision, not a default.

### Diagnostics and remaining surfaces

`for_each_header` / `start_cycle`'s repaint / `promote_all_to_old` /
`reset_all_ages` / `abort_cycle`: iterate vectors, skip `None`.
`type_name_count` and `allgc_cohort_stats` (lua-cli telemetry): trivial slot
walks; cohort stats become index arithmetic instead of a cursor-compare
walk. `Marker` internals: untouched (its `gray_queue` is already a
`Vec<NonNull<GcBox<dyn Trace>>>`). No code outside `heap.rs` touches
`header.next`/`gray_next` (verified by grep across `lua-gc`, `lua-types`,
`lua-vm`); `lua-vm` consumes only the three `move_*` functions, whose
signatures are unchanged.

### tobefnz FIFO (R1 finding 7)

Stated explicitly: **`FinalizerRegistry` is the semantic FIFO authority**
for `__gc` order — `push_to_be_finalized` appends, `pop_to_be_finalized`
consumes via `remove(0)`. The heap-side `tobefnz` vector is a *membership*
structure only: entries enter both structures in the same act (the runtime
registers the entry and calls `move_finobj_to_tobefnz`) and leave in the
same act (`pop_to_be_finalized` then `move_tobefnz_to_allgc`), so the sets
stay equal; the heap-side *order* is read by nothing except sweep visitation
(order-insensitive) and `drop_all`. Tail-append (today's `link_to_tail`)
preserves front-to-back FIFO order in the heap vector anyway, but
correctness does not lean on it. `swap_remove` appears nowhere in this
design.

Risk: MEDIUM-HIGH stands — this rewires the collector's spine even though
each piece now has a named precedent. Mitigations: the tombstone rule is a
single mechanism replacing seven cursor patches; the FinalizerRegistry
pattern is already battle-tested in-tree; W2 starts only after W1's measured
landing; supervised branch, full battery per commit, no mixing with other GC
work.

## Wave 3 — deleted (R1 finding 2 accepted)

After both links are removed the header is already 8 B on both widths —
`color(1) + age(1) + flags(1) + pad(1) + size(4)` — so `Cell<u16>` packing
saves nothing. Worse, it is a measured anti-pattern in this exact spot: the
`GcHeader` doc-comment records that packing the hot fields cost ~+4% Ir on
gc_pressure (recount 2026-06-10), which is why color/age/flags each own a
byte today. `Cell`-based bit-packing turns every hot color/age read-write in
`mark_box`, `drain_gray_queue`, and the sweep loops into read-modify-write.
Wave 3 is deleted, not deferred.

What replaces it (mechanical, rides with W2's PR): `#[repr(u8)]` on `Color`
and `GcAge` to lock the one-byte layout, and a compile-time size assertion
on `GcHeader` — valid ungated on both widths since the post-diet header
contains no pointers (`const _: () = assert!(size_of::<GcHeader>() == 8)`),
plus updated `value_layout` example output.

## Measurement plan (replaces rev-1's RSS projections)

Rev-1's projected ratios (closure_ops 2.96× → ~2.2×, binarytrees 2.22× →
~1.9×) are withdrawn — R1's arithmetic critique is accepted in full: the W2
pointer is relocated not removed, vector capacity was omitted, allocator
buckets are unknown, no absolute C/Rust peak-RSS pairs were shown, and prior
evidence (the R2-diet packet) showed payload-byte savings translating
roughly 1:1 into process RSS, not into outsized ratio movement. No number
appears below as a target; the done-condition is that the following
measurements exist and the drop-if-neutral rule is applied to them.

All measurements follow `docs/MEASUREMENT_PROTOCOL.md`: frozen baseline
binary built from origin/main before edits, interleaved A/B within rounds,
≥4 rounds judged on min-ratio, quiet machine for any number that enters the
PR body, revert-validation for any surprise, and the bench host used
exclusively (coordination board).

Per wave (W1 first, W2 separately):

1. **Compute-neutrality (Ir arbiter)** — `harness/bench/instr-count.sh` on
   fibonacci and mandelbrot. These rows allocate little and must be flat;
   the abort criterion is Ir regression > 2% on either. This is the
   instruction-removal class check: the diet must be free for non-GC paths.
2. **GC-path Ir** — instr-count on binarytrees, table_ops, closure_ops.
   W1 expectation: flat to slightly down (barrier fast path loses a Cell
   write). W2 expectation: sweep-loop instruction mix changes shape
   (pointer-chase → linear scan + tombstone skips); an Ir increase here with
   a wall/Bcm improvement is a legitimate CPI-class outcome — classify
   before judging, per the protocol's win-class table. Any change touching
   slot layout is codegen-layout-adjacent: pair Ir with a cold-machine wall
   A/B (the T5a lesson).
3. **Heap-diff including ownership storage** — `harness/bench/heap-diff.sh`
   on closure_ops and binarytrees for alloc-count / bytes-per-block deltas,
   which surfaces both the shrunken `GcBox` allocations and the owner-vector
   backing blocks (few, large). Supplement with the new
   `Heap::owner_capacity_bytes()` diagnostic sampled at workload peak so
   excess capacity and tombstone slack are numbers, not assumptions.
4. **Peak RSS with absolutes** — `harness/bench/compare.sh` on the `_long`
   GC-heavy rows, reporting C peak, Rust-before peak, Rust-after peak in
   bytes, and only then the ratios. Sub-100ms rows are excluded
   (startup-dominated, per the protocol).
5. **Cadence** — `collections()` / `minor_collections()` totals per workload
   before/after. A shift is *expected* (header bytes shrink, so
   pacer-derived thresholds shift; `GC_MIN_THRESHOLD` floors small heaps).
   Record it; if a GC-heavy row's wall regression tracks the cadence change,
   test the fallback (slack charge at compaction) before concluding.
6. **wasm32** — `cargo check -p lua-vm --target wasm32-unknown-unknown` is
   the compile gate (layout asserts must not be 64-bit-only), but per R1
   finding 11 that is not a memory measurement: additionally run a fixed
   GC-heavy workload under a wasm runtime and record linear-memory
   high-water (`memory.size` after run, before/after the change — the
   playground harness or wasmtime both expose it). Wasm linear memory
   retains grown pages, so owner-vector growth spikes cost real,
   unreturnable pages there; this is the environment where capacity slack
   matters most.

Decision rule: W1 keeps if Ir is flat on compute rows and heap-diff/RSS do
not regress. W2 keeps only if the measurements show a net win at peak
(heap-diff + owner_capacity_bytes accounting) or a wall/Ir win from sweep
locality — otherwise it is reverted and the honest negative recorded, per
the drop-if-neutral rule. "The 8-byte header is aesthetically right" is not
a keep reason.

## Test matrix

R1's enumerated list, mapped to existing gates or new kit tests. "Kit"
means deterministic in-memory tests in `heap.rs`'s test module — the
fast-inner-loop tier, milliseconds per run.

| # | Scenario | Existing coverage | New work |
|---|---|---|---|
| M1 | Each of the 3 owner moves with target **before** the active sweep cursor | none mid-sweep (`finalizer_intrusive_lists_sweep_and_drop` is at-Pause only) | NEW kit: drive `incremental_run_until_state_with_post_mark` to `SweepAllGc`, advance partially with `StepBudget::from_work(n)`, move an already-visited object, finish, assert membership/liveness/`allgc_count` |
| M2 | Moves with target **at** the cursor slot | none | NEW kit (same harness, position at `sweep_index`) |
| M3 | Moves with target **ahead of** the cursor (and beyond watermark) | none | NEW kit; assert the recolor rule preserved the object through both lists' sweeps |
| A1 | Allocation during `SweepAllGc` | `allocation_during_incremental_sweep_survives_current_cycle` | extend to `SweepFinObj`, `SweepToBeFnz`, and the `EnterAtomic`→`Atomic` gap |
| B1 | `StepBudget::from_work(1)` resumption to completion | `full_collect_equivalent_to_incremental_to_pause`, `budget_zero_does_some_work`, `sweep_can_pause_and_resume` | NEW kit variant: interleave a move + an allocation at every `InProgress` step under budget 1 (churn test; also proves tombstone-skip termination) |
| F1 | FIFO finalization order | `finalizer_registry_minor_snapshot_uses_cohort_boundaries`, `finalizer_registry_marks_and_clears_finalized_bit`; gc.lua/gengc.lua `__gc`-order asserts | NEW kit: registry↔heap sync — register N finalizable, kill all, pop FIFO, assert each `move_tobefnz_to_allgc` succeeds and set-equality holds throughout |
| G1 | grayagain deletion via full-sweep free | `full_sweep_unlinks_freed_grayagain_entries` | ports as-is |
| G2 | grayagain dedup + persistence | `grayagain_links_object_once`, `grayagain_list_carries_old1_until_old`, `grayagain_list_carries_touched2_until_old` | port as-is (these pin the persistence facts rev-1 got wrong) |
| G3 | Cross-list move of a gray-listed object deletes its entry | none (behavior exists via `unlink_from_list` → `correct_generation_pointers` but untested) | NEW kit: `Touched1` object → `move_allgc_to_finobj`, assert `grayagain_count` drops and next minor is sane |
| Q1 | Quarantine parking + single free at teardown | canary battery under `LUA_RS_GC_QUARANTINE=1`, `harness/asan-stress.sh` | NEW kit: quarantined box's slot is tombstoned, box appears once in `quarantined`, `drop_all` frees exactly once |
| U1 | Uncollected teardown | `allocate_uncollected_survives_collection_but_is_freed_on_heap_drop`, `bootstrapping_routes_allocate_to_the_uncollected_list`, #249 leak canaries (with the git-stash-revert verification caveat) | port as-is |
| O1 | Newest-first order dependence (absence thereof) | not directly unit-testable — it is a claim of *no* dependence | falsifiers: 36 GC canaries × incremental+generational (`harness/canaries/gc/run_canaries.sh`), gc.lua + gengc.lua officials ×5 versions, the quarantine canary run |
| C1 | Cohort rotation / scan bounding | `minor_collect_frees_young_and_keeps_old`, `minor_sweep_uses_generation_cursors_to_skip_old_tail` (sweep-visited counts pin the scan range), `minor_collect_skips_untouched_old_root_scan_work`, `promote_and_reset_all_ages`, `full_sweep_corrects_generation_cursors_when_cursor_object_is_freed` (assertion rewrites to counter state) | port with cursor asserts translated to counter/index asserts |

PR gate on top of the kit tier: full battery — canaries ×2 modes,
quarantine run, strict-guard run, `harness/run_official_all.sh`,
`specs/oracle/check.sh` ×5, workspace tests, wasm check — per the repo's
rung-6 definition.

## Sequencing & gates

1. **W1 lands alone**: own PR, kit rows G1–G3 + A1 + battery + measurement
   plan items 1/3/5. Small, reversible, and it produces the first real
   number for "what does removing one link actually buy" — which is the
   evidence W2's approval depends on.
2. **W2 only after** W1's measured result and this spec's round-2 review
   verdict. Supervised branch (per the deep-spec → codex-review → execute
   workflow), full battery per commit, no mixing with other GC work. The
   `repr(u8)`/size-assert cleanup (ex-W3) rides with W2.
3. Abort criteria (unchanged from rev-1, plus one): any canary/quarantine
   flip that needs more than a localized fix; Ir regression > 2% on
   fibonacci/mandelbrot; new: a GC-heavy-row regression attributable to
   cadence or tombstone-scan overhead that survives the slack-charge
   fallback test — then W2 reverts and the negative is recorded.

## Relation to other open work

Independent at the code level of #252 (Rc<Heap> ownership — already landed;
`Heap::new` returns `Rc<Self>` in current main, so rev-1's "sequence after"
note is satisfied) and #253 (LuaError bytes). If #253 is still open when W2
starts, land it first for the same diff-purity reason as before. The
`grayagain`/owner-vector work overlaps textually with any generational-GC
follow-up in `specs/followup/issue-93-generational-gc-plan.md`; coordinate
on the board before parallel GC branches exist.
