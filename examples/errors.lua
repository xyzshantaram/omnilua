-- Protected calls, error objects, and to-be-closed variables (Lua 5.4).

local function risky(n)
  if n < 0 then
    error({ code = "negative", value = n })
  end
  return math.sqrt(n)
end

local ok, result = pcall(risky, 16)
print("pcall(16) ->", ok, result)

local ok2, err = pcall(risky, -1)
print("pcall(-1) ->", ok2, type(err) == "table" and err.code or err)

-- A to-be-closed variable runs its __close handler on scope exit.
local function resource(label)
  return setmetatable({}, { __close = function() print("closed:", label) end })
end

do
  local _ <close> = resource("file-handle")
  print("inside scope")
end
print("after scope")
