--!nonstrict
local timers = {
  { id = "t1", seconds = 3600, label = "1h", message = "tea", paused = false, endsAt = __PARITY + 3600000 },
  { id = "t2", seconds = 3900, label = "1h 5m", paused = false, endsAt = __PARITY + 3900000 },
  { id = "t3", seconds = 7200, label = "2h", message = "standup", paused = false, endsAt = __PARITY + 7200000 },
  { id = "t4", seconds = 86400, label = "1d", paused = false, endsAt = __PARITY + 86400000 },
  { id = "t5", seconds = 172800, label = "2d", paused = true, remainingMs = 5000000 },
}

__host.set_state({
  ["timed.list"] = { timers = timers },
  ["timed.colors"] = {
    safe = "tertiary", warning = "secondary", urgent = "error",
    idle = "primary", useUrgency = true,
  },
  ["timed.lastCommand"] = 0,
})
__host.set_config({
  show_text = true, show_count = true, show_progress = false,
  hide_when_idle = false, use_urgency_colors = true,
  safe_color = "tertiary", warning_color = "secondary",
  urgent_color = "error", idle_color = "primary",
})
__host.set_now(__PARITY)
__host.load_entry("bar")

local function prepare(now)
  __host.set_now(now)
end

__host.run(__PARITY, prepare, function() update() end)
