//! stale_handle_kit — deterministic in-memory reproductions of the issue #267
//! stale-`GcRef` unsoundness cases.
//!
//! This is the "custom subsystem tester" for GC stale-handle safety: it
//! constructs a `Heap` + `Gc`/`GcRef` directly and scripts the exact failure
//! shapes from `specs/ISSUE_267_OWNER_IDENTITY_SPEC.md` §2, so each reproduces
//! in milliseconds, 100% of the time, with no VM/oracle in the loop. The
//! full-VM oracle *cannot* easily produce a use-after-teardown — that
//! invisibility is the whole reason the bug is subtle — so this kit is what
//! makes the fix (and the residuals) legible.
//!
//! # What #267 actually closes (the sound part — release, every build)
//!
//! The **no-guard** `downgrade`/`account_buffer` paths are made **deref-free**:
//! with no active `HeapGuard` they panic while reading *nothing*. Previously
//! they called `is_heap_owned()`, which dereferences the box to decide — a
//! use-after-free *inside the safety check* once the owning heap is dropped.
//! This is real release-mode UAF removal.
//!
//! | Test | Case | Status |
//! |---|---|---|
//! | `f1_no_guard_downgrade_of_detached_box_panics` | F1 no-guard | **green** — panics deref-free (was: returned an always-upgrades weak) |
//! | `f1_no_guard_downgrade_after_heap_drop_is_deref_free` | F1 no-guard, box actually freed | **green w/ fix** — panics without reading the freed box (pre-fix: a UAF, catchable under Miri/ASan) |
//! | `f3_no_guard_account_buffer_panics` | F3 no-guard | **green** — panics deref-free |
//! | `f4_stale_deref_trips_quarantine` | F4 baseline | **green** — the `as_box` `HDR_FREED` assert |
//! | `f4_account_buffer_on_swept_box_trips_quarantine` | F3c/F4 same-heap swept | **green** — the intrinsic box read hits the `as_box` assert |
//!
//! # What #267 does NOT close (documented residuals — require spec option B)
//!
//! An early revision tried to close these cheaply with a quarantine "tripwire"
//! that read the target box header at `downgrade`/`account_buffer` time. The
//! codex review found that unsound: the *active* heap's quarantine flag says
//! nothing about whether the *target's owner* freed the box, so a foreign-heap
//! op would dereference freed memory *inside* the check — strictly worse than
//! doing nothing. It was reverted. For these cases, **validating the handle at
//! all means reading a possibly-freed box, which is itself the use-after-free**;
//! there is no cheap fix. They are kept below as `#[ignore]`d tests that assert
//! the *desired* option-B end state (a stale handle must not upgrade), so they
//! currently fail on purpose and become the acceptance test when option B (a
//! slot-indexed handle carrying its own owner identity, or an API-shape change
//! that ties teardown to an exclusive borrow) lands.
//!
//! - `f2a_foreign_live_downgrade_resurrects_residual` — foreign heap, box live.
//! - `f2b_foreign_swept_downgrade_resurrects_residual` — foreign heap, box swept.
//! - `f2c_same_heap_swept_redowngrade_resurrects_residual` — same open heap, box
//!   swept while unrooted, re-downgraded (the token is re-minted for a freed
//!   address — the repo's own `registering_after_sweep_yields_a_distinct_token`
//!   test is the mechanism).
//! - **Not stageable in-memory at all:** foreign/stale use of a box whose owner
//!   already ran `drop_all` (the memory is truly freed — a later deref is a hard
//!   UAF), and an already-issued `&T` outliving a later `drop_all(&self)`. Both
//!   need option B / an API-shape change; a `trybuild` compile-fail test is the
//!   real guard for the latter once teardown takes `&mut`.
//!
//! Note `upgrade()` itself never dereferences the box (it only consults the
//! heap's allocation-token map), so the residual tests below are memory-safe to
//! run — the unsoundness is that they return `Some`, and the UAF would land on a
//! later deref of the resurrected handle.

