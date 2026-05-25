-- Object-orientation via metatables: a class with methods and inheritance.

local Animal = {}
Animal.__index = Animal

function Animal.new(name, sound)
  return setmetatable({ name = name, sound = sound }, Animal)
end

function Animal:speak()
  return string.format("%s says %s", self.name, self.sound)
end

local Dog = setmetatable({}, { __index = Animal })
Dog.__index = Dog

function Dog.new(name)
  local self = Animal.new(name, "woof")
  return setmetatable(self, Dog)
end

function Dog:fetch()
  return self.name .. " fetches the ball"
end

local cat = Animal.new("Cat", "meow")
local rex = Dog.new("Rex")

print(cat:speak())
print(rex:speak())   -- inherited from Animal
print(rex:fetch())   -- defined on Dog
