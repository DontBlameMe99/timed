--!nonstrict
local duration = require("./lib/duration.luau")

local INPUTS = {
  "5", "1h5", "1m5", "3h20m10s", "1s4h", "4s10h3m", "1h 5m", "1y", "1y2y",
  "100y", "101y", "1s5", "0", "", "abc", "1x", "1.5h", "3h2s something I want",
  "30 walk the dog", "1h5 30",
}

local function num(value)
  if value == nil then
    return "-"
  end
  return tostring(value)
end

local function line(input)
  local parsed = duration.parse(input)
  local seconds, message = duration.split(input)
  local human = parsed and duration.human(parsed) or "-"
  local approx = parsed and duration.approx(parsed) or "-"
  local clock = parsed and duration.clock(parsed) or "-"
  return table.concat({
    input, num(parsed), num(seconds), message or "-", human, approx, clock,
  }, " | ")
end

local function run()
  local parts = {}
  for index, input in ipairs(INPUTS) do
    parts[index] = line(input)
  end
  noctalia.log(table.concat(parts, "\n"))
end

__host.run(__PARITY, function() end, run)
