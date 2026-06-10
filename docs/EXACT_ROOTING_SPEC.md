# Exact-rooting audit — spec for issue #140

Status: SPEC (2026-06-10). The next major work item, ahead of all further
perf packets. Companion evidence lives in issue #140; the dead-key fix that
opened this line of work landed as `9c5125c` (canary
`canary_p_deadkey_tombstone`).

## 0. Why this outranks everything else

ASAN found a FAMILY of latent use-after-frees: objects the VM still uses
get swept because the GC's root set does not exactly cover them. These are
cadence-dependent — any allocation-size or pacing change re-rolls whether a
given program silently reads recycled memory or segfaults. Three instances
are known; the class is systemic. Until the root set is provably exact,
every perf packet is a dice roll on top of memory corruption, and a release
would ship a known deterministic segfault (`target/release/lua-rs
harness/impl/official/db.wrap.lua` crashes every run).

**Goal.** Make the statement "every object reachable by future VM execution
is reachable from the root trace at every collection point" mechanically
checked, not vibes. Fix the two open bugs as corollaries.

**Definition of done.**
- The stress+ASAN battery (P0) runs the FULL official suite plus the GC
  canaries with zero sanitizer reports, in both GC modes.
- The official suite passes with the RELEASE-profile binary (the gap that
  hid bug A), and that run is part of the PR gate.
- Issue #140's two bugs have dedicated canaries that fail on their parent
  commits.
- No perf regression outside the tolerance policy on the standard A/B
  matrix (exactness changes touch call/return hot paths — gate them like
  any packet).

**Non-goals.** No GC algorithm redesign; no switch away from index-based
stacks; no `unsafe` additions.

## 1. Evidence inventory (what we know going in)

