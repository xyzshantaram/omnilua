//! `GcRef<T>` — the GC-managed reference handle.
//!
//! Newtype around `lua_gc::Gc<T>` — Copy under the hood, tracks allocation
//! in the active `Heap` (via `lua_gc::with_current_heap(...)`). An earlier
//! revision was a thin `Rc<T>` newtype instead; the surface stayed stable
//! across the swap (`new`, `ptr_eq`, `identity`, `strong_count`,
//! `weak_count`, `downgrade`), and existing code touching `gc.0` continues
//! to work — `.0` is now `Gc<T>` instead of `Rc<T>`.
//!
//! # Weak refs
//!
//! Heap-tracked `GcWeak<T>` handles remember the heap active when they were
//! created plus the target's heap allocation token. They upgrade only while
//! that identity/token pair remains live. Handles to legacy uncollected boxes
//! still upgrade forever, matching their process-lifetime allocation model.

use lua_gc::{Gc, HeapRef, Trace};

/// A GC-managed pointer to a Lua collectable object. Newtype over
/// `lua_gc::Gc<T>` so callers preserve `gc.0`-shape access while the
/// backend swaps under them.
#[derive(Debug)]
pub struct GcRef<T: Trace + 'static>(pub Gc<T>);

impl<T: Trace + 'static> GcRef<T> {
    /// Allocate a new GC-tracked value on the active heap: joins the allgc
    /// chain under a `HeapGuard` (set by `state.run()` / `pcall_k` /
    /// `with_state`), or the heap's bootstrap list inside a bootstrap
    /// window.
    ///
    /// Panics if no heap is active. There is no fallback: the old detached
    /// arm allocated a box no heap ever freed (issue #249's leak class), and
    /// since #253 moved host-side `LuaError` messages to owned bytes, no
    /// legitimate guard-less allocation path remains — a panic here is
    /// always a missing guard on the entry path, and the message says so.
    pub fn new(value: T) -> Self {
        let gc = lua_gc::with_current_heap(|heap| match heap {
            Some(heap) => heap.allocate(value),
            None => panic!(
                "GcRef::new::<{}> with no active HeapGuard — a detached allocation \
                 would never be freed (issue #249 class); push a HeapGuard or \
                 bootstrap window on the entry path",
                std::any::type_name::<T>()
            ),
        });
        GcRef(gc)
    }
}

