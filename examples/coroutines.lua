-- Coroutines as lazy generators and as a cooperative producer/consumer pair.

local function range(first, last, step)
  step = step or 1
  return coroutine.wrap(function()
    for i = first, last, step do
      coroutine.yield(i)
    end
  end)
end

local squares = {}
for n in range(1, 10) do
  squares[#squares + 1] = n * n
end
print("squares 1..10:", table.concat(squares, " "))

local producer = coroutine.create(function()
  for _, word in ipairs({ "safe", "rust", "lua" }) do
    coroutine.yield(word:upper())
  end
end)

while true do
  local ok, value = coroutine.resume(producer)
  if not ok or value == nil then break end
  print("consumed:", value)
end
