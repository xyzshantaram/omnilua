collectgarbage()
local a = setmetatable({}, {__mode = "kv"})
a[string.rep("a", 100)] = 25
a[string.rep("b", 100)] = {}
a[{}] = 14
print("before gc:")
for k,v in pairs(a) do print(" ", type(k), tostring(k):sub(1,10), "->", type(v), v) end
collectgarbage()
print("after first gc:")
for k,v in pairs(a) do print(" ", type(k), tostring(k):sub(1,10), "->", type(v), v) end
local k, v = next(a)
print("first next:", type(k), tostring(k):sub(1,10), v)
a[k] = nil
k = nil
collectgarbage()
print("after deletion gc:")
for k,v in pairs(a) do print(" ", type(k), tostring(k):sub(1,10), "->", type(v), v) end
print("next(a):", next(a))
