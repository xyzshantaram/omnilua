-- repro state
local u = setmetatable({}, {__gc = true})
setmetatable(getmetatable(u), {__mode = "v"})
getmetatable(u).__gc = function (o) os.exit(1) end
u = nil
collectgarbage()
local u = setmetatable({}, {__gc = true})
local m_ = getmetatable(u)
m_.x = {[{0}] = 1; [0] = {1}}; setmetatable(m_.x, {__mode = "kv"});
m_.__gc = function (o)
  assert(next(getmetatable(o).x) == nil)
  m_ = 10
end
u, m_ = nil
collectgarbage()

do
  collectgarbage(); collectgarbage()
  local m = collectgarbage("count")
  print("baseline m=", m)
  local a = setmetatable({}, {__mode = "kv"})
  a[string.rep("a", 2^22)] = 25
  a[string.rep("b", 2^22)] = {}
  a[{}] = 14
  local c1 = collectgarbage("count")
  print("after fill, c1=", c1, "diff=", c1 - m, "expected >", 2^13)
  collectgarbage()
  local c2 = collectgarbage("count")
  print("after gc, c2=", c2, "diff=", c2 - m, "expected in [", 2^12, ",", 2^13, ")")
end
