-- ASCII Mandelbrot set, rendered in lua-rs.
local W, H, MAX_ITER = 78, 28, 40
local palette = " .:-=+*#%@"

local function intensity(cx, cy)
  local x, y, iter = 0.0, 0.0, 0
  while x*x + y*y <= 4.0 and iter < MAX_ITER do
    x, y = x*x - y*y + cx, 2*x*y + cy
    iter = iter + 1
  end
  return iter
end

local lines = {}
for py = 0, H - 1 do
  local row = {}
  local cy = (py / (H - 1)) * 2.0 - 1.0
  for px = 0, W - 1 do
    local cx = (px / (W - 1)) * 3.0 - 2.1
    local n = intensity(cx, cy)
    if n == MAX_ITER then
      row[#row + 1] = " "
    else
      local idx = math.floor((n / MAX_ITER) * (#palette - 1)) + 1
      row[#row + 1] = string.sub(palette, idx, idx)
    end
  end
  lines[#lines + 1] = table.concat(row)
end
print(table.concat(lines, "\n"))
print(string.format("(%dx%d, max %d iterations)", W, H, MAX_ITER))