impl<T: Trace + 'static> GcRef<T> {
    /// Two `GcRef`s are identity-equal iff they point at the same box.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Gc::ptr_eq(a.0, b.0)
    }

    /// Identity as a usize — used as a HashMap key for "same object" tests.
    pub fn identity(&self) -> usize {
        self.0.identity()
    }

    /// Number of strong references. `GcRef` is not reference-counted, so a live
    /// handle reports one owning GC reachability handle.
    pub fn strong_count(&self) -> usize {
        1
    }

    /// Number of weak references. Weak handles are not counted.
    pub fn weak_count(&self) -> usize {
        0
    }

    /// Get a weak handle. If this allocation belongs to the currently-active
    /// heap, the weak handle will stop upgrading once sweep removes that exact
    /// heap allocation.
    ///
    /// # No-guard path is deref-free (issue #267 F1)
    ///
    /// With no active `HeapGuard` there is no heap to validate against, and the
    /// handle **must not** read the box to decide — if its owning heap has been
    /// dropped, the box is freed and that read is itself the use-after-free (the
    /// old `is_heap_owned()` check was exactly this UAF-in-the-safety-check). So
    /// this panics unconditionally, reading nothing, consistent with
    /// [`GcRef::new`]'s guard-less panic and the always-on guard policy. The
    /// legacy detached-box "always upgrades on a guard-less downgrade" path is
    /// dropped with it.
    ///
    /// # Guarded path tripwire (issue #267 F2a/F2b/F2c)
    ///
    /// Under quarantine mode the guarded path first runs
    /// [`Heap::stale_handle_tripwire`], which refuses a downgrade that names an
    /// already-swept box (`HDR_FREED`) or a box owned by a different live heap
    /// generation (`owner_gen`) — turning the same-heap swept-re-downgrade
    /// resurrection and the foreign-heap mint into deterministic panics. In
    /// release the tripwire is a no-op branch.
    ///
    /// A `GcRef` obtained before its heap closed (`Heap::drop_all` / `close`) is
    /// dangling afterwards. When the *closed* heap is the active guard, the
    /// closed-heap token refusal makes the resulting weak handle permanently
    /// dead. The remaining unsound corners (foreign-heap use of a box whose
    /// owner has already run `drop_all`, and an already-issued `&T` outliving a
    /// later `drop_all(&self)`) are release residuals that only slot-indexed
    /// handles (issue #267 option B) close; see the spec.
    pub fn downgrade(&self) -> GcWeak<T> {
        let identity = self.identity();
        match lua_gc::with_current_heap(|heap| {
            heap.map(|heap| {
                heap.stale_handle_tripwire(self.0);
                let token = heap.register_allocation_token(identity);
                (HeapRef::from_heap(heap), token)
            })
        }) {
            Some((heap, allocation_token)) => GcWeak {
                target: self.0,
                identity,
                allocation_token,
                heap: Some(heap),
            },
            None => panic!(
                "GcRef::downgrade::<{}> with no active HeapGuard — a GcRef \
                 operated outside its owning heap's guard cannot be validated \
                 against any heap, and reading the box to decide would be a \
                 use-after-free if the heap has been dropped; push a HeapGuard \
                 on the entry path",
                std::any::type_name::<T>()
            ),
        }
    }

    /// Charge (`delta > 0`) or refund (`delta < 0`) bytes of this object's
    /// owned heap buffers against the active heap's pacer, so collections
    /// fire at honest memory pressure. No-op on `delta == 0` or when the
    /// underlying box is uncollected (see [`lua_gc::Gc::account_buffer`]).
    ///
    /// # No-guard path is deref-free (issue #267 F3)
    ///
    /// With no active `HeapGuard` this panics unconditionally, reading nothing:
    /// the charge would otherwise be silently dropped (pacer drift), and — as
    /// with [`downgrade`](Self::downgrade) — reading the box to decide would be
    /// a use-after-free if the heap has been dropped (the old `is_heap_owned()`
    /// check was that UAF). The legacy detached no-op path is dropped with it.
    ///
    /// The guarded path first runs [`Heap::stale_handle_tripwire`] (a no-op
    /// outside quarantine), so a charge against an already-swept or foreign box
    /// panics deterministically instead of mutating a freed header. Unlike
    /// `downgrade`, the guarded charge itself intrinsically reads the box (it
    /// mutates `header.size`), so it cannot be made deref-free in release — that
    /// residual is documented in the spec.
    pub fn account_buffer(&self, delta: isize) {
        if delta == 0 {
            return;
        }
        match lua_gc::with_current_heap(|h| {
            h.map(|h| {
                h.stale_handle_tripwire(self.0);
                self.0.account_buffer(h, delta);
            })
        }) {
            Some(()) => {}
            None => panic!(
                "GcRef::account_buffer::<{}>({delta}) with no active HeapGuard \
                 — the charge would be silently dropped and the pacer would \
                 drift from real memory, and reading the box to decide would be \
                 a use-after-free if the heap has been dropped; push a HeapGuard \
                 on the entry path",
                std::any::type_name::<T>()
            ),
        }
    }
}

/// A weak handle to a `GcRef<T>`.
#[derive(Debug)]
pub struct GcWeak<T: Trace + 'static> {
    target: Gc<T>,
    identity: usize,
    allocation_token: usize,
    heap: Option<HeapRef>,
}

impl<T: Trace + 'static> GcWeak<T> {
    /// Try to promote to a strong reference.
    pub fn upgrade(&self) -> Option<GcRef<T>> {
        if let Some(heap) = &self.heap {
            if !heap.contains_allocation(self.identity, self.allocation_token) {
                return None;
            }
        }
        Some(GcRef(self.target))
    }

    /// Strong reference count of the target from this weak handle's point of
    /// view: one while it can still upgrade, zero after sweep.
    pub fn strong_count(&self) -> usize {
        usize::from(self.upgrade().is_some())
    }

    pub fn identity(&self) -> usize {
        self.identity
    }
}

