do
  collectgarbage(); collectgarbage()
  local m = collectgarbage("count")
  local a = setmetatable({}, {__mode = "kv"})
  a[string.rep("a", 2^22)] = 25
  a[string.rep("b", 2^22)] = {}
  a[{}] = 14
  print("count1:", collectgarbage("count"), "m+2^13:", m + 2^13)
  collectgarbage()
  print("count2:", collectgarbage("count"))
  local k, v = next(a)
  print("k(short):", tostring(k):sub(1,5), "v:", v)
  print("next(a,k):", next(a, k))
  print("a[rep b]:", a[string.rep("b", 2^22)])
  a[k] = nil
  k = nil
  collectgarbage()
  print("FINAL next(a):", next(a))
  print("count3:", collectgarbage("count"))
end