use lua_gc::{Gc, Heap, HeapGuard, Marker, Trace};
use lua_types::GcRef;

/// A payload with no outgoing GC edges.
#[derive(Debug)]
struct Probe;

impl Trace for Probe {
    fn trace(&self, _m: &mut Marker) {}
}

/// A root set that traces nothing, so `full_collect` sweeps every unrooted box.
struct NoRoots;

impl Trace for NoRoots {
    fn trace(&self, _m: &mut Marker) {}
}

// ---------------------------------------------------------------------------
// Closed (green): the deref-free no-guard guards — issue #267 F1/F3/F4
// ---------------------------------------------------------------------------

/// F1 (no-guard, detached target). With no active `HeapGuard`, `downgrade`
/// cannot validate against any heap and must not read the box to decide, so it
/// panics deref-free. Pre-fix this detached box (`is_heap_owned() == false`)
/// slipped past the guard clause and returned an always-upgrades weak — the
/// legacy detached path the fix drops.
#[test]
#[should_panic(expected = "no active HeapGuard")]
fn f1_no_guard_downgrade_of_detached_box_panics() {
    let detached = GcRef(Gc::new_uncollected(Probe));
    let _ = detached.downgrade();
}

/// F1 (no-guard, box genuinely freed) — the deref-free proof (codex finding 5).
///
/// The heap is fully dropped before the downgrade, so the box is freed for
/// real. The deref-free `downgrade` must still panic *without reading the box*:
/// it reads only pointer bits (`identity`) and the absent guard. Pre-fix,
/// `is_heap_owned()` dereferenced this freed box — a use-after-free inside the
/// safety check. That distinction is what makes this test meaningful: with the
/// fix it is a clean deterministic panic; reverting the fix turns it into UB
/// that Miri/ASan flags (a normal build may or may not crash).
#[test]
#[should_panic(expected = "no active HeapGuard")]
fn f1_no_guard_downgrade_after_heap_drop_is_deref_free() {
    let strong = {
        let heap = Heap::new();
        let guard = HeapGuard::push(&heap);
        let s = GcRef::new(Probe);
        drop(guard);
        s
        // `heap` (the last `Rc<Heap>`) drops here → `drop_all` frees `s`'s box.
    };
    let _ = strong.downgrade();
}

/// F3 (no-guard `account_buffer`). Same deref-free treatment as `downgrade`:
/// with no active guard it panics reading nothing, rather than the old
/// `is_heap_owned()` deref.
#[test]
#[should_panic(expected = "no active HeapGuard")]
fn f3_no_guard_account_buffer_panics() {
    let strong = {
        let heap = Heap::new();
        let _guard = HeapGuard::push(&heap);
        GcRef::new(Probe)
    };
    strong.account_buffer(64);
}

/// F4 (baseline) — any read of a swept box is caught by the `as_box`
/// quarantine assert. This predates #267 and is unchanged; the kit pins it so
/// the baseline coverage cannot silently regress. The box is swept on its own
/// still-open heap (parked under quarantine), so the read is memory-safe and
/// the assert is deterministic.
#[test]
#[should_panic(expected = "use-after-sweep")]
fn f4_stale_deref_trips_quarantine() {
    let heap = Heap::new_quarantined();
    heap.unpause();
    let _guard = HeapGuard::push(&heap);

    let strong = GcRef::new(Probe);
    heap.full_collect(&NoRoots);
    let _ = &*strong;
}