impl<T: Trace + 'static> Clone for GcWeak<T> {
    fn clone(&self) -> Self {
        GcWeak {
            target: self.target,
            identity: self.identity,
            allocation_token: self.allocation_token,
            heap: self.heap.clone(),
        }
    }
}

impl<T: Trace + 'static> Clone for GcRef<T> {
    fn clone(&self) -> Self {
        GcRef(self.0)
    }
}

impl<T: Trace + 'static> Copy for GcRef<T> {}

impl<T: Trace + 'static> std::ops::Deref for GcRef<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.0
    }
}

impl<T: Trace + 'static> AsRef<T> for GcRef<T> {
    fn as_ref(&self) -> &T {
        &*self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lua_gc::Marker;

    struct NoRoots;

    impl Trace for NoRoots {
        fn trace(&self, _m: &mut Marker) {}
    }

    #[derive(Debug)]
    struct Cell0;

    impl Trace for Cell0 {
        fn trace(&self, _m: &mut Marker) {}
    }

    #[test]
    fn heap_tracked_weak_refs_stop_upgrading_after_sweep() {
        let heap = lua_gc::Heap::new();
        heap.unpause();
        let _guard = lua_gc::HeapGuard::push(&heap);

        let strong = GcRef::new(Cell0);
        let weak = strong.downgrade();
        assert!(weak.upgrade().is_some());
        assert_eq!(weak.strong_count(), 1);

        heap.full_collect(&NoRoots);
        assert!(weak.upgrade().is_none());
        assert_eq!(weak.strong_count(), 0);
    }

    /// The detached fallback is gone (#253): a guard-less allocation is a
    /// bug on the entry path, and it must fail loudly in every build, not
    /// leak quietly for the life of the process.
    #[test]
    #[should_panic(expected = "no active HeapGuard")]
    fn guardless_allocation_panics() {
        let _ = GcRef::new(Cell0);
    }

    /// A guard-less `downgrade` of a heap-owned box mints a `GcWeak` with no
    /// heap identity, so it upgrades forever — including after sweep frees the
    /// target (use-after-free). That now panics unconditionally, not just
    /// under a retired env flag.
    #[test]
    #[should_panic(expected = "no active HeapGuard")]
    fn guardless_downgrade_of_heap_owned_box_panics() {
        let heap = lua_gc::Heap::new();
        let strong = {
            let _guard = lua_gc::HeapGuard::push(&heap);
            GcRef::new(Cell0)
        };
        let _ = strong.downgrade();
    }

    /// A guard-less `account_buffer` on a heap-owned box would silently drop
    /// the pacer charge, drifting the collector from real memory. That now
    /// panics unconditionally, not just under a retired env flag.
    #[test]
    #[should_panic(expected = "no active HeapGuard")]
    fn guardless_account_buffer_on_heap_owned_box_panics() {
        let heap = lua_gc::Heap::new();
        let strong = {
            let _guard = lua_gc::HeapGuard::push(&heap);
            GcRef::new(Cell0)
        };
        strong.account_buffer(64);
    }

    /// Codex finding 2 on issue #260: downgrading a stale `GcRef` AFTER the
    /// heap closed (its box already freed by `drop_all`) must yield a weak
    /// handle that never upgrades. Before the closed-heap token refusal,
    /// `downgrade` re-registered the freed box's address in the token map and
    /// the weak handle upgraded into freed memory.
    #[test]
    fn downgrade_after_close_cannot_resurrect_freed_box() {
        let heap = lua_gc::Heap::new();
        heap.unpause();
        let _guard = lua_gc::HeapGuard::push(&heap);

        let stale = GcRef::new(Cell0);
        heap.drop_all();

        let weak = stale.downgrade();
        assert!(
            weak.upgrade().is_none(),
            "a weak handle minted after close must never upgrade — the box \
             is freed and the heap is closed"
        );
    }
}

impl<T: PartialEq + Trace + 'static> PartialEq for GcRef<T> {
    fn eq(&self, other: &Self) -> bool {
        Gc::ptr_eq(self.0, other.0) || **self == **other
    }
}
