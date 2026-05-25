-- Lua string patterns: matching, capturing, and substitution.

local line = "2026-05-24 ERROR disk full"
local date, level, msg = line:match("(%d+-%d+-%d+)%s+(%u+)%s+(.+)")
print("date  =", date)
print("level =", level)
print("msg   =", msg)

local words = {}
for w in ("the quick brown fox"):gmatch("%a+") do
  words[#words + 1] = w
end
print("word count:", #words)

local shouted = ("hello world"):gsub("%w+", function(w)
  return w:sub(1, 1):upper() .. w:sub(2)
end)
print("title case:", shouted)