/// F3c/F4 (`account_buffer` on a same-heap swept box). `account_buffer`
/// intrinsically reads the box (it mutates `header.size`), so it cannot be made
/// deref-free the way the no-guard path can. On the box's own still-open heap
/// under quarantine the swept box is parked, so the intrinsic read is memory-
/// safe and lands on the `as_box` `HDR_FREED` assert — a deterministic panic
/// instead of a silent freed-header mutation.
#[test]
#[should_panic(expected = "use-after-sweep")]
fn f4_account_buffer_on_swept_box_trips_quarantine() {
    let heap = Heap::new_quarantined();
    heap.unpause();
    let _guard = HeapGuard::push(&heap);

    let strong = GcRef::new(Probe);
    heap.full_collect(&NoRoots);
    strong.account_buffer(64);
}

// ---------------------------------------------------------------------------
// Residuals (#[ignore]): require spec option B — issue #267 F2a/F2b/F2c
//
// Each asserts the DESIRED end state (a stale handle must not upgrade). They
// fail today because closing them cheaply is impossible: validating the handle
// means reading a possibly-freed box, which is the use-after-free. `upgrade()`
// itself only consults the token map (no box deref), so running these under
// `--ignored` is memory-safe and demonstrates the resurrection directly.
// ---------------------------------------------------------------------------

/// F2a (foreign heap, box still live). Downgrading under a *different* live
/// heap mints a token in the wrong heap's map, so the handle resurrects a
/// pointer into the owner's memory after the owner frees it. Desired: the weak
/// must not upgrade. Requires option B (owner identity in the handle).
#[test]
#[ignore = "issue #267 residual: requires option B — validating a foreign handle means reading a possibly-freed box (the UAF)"]
fn f2a_foreign_live_downgrade_resurrects_residual() {
    let heap_a = Heap::new_quarantined();
    let heap_b = Heap::new_quarantined();

    let strong = {
        let _guard_a = HeapGuard::push(&heap_a);
        GcRef::new(Probe)
    };

    let _guard_b = HeapGuard::push(&heap_b);
    let weak = strong.downgrade();
    assert!(
        weak.upgrade().is_none(),
        "DESIRED (option B): a foreign-heap downgrade must not produce an \
         upgradable handle; today it resurrects"
    );
}

/// F2b (foreign heap, box already swept). Box swept by A (parked under
/// quarantine), then downgraded under B — B mints a token for the freed
/// address. Desired: no upgrade. Requires option B.
#[test]
#[ignore = "issue #267 residual: requires option B — validating a foreign handle means reading a possibly-freed box (the UAF)"]
fn f2b_foreign_swept_downgrade_resurrects_residual() {
    let heap_a = Heap::new_quarantined();
    heap_a.unpause();
    let heap_b = Heap::new_quarantined();

    let strong = {
        let _guard_a = HeapGuard::push(&heap_a);
        let strong = GcRef::new(Probe);
        heap_a.full_collect(&NoRoots);
        strong
    };

    let _guard_b = HeapGuard::push(&heap_b);
    let weak = strong.downgrade();
    assert!(
        weak.upgrade().is_none(),
        "DESIRED (option B): a downgrade of a box swept on a foreign heap must \
         not upgrade; today it resurrects"
    );
}

/// F2c (same open heap, target swept, re-downgrade). The most in-tree-shaped
/// variant: an unrooted `GcRef` swept while its heap stays open, then
/// re-`downgrade`d — the token is re-minted for the freed address and `upgrade`
/// hands back the swept box. No owner tag can see it (the owner still matches);
/// only reading the box (the UAF) or option B's per-slot generation detects it.
#[test]
#[ignore = "issue #267 residual: requires option B — re-minting a token for a swept address can only be caught by reading the box (the UAF) or a per-slot generation"]
fn f2c_same_heap_swept_redowngrade_resurrects_residual() {
    let heap = Heap::new_quarantined();
    heap.unpause();
    let _guard = HeapGuard::push(&heap);

    let strong = GcRef::new(Probe);
    heap.full_collect(&NoRoots);
    let weak = strong.downgrade();
    assert!(
        weak.upgrade().is_none(),
        "DESIRED (option B): re-downgrading a swept handle must not upgrade; \
         today the re-minted token resurrects it"
    );
}
