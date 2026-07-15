# Backlog-crush campaign — 2026-07-15

Goal: drive the open-issue count to zero (or to a defensible parked state
with evidence), autonomously, under the supervisor+subagent pattern. The
supervisor (Fable) does the upfront planning here, builds/specs the custom
kits each ticket needs for a fast inner loop, runs the heavy gates + codex
rounds, and decides keep-vs-nuke by evidence. Honest negatives are
first-class outcomes — a well-evidenced "close as won't-fix" or
"measured-neutral, reverted" counts as crushing the ticket.

This file is the referenceable source of truth. It is updated as tickets
resolve. Do not trust a hardcoded status elsewhere — the GitHub issue state
and this table's "Outcome" column are authoritative.

## The open tickets (at campaign start)

| # | Title (short) | Class | Approach | Kit | Verdict authority |
|---|---|---|---|---|---|
| 267 | Gc boxes carry no owner identity | GC soundness | Implement spec-recommended **C/D** (deref-free guards + free HDR_FREED tripwire + u8 owner-id in padding + seal the raw surface). Spec PR #290 merges first as docs. | **stale_handle_kit** (new) | supervisor + codex |
| 282 | loadlib 5.1/5.2 path/env fidelity | oracle fidelity | Oracle-driven fidelity fixes vs the 5 reference binaries, follow-up to #273. | existing loadlib oracle probes | supervisor + oracle |
| 291 | FREELIST_REF collides with 5.5 mainthread slot | latent version bug | Version-gate FREELIST_REF cheaply; unreachable today (luaL_ref has no callers) so no test for the uncallable path — insurance only. Fold into #282's loadlib lane. | (none) | supervisor |
| 278 | Embedding-API stubs grab-bag | judgment / triage | Triage each sub-item into FIX / DOCUMENT / CLOSE by common sense (real embedding correctness → fix; niche/uncallable → document or close). Split the parse-codegen items to their own issue if kept. | **embedding_api_kit** (new, as needed) | supervisor |
| 113 | RSS object diet (parked, W2 killed) | perf, measure-first | **Analysis first, no blind surgery.** Build the size-class histogram tool; rank shrink candidates by malloc-bucket-crossing × population (the only lever that's actually paid off — see PERF_EVIDENCE_113_W2). Implement ONLY a concretely-measured bucket-crossing win; else document the finding and park with evidence. | **size_class_histogram** (new tool) | supervisor + Ir/RSS |

## Iteration-cycle analysis (where the inner loop lives, and the kit that speeds it)

The discipline (CLAUDE.md "custom subsystem testers"): before grinding a
subsystem against the slow end-to-end oracle, build a small in-memory
deterministic kit that exercises exactly that part. Per ticket:

- **#267** — the inner loop is *use-after-close / foreign-heap scenarios*.
  The full-VM oracle CANNOT easily produce a use-after-teardown (that's the
  whole reason the bug is invisible). So the inner loop must be a
  **stale_handle_kit**: construct a `Heap` + `Gc`/`GcRef` directly, script
  the F1/F2b/F2c/F4 failure cases from the spec (no-guard downgrade of a
  freed box; foreign-heap token mint; same-heap swept-then-re-downgrade;
  account_buffer on stale), and assert the guard/tripwire fires. Milliseconds,
  100%-reproducible, where a real socket/VM reproduces it never. This kit is
  the deliverable that makes the fix verifiable at all.

- **#282** — inner loop is `package.path`/`cpath`/env-precedence bytes vs
  each reference version. The kit already exists: `specs/oracle/diff_one.sh
  <ver>` + the hermetic `HostHooks::env` fake from #273's
  loadlib_strengthen.rs. Rung 3 (diff_one) is the loop; rung 6 (official +
  check.sh ×5) is the gate. No new kit.

- **#278** — inner loop is *per-API behavior from the host side*. Where an
  item is behaviorally checkable (to_close/TBC, to_cfunction, the "0x?"
  address placeholder), drive it from a Rust test comparing to reference
  semantics — an **embedding_api_kit** only if the existing
  crates/lua-rs-runtime/tests/ harnesses don't already reach it. Many items
  are triage-by-reading, not code (uncallable stubs → close).

- **#113** — inner loop for the ANALYSIS is a **size_class_histogram** tool
  (new): allocate a representative workload, dump every live `GcBox<T>`'s
  `size_of` against the platform allocator's size-class table (macOS
  nano/scalable zones: 16-byte quanta to 256, then 512-byte, etc.), and
  rank each object type by (bucket-step-if-shrunk × live-population). This
  is a READ-ONLY analysis kit — it decides whether any surgery is worth it
  BEFORE surgery. If it finds a win, the implement inner loop is instr-count
  (Ir, deterministic, cachegrind) + heap-diff for RSS, per MEASUREMENT_PROTOCOL.

## Keep-vs-nuke decision framework (the common-sense rules)

- **A change ships** iff: it fixes a confirmed-reachable bug OR removes dead
  code OR is oracle-verified fidelity, AND passes the full gate battery, AND
  a codex round finds no unaddressed correctness defect. Comment-only/doc
  changes need only the mechanical + build gates.
- **A change is nuked (reverted/closed)** iff: measured-neutral/negative on
  its own success metric (the W2 precedent), OR it targets an uncallable
  path where the fix adds risk without reachable benefit, OR the "bug" turns
  out to be correct-as-is on oracle inspection. A nuke with evidence closes
  the ticket as effectively as a merge.
- **An item is documented-and-parked** iff: it's a real gap but the fix is
  disproportionate to reach (niche embedding features, multi-day redesigns
  whose payoff isn't yet justified). Park with a precise doc + issue note,
  don't leave a silent gap.

## Wave plan (file-disjoint parallelism; supervisor merges + rebases)

**Wave A** (launch together — file-disjoint):
- #267 → Opus, owns lua-types/gc.rs + lua-gc/heap.rs + lua-rs-runtime seal
  surface; builds stale_handle_kit first.
- #282+#291 → Sonnet, owns lua-stdlib/loadlib.rs.
- #113-analysis → Opus, READ-ONLY (builds size_class_histogram, produces a
  findings doc + go/no-go verdict); no source mutation, so it cannot collide.

**Wave B** (after A resolves the file locks):
- #278 → triage lane; fixes land after #267 frees api.rs if they overlap;
  #291 already folded into #282.
- #113-implement → ONLY if the analysis returns a concrete bucket-crossing
  win; sequenced after #267 (both touch heap.rs).

## Operating rules for every lane (carried from the proven method)

Background+poll any command >2 min; incremental commits from edit one; phase
pings to main only; ≤400-line read slices; stage files explicitly (never
`git add -A`, never stage reference/lua-c or harness/impl/official/); NEVER
`git stash` in a worktree (shared refs/stash — CLAUDE.md); push via the
`gh auth token -u ianm199` one-shot credential override; agents live on
ladder rungs 1–4, supervisor runs official/Ir/RSS/codex/merge; stop-the-line
on any canary/official flip; codex fix-rounds capped at ~3 with triage
(FIX / DEFER-to-issue / REBUT).

## Outcome log (updated as tickets resolve)

- (pending)
