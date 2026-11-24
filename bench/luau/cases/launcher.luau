--!nonstrict
local timers = {
  { id = "l1", seconds = 600, label = "10m", message = "focus", paused = false, endsAt = __PARITY + 600000 },
  { id = "l2", seconds = 1800, label = "30m", paused = false, endsAt = __PARITY + 1800000 },
  { id = "l3", seconds = 5400, label = "1h 30m", message = "standup", paused = false, endsAt = __PARITY + 5400000 },
  { id = "l4", seconds = 86400, label = "1d", paused = true, remainingMs = 900000 },
  { id = "l5", seconds = 172800, label = "2d", paused = false, endsAt = __PARITY + 172800000 },
}

__host.set_state({
  ["timed.list"] = { timers = timers },
  ["timed.lastCommand"] = 0,
})
__host.set_now(__PARITY)
__host.load_entry("launcher")

local function prepare(now)
  __host.set_now(now)
end

__host.run(__PARITY, prepare, function() onQuery("1h5 wake up") end)
