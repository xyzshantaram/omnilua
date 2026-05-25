-- Recursion, memoization with a table, and Lua 5.4 integer/float division.

local function fib(n)
  if n < 2 then return n end
  return fib(n - 1) + fib(n - 2)
end

local memo = { [0] = 0, [1] = 1 }
local function fib_memo(n)
  if memo[n] then return memo[n] end
  memo[n] = fib_memo(n - 1) + fib_memo(n - 2)
  return memo[n]
end

for i = 0, 15 do
  io.write(fib(i), " ")
end
print()

print("fib(50) via memoization =", fib_memo(50))
print("golden ratio approx     =", fib_memo(50) // fib_memo(49), fib_memo(50) / fib_memo(49))
