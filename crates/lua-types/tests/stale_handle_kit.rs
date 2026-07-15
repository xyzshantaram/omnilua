//! stale_handle_kit — deterministic in-memory reproductions of the issue #267
//! stale-`GcRef` unsoundness cases.
//!
//! These are the "custom subsystem tester" for GC owner-identity safety: they
//! construct a `Heap` + `Gc`/`GcRef` directly and script the exact failure
//! shapes from `specs/ISSUE_267_OWNER_IDENTITY_SPEC.md` §2, so each bug
//! reproduces in milliseconds, 100% of the time, with no VM/oracle in the
//! loop. The full-VM oracle *cannot* easily produce a use-after-teardown —
//! that invisibility is the whole reason the bug is subtle — so this kit is
//! what makes the fix verifiable at all.
//!
//! # What is staged deterministically here
//!
//! Every tripwire case runs on a [`Heap::new_quarantined`] heap: under
//! quarantine, sweep *parks* a dead box (sets `HDR_FREED`, keeps the memory
//! allocated) instead of freeing it, so a stale read hits intact memory and
//! trips a deterministic Rust panic rather than undefined behavior. That is
//! precisely the window in which the `downgrade`/`account_buffer` tripwires
//! can read a swept box's header safely.
//!
//! | Test | Spec case | Pre-fix behavior (red) |
//! |---|---|---|
//! | `f1_no_guard_downgrade_of_detached_box_panics` | F1 no-guard | returned an always-upgrades weak (no panic) |
//! | `f1_no_guard_downgrade_of_heap_owned_box_panics` | F1 no-guard | panicked, but only after a UAF deref of the box header |
//! | `f2a_foreign_live_downgrade_panics` | F2a foreign-live | minted a token on the wrong heap → resurrection |
//! | `f2b_foreign_swept_downgrade_panics` | F2b foreign-swept | minted a token for a freed address → resurrection |
//! | `f2c_same_heap_swept_redowngrade_panics` | F2c same-heap swept | re-minted a token → `upgrade` resurrected the freed box |
//! | `f4_stale_deref_trips_quarantine` | F4 baseline | (already covered by the `as_box` assert) |
//! | `f4_account_buffer_on_swept_box_panics` | F3/F4 account | dereferenced the swept box header |
//!
//! # What CANNOT be staged safely in-memory (documented residuals)
//!
//! - **F1 post-drop / F2b post-close (owner heap actually `drop_all`-ed).**
//!   Teardown drains the quarantined list and frees its boxes for real, so
//!   there is no parked memory left to read: any check that touches the box
//!   header *is itself* the use-after-free. These are true UB only observable
//!   under ASan/Miri, not a deterministic Rust panic. The deref-free no-guard
//!   fix removes the F1 read entirely; the foreign-post-close resurrection is
//!   the release residual that only slot-indexed handles (spec option B)
//!   close. See `f2b_foreign_swept_downgrade_panics` for the *parked* variant,
//!   which is the catchable half.
//! - **An already-issued `&T` outliving a later `drop_all(&self)`** (codex
//!   finding 3). `Deref::deref` hands out `&T`; a subsequent safe teardown
//!   frees the box while that borrow is live. No owner-identity scheme closes
//!   this — it needs an API-shape change (exclusive/`&mut` teardown or an
//!   access guard instead of `&T`), which is out of scope for #267. The real
//!   fix is a `trybuild` compile-fail test once teardown takes `&mut`.

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

/// F1 (no-guard, detached target) — issue #267.
///
/// With no active `HeapGuard`, `downgrade` cannot validate the handle against
/// any heap and must not read the (possibly freed) box to decide. The fix
/// panics unconditionally and deref-free. Pre-fix this box (a detached
/// `new_uncollected`, so `is_heap_owned() == false`) slipped past the guard
/// clause and returned an always-upgrades weak — the legacy detached path the
/// fix drops.
#[test]
#[should_panic(expected = "no active HeapGuard")]
fn f1_no_guard_downgrade_of_detached_box_panics() {
    let detached = GcRef(Gc::new_uncollected(Probe));
    let _ = detached.downgrade();
}

