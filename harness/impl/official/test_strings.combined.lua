-- harness preamble: emulate the globals lua-c testes/all.lua sets
_soft = true
_port = true
_nomsg = true
_U = false
arg = arg or {}
_G = _G or _ENV
if _VERSION == nil then _VERSION = "Lua 5.4" end

print('testing')
local maxi <const> = math.maxinteger
local mini <const> = math.mininteger
print('past consts', maxi, mini)
print(string.sub("123456789",2,4))
print('--ok--')