| # | Bug | State | Repro | ASAN signature |
|---|---|---|---|---|
| 1 | Dead-key tombstones: nil'd table entries left dereferenceable freed keys | FIXED `9c5125c` | `canary_p_deadkey_tombstone` (11 lines, deterministic) | READ 1 in `TableNode::key_is_short_str`, freed by sweep, alloc'd by lexer `intern_str` |
| 2 | Coroutine traceback reads swept `LuaLClosure` (#140 bug A) | OPEN | `target/release/lua-rs harness/impl/official/db.wrap.lua` → exit 139 EVERY run; ASAN debug build trips without stress | READ 8 in `debug::funcname_from_call` → `ci_lua_proto` (`get_at(ci.func)` → `cl.proto`); freed by `sweep_young_range` (minor); alloc'd by `OP_CLOSURE`/`push_closure`; call path `debug.traceback(co)` ← db.lua ~734-806 |
| 3 | Root tracer derefs stale debug-local slots (#140 bug B) | OPEN | `LUA_RS_GC_STRESS=1` + ASAN on full db.lua | READ in `Marker::mark_box` ← `LuaState::trace` — the `trace_debug_locals` pass feeds a stale slot's GcRef into the marker |

Hard-won method facts:
- ASAN build: `RUSTFLAGS=-Zsanitizer=address cargo +nightly build -p
  lua-cli --target aarch64-apple-darwin`. lldb cannot attach in the agent
  environment; macOS crash reports are unreadable; gmalloc did not trip.
  ASAN-first is the rule for this class.
- `LUA_RS_GC_STRESS=1` (HARDMEMTESTS equivalent, landed `9c5125c`) makes
  `would_collect`/`step` fire at every checkpoint. It surfaced bug 3 in
  seconds. It does NOT reproduce bug 2 (stress changes the cadence that
  bug needs) — stress finds anchoring bugs at allocation points, but
  bugs that need a collect in a NARROW window between two distant VM
  events can hide from it. Both instruments are needed.
- GC-cadence bugs are NOT line-bisectable: early-exit injection perturbs
  allocation history and masks them (measured: injection at line 824 made
  the full-file crash vanish).
- db.lua tests pattern-match their own filename — standalone section
  repros must live at a path ending in `db.lua`.

## 2. The problem statement (why the class exists)

C-Lua's rooting model is implicit but total: every live value is reachable
from `L->stack[0 .. L->top]` (plus ci chain tops during calls), the
registry, globals via the registry, open upvalues, and the fixed objects —
and C maintains the discipline that **`luaC_checkGC` is only called at
points where everything live is anchored in one of those places**
(`lvm.c` checks it between instructions with `L->top` set; allocation
helpers anchor intermediates on the stack first).

Our port has four structural deviations, each a root-gap class:

1. **Frame-bounded trace ranges.** `LuaState::trace` walks the ci chain
   and traces `[ci.func .. next_ci.func)` per frame, `[ci.func ..
   self.top)` for the current frame — deliberately NOT `[.. ci.top]`
   because the reserved tail "can contain stale values from previous
   calls". The compensation is (2).
2. **The debug-local heuristic.** Named locals above `top` are traced via
   `get_local_name(proto, n, saved_pc)` metadata. `saved_pc` is only
   current at call boundaries — mid-frame it is stale, so the heuristic
   can BOTH under-cover (miss a live local → bug-A-like) and over-read
   (trace a slot whose value is garbage → bug 3).
3. **Rust-frame temporaries.** VM opcode arms, stdlib C-functions, and
   debug machinery hold `GcRef`s in Rust locals across allocation points.
   The GC cannot see Rust frames. C avoids the equivalent by anchoring on
   the Lua stack before any allocation; our code sometimes does not
   (bug 2's victim is reachable only via a coroutine frame the walk
   apparently does not cover, or via a Rust local during traceback).
4. **Suspended/finished coroutines.** A coroutine's frames are traced
   through its `LuaThread` object via the same range logic, with
   `cached_thread_id`-gated extras. Yield/resume/death transitions move
   `top` and the ci chain in ways the range logic must exactly track —
   and the coroutine snapshot pools (`04cd144`) added a parallel rooting
   structure that must agree with it.

## 3. Phases

### P0 — instruments first (the battery)

Build `harness/asan-stress.sh`:
- Caches the nightly ASAN build keyed on commit sha (bincache pattern).
- Runs, under BOTH `LUA_RS_GC_STRESS=1` and stress-off, in BOTH GC modes:
  all GC canaries, the deadkey/#140 repro set, and the full official
  suite (stress full-suite will be slow — provide `--quick` running the
  canaries + db/gc/coroutine/locals/api subset; full sweep is the
  end-of-phase gate).
- Greps ASAN reports; any report = FAIL with the report saved as
  evidence. Exit codes wired for CI use.
- Add a RELEASE-profile `run_official_all.sh` invocation (`LUA_RS_BIN`
  override exists) to the PR gate — this alone would have caught bug 2
  years... weeks earlier.

Acceptance: battery runs; documents current state (expected: bug-2 trip
on db, bug-3 trip under stress); wired into `harness/CLAUDE.md` + bench
README.

### P1 — root-source inventory (pre-computed analysis, not archaeology)

Produce `ANALYSES/GC_ROOTS.md`: every place a `GcRef`/`Gc` can live,
audited against "who traces this, and is it traced at every
`would_collect` checkpoint?":

stack slots (per-frame ranges + top discipline), ci chain func slots,
globals/registry, open + closed upvalues, hook registry
(`registry[HOOKKEY]`, weak-keyed) AND the VM-level hook closure box,
coroutine LuaStates + snapshot pools + yielded values, to-be-closed list,
metatables (incl. `setmetatable(t,t)` self-cycles), string table (weak by
design — dead-interned removal contract), pending finalizer lists,
lua-rs-runtime embedding handles, and — the open-ended class — Rust-frame
temporaries in lua-vm/lua-stdlib (enumerate by grepping allocation calls
reachable inside opcode arms and C-functions; each call site needs an
"anchored where?" answer).

Each row gets: traced-by, checkpoint coverage, and a canary exercising it
under stress+ASAN (the fast in-memory tester doctrine — most can be
5-line Lua snippets in the canary battery).

### P2 — strategy decision (measure, then choose)

The core design choice for the stack, decided by spike + recount, not
taste:

- **(a) C-style top discipline.** Make `state.top` always cover live
  slots and audit every allocation checkpoint for anchoring. Faithful,
  zero over-marking, but it is a forever-discipline on every future
  contributor — and the current range logic exists precisely because our
  top is NOT maintained that way.
- **(b) Range widening + pop hygiene.** Trace `[ci.func .. ci.top]` for
  every frame, and CLEAR slots on frame pop / top decrease so stale slots
  are nil (kills bug 3's input AND removes the need for the debug-local
  heuristic entirely — delete it). Cost: clearing on the call-return hot
  path. MEASURE with the Ir rig (`call_only` probe + call_return_shapes)
  before committing; budget tolerance per the standard gate. Note C
  effectively pays this cost differently (its top discipline keeps the
  window small).
- **(c) Anchor API for Rust temporaries.** Whatever (a)/(b) decides, the
  Rust-frame class needs either a scoped anchor (`state.anchor(value)`
  RAII pushing to a traced side-stack) or a rule that C-function code
  keeps values on the Lua stack across allocations. Inventory P1 decides
  how many sites exist; if few, fix in place; if many, build the API.

Spike deliverables: Ir delta for clear-on-pop; A/B on call-heavy rows;
written decision in this spec.

### P3 — fix bug A (coroutine traceback)

With P0's battery + P1's coroutine rows: reproduce bug 2 in a dedicated
canary (suspend/finish coroutines, force minor collects via stress,
traceback at every state), fix per the P2 strategy, prove with ASAN +
canary + release-profile db.wrap green ×10 runs.

### P4 — fix bug B (debug-local tracing)

Falls out of P2(b) if widening is chosen (delete the heuristic). If (a)
is chosen instead: make the heuristic read-safe (only trace slots below a
verified live-water mark) or replace with precise liveness from the
function's `maxstacksize`/active-range metadata at a SAVED pc that is
guaranteed current (save pc before any checkpoint that can collect —
audit). Same proof obligations.

### P5 — drive the battery clean

Full official suite + canaries under stress+ASAN, both GC modes, zero
reports. This is the real DoD. Time-box the full-suite stress runs
(stress is slow); quarantine-with-justification anything that cannot run
stressed, mirroring the bench manifest pattern.

### P6 — unwind the queue

- Re-gate and land the stashed R2 (LuaTable field diet — stash
  `R2-luatable-field-diet`, table box 144 → 128 B).
- Re-run the perf wrap matrices (stock + PGO) — exactness changes will
  have moved numbers; the PGO retraining follow-up from the model doc
  rides along.
- THEN release (see §5).

## 4. Risks and traps

- **Over-marking is not free-safe.** Widened ranges trace stale slots —
  garbage refs in the marker is bug 3 by another road. Widening WITHOUT
  pop-clearing is strictly worse than today. The pair is atomic.
- **Resurrection semantics.** Over-marking can keep weak-table entries /
  finalizable objects alive a cycle longer than C. The weak/ephemeron
  canaries and gc.lua/gengc.lua are the oracle; any divergence is a
  failed gate, not a judgment call.
- **Perf.** Pop-clearing and anchor traffic sit on call/return paths the
  W2.2 diet was about to attack. Every exactness change goes through the
  standard packet gate (recount + interleaved A/B + tolerance policy).
  The two workstreams meet in P6.
- **Stress-mode re-entrancy.** `would_collect` always-true must never
  trigger collection FROM collection paths (current stress implementation
  rides existing checkpoints only — keep it that way; assert in debug).
- **The instruments lie last.** Bug 2 hides from stress; bug 3 hides
  without it. P5 requires BOTH configurations, both GC modes, both build
  profiles.

## 5. Release policy

No release until P5. The current deterministic release-profile segfault
(bug A) means a release today ships a known crash — worse, the release CI
itself runs release-profile binaries through `make perf-pgo`'s
conformance gate and may go red on db.lua nondeterministically. After P5,
the standard `RELEASING.md` flow applies, and the release notes get the
correctness-fix headline (dead keys + rooting audit), which is also the
honest changelog story for why this release matters more than its perf
numbers.

## 6. Acceptance checklist

- [ ] P0 battery exists, wired, documented; release-profile suite in PR gate
- [ ] `ANALYSES/GC_ROOTS.md` complete with per-row canaries
- [ ] P2 strategy decision recorded with spike numbers
- [ ] Bug A fixed: canary + ASAN clean + release db.wrap green ×10
- [ ] Bug B fixed: canary + stress+ASAN clean on db.lua
- [ ] Full suite + canaries clean under stress+ASAN, both modes, both profiles
- [ ] Perf gates green (or tracked regressed-minor) on exactness changes
- [ ] R2 landed; fresh matrices; release shipped per RELEASING.md
