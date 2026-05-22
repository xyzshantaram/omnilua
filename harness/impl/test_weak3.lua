-- __gc x weak tables
local u = setmetatable({}, {__gc = true})
setmetatable(getmetatable(u), {__mode = "v"})
getmetatable(u).__gc = function (o) os.exit(1) end
u = nil
collectgarbage()

local u = setmetatable({}, {__gc = true})
local m = getmetatable(u)
m.x = {[{0}] = 1; [0] = {1}}; setmetatable(m.x, {__mode = "kv"});
m.__gc = function (o)
  assert(next(getmetatable(o).x) == nil)
  m = 10
end
u, m = nil
collectgarbage()
print("after second collectgarbage: m =", m)
assert(m==10)
print("m==10 passed")

do
  collectgarbage(); collectgarbage()
  local m = collectgarbage("count")
  local a = setmetatable({}, {__mode = "kv"})
  a[string.rep("a", 2^22)] = 25
  a[string.rep("b", 2^22)] = {}
  a[{}] = 14
  assert(collectgarbage("count") > m + 2^13)
  collectgarbage()
  assert(collectgarbage("count") >= m + 2^12 and
        collectgarbage("count") < m + 2^13)
  local k, v = next(a)
  assert(k == string.rep("a", 2^22) and v == 25)
  assert(next(a, k) == nil)
  assert(a[string.rep("b", 2^22)] == nil)
  a[k] = nil
  k = nil
  collectgarbage()
  print("FINAL: next(a) =", next(a))
  assert(next(a) == nil)
  assert(a[string.rep("b", 100)] == nil)
  assert(collectgarbage("count") <= m + 1)
end
print("all passed")