/// F1 (no-guard, heap-owned target) — issue #267.
///
/// The heap-owned no-guard case stayed loud before the fix, but only by
/// reading the box header (`is_heap_owned()`) — a UAF in the safety check
/// itself once the box is gone. The fix reaches the same panic without any
/// deref. The box is still alive here (guard merely popped), so this is safe
/// to stage.
#[test]
#[should_panic(expected = "no active HeapGuard")]
fn f1_no_guard_downgrade_of_heap_owned_box_panics() {
    let heap = Heap::new();
    let strong = {
        let _guard = HeapGuard::push(&heap);
        GcRef::new(Probe)
    };
    let _ = strong.downgrade();
}

/// F2a (foreign, box still live) — issue #267.
///
/// Two live heaps on one thread. A box allocated on A, downgraded while B is
/// the active guard, mints a token in B's map — later `upgrade` resurrects a
/// pointer into A's memory after A frees it. The `owner_gen` tripwire catches
/// this while the box is still live (the read is safe): the box's owner
/// generation does not match the active heap's.
#[test]
#[should_panic(expected = "foreign")]
fn f2a_foreign_live_downgrade_panics() {
    let heap_a = Heap::new_quarantined();
    let heap_b = Heap::new_quarantined();

    let strong = {
        let _guard_a = HeapGuard::push(&heap_a);
        GcRef::new(Probe)
    };

    let _guard_b = HeapGuard::push(&heap_b);
    let _ = strong.downgrade();
}

/// F2b (foreign, box already swept) — issue #267 (the parked, catchable half).
///
/// Box allocated on A, swept by A (parked under quarantine), then downgraded
/// while B is active. Pre-fix, B's `register_allocation_token` minted a token
/// for the freed address → resurrection. The `HDR_FREED` tripwire refuses:
/// the box the handle names has already been swept.
#[test]
#[should_panic(expected = "swept")]
fn f2b_foreign_swept_downgrade_panics() {
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
    let _ = strong.downgrade();
}

/// F2c (same open heap, target swept, re-downgrade) — issue #267.
///
/// The most in-tree-shaped variant and the one no owner tag can see (the owner
/// still matches). A `GcRef` local is not a GC root, so it can be swept while
/// its heap stays open; re-`downgrade`ing it re-mints a valid token for the
/// freed address (`registering_after_sweep_yields_a_distinct_token`), and
/// `upgrade` then hands back the freed box. The `HDR_FREED` tripwire is what
/// catches it: same owner, but the box is swept.
#[test]
#[should_panic(expected = "swept")]
fn f2c_same_heap_swept_redowngrade_panics() {
    let heap = Heap::new_quarantined();
    heap.unpause();
    let _guard = HeapGuard::push(&heap);

    let strong = GcRef::new(Probe);
    heap.full_collect(&NoRoots);
    let _ = strong.downgrade();
}

/// F4 (baseline) — issue #267.
///
/// Any read of a swept box is caught by the `as_box` quarantine assert. This
/// predates the tripwire and is unchanged; the kit pins it so the baseline
/// coverage cannot silently regress.
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

/// F4 / F3c (account_buffer on a swept box) — issue #267.
///
/// `account_buffer` intrinsically reads the box (it mutates `header.size`), so
/// the foreign/swept path cannot be made deref-free the way `downgrade` can.
/// The `HDR_FREED` tripwire refuses before the intrinsic read, turning what
/// was a silent freed-header mutation (pacer drift / UAF) into a deterministic
/// panic under quarantine.
#[test]
#[should_panic(expected = "swept")]
fn f4_account_buffer_on_swept_box_panics() {
    let heap = Heap::new_quarantined();
    heap.unpause();
    let _guard = HeapGuard::push(&heap);

    let strong = GcRef::new(Probe);
    heap.full_collect(&NoRoots);
    strong.account_buffer(64);
}

// PORT STATUS: complete — stale_handle_kit for issue #267 owner-identity work.
