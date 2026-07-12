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

TODO

## Wave 1 — grayagain becomes a heap-owned Vec

TODO — lifecycle, ordering, dedup-flag deletion paths, drop_all, quarantine.

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
