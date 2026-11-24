--!nonstrict
local urgency = require("./lib/urgency.luau")

__host.set_state({
  ["timed.colors"] = {
    safe = "tertiary", warning = "secondary", urgent = "error",
    idle = "primary", useUrgency = true,
  },
})

local function run()
  local colors = urgency.colors()
  local parts = {}
  for index = 1, 200 do
    local timer = {
      id = "u" .. index,
      seconds = 100 + index,
      label = "x",
      paused = index % 7 == 0,
      endsAt = __PARITY + index * 1000,
    }
    parts[index] = urgency.state(timer, __PARITY) .. ":" .. urgency.role(timer, __PARITY, colors, nil)
  end
  noctalia.log(table.concat(parts, " "))
end

__host.run(__PARITY, function() end, run)
