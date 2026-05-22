-- Sieve of Eratosthenes, lua-rs.
local LIMIT = 200

local sieve = {}
for i = 2, LIMIT do sieve[i] = true end
for i = 2, math.floor(math.sqrt(LIMIT)) do
  if sieve[i] then
    for j = i*i, LIMIT, i do sieve[j] = false end
  end
end

local primes = {}
for i = 2, LIMIT do
  if sieve[i] then primes[#primes + 1] = i end
end

print(string.format("Found %d primes up to %d:", #primes, LIMIT))
local buf = {}
for i, p in ipairs(primes) do
  buf[#buf + 1] = string.format("%4d", p)
  if i % 10 == 0 then buf[#buf + 1] = "\n" end
end
print(table.concat(buf))
