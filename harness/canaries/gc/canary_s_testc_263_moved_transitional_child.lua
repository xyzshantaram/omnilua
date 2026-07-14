-- issue #263: a reachable young child must survive a minor collection even
-- when its only revisit root -- a gray-listed transitional old object -- is
-- moved off allgc by setmetatable-with-__gc.
--
-- Graph: root -> exact-Old parent -> gray-listed (touched1) transitional ->
-- New child. The parent is exact-Old, so a minor marker skips it
-- (should_trace_age); the transitional's grayagain revisit entry is the only
-- path by which the minor reaches the child. Applying a __gc metamethod moves
-- the transitional allgc -> finobj. Pre-fix that move deleted the grayagain
-- entry, so the following minor had no path to the child and swept the
-- reachable object. Fixed: the entry survives the move and the child lives.
--
-- The child is observed through a weak-valued table: if the minor wrongly
-- frees it, weak-table clearing nils the slot (a clean, non-dangling signal);
-- if it survives, the slot still holds it. This reproduces on pristine
-- origin/main (witness cleared -> child freed) and passes once #263 is fixed.

assert(T and T.gcage and T.totalmem, "FAIL: internal T table missing")

collectgarbage("generational")

-- Build root -> parent -> transitional and age both to exact-old together, so
-- the parent stays exact-Old (never re-touched) and the minor marker skips it.
local parent = {}
local transitional = {}
parent[1] = transitional
collectgarbage()
assert(T.gcage(parent) == "old", "FAIL: parent did not become old")
assert(T.gcage(transitional) == "old", "FAIL: transitional did not become old")

-- Store a fresh young child in the old transitional: the backward barrier
-- touches (gray-lists) the transitional; the child is new. No local holds the
-- child, so its only strong reference is the transitional[1] heap edge.
transitional[1] = {payload = 234}
assert(T.gcage(transitional) == "touched1",
  "FAIL: transitional was not touched by the young store")
assert(T.gcage(transitional[1]) == "new", "FAIL: child has wrong initial age")

-- Weak-valued witness: observes the child's liveness without rooting it.
local witness = setmetatable({}, {__mode = "v"})
witness[1] = transitional[1]

-- Move the transitional allgc -> finobj by giving it a finalizer. Pre-#263-fix
-- this deleted its grayagain revisit entry.
setmetatable(transitional, {__gc = function() end})
assert(T.gcage(transitional) == "touched1",
  "FAIL: transitional age changed unexpectedly at setmetatable")

-- Drop the local root so the transitional is reachable only via parent (an
-- exact-Old object the minor skips) plus its own grayagain entry.
transitional = nil

-- A minor collection. Pre-fix: no path to the child, so the reachable child is
-- swept and the weak witness is cleared. Post-fix: the surviving grayagain
-- entry re-marks the transitional and its child lives.
collectgarbage("step", 0)

assert(witness[1] ~= nil,
  "FAIL(#263): reachable young child was freed by the minor after its \
transitional parent was moved off allgc")
assert(witness[1].payload == 234, "FAIL(#263): young child payload corrupted")
assert(parent[1] ~= nil, "FAIL: transitional edge lost")

print("PASS canary_s")
