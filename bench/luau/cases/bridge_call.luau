--!nonstrict
-- Shell-side cost of submitting one event through the real generated bridge.
-- Files are held in memory here, so this excludes the host's own write time.

local timers = {
  { id = "t1", seconds = 3600, label = "1h", message = "tea", paused = false, endsAt = __PARITY + 3600000 },
  { id = "t2", seconds = 3900, label = "1h 5m", paused = false, endsAt = __PARITY + 3900000 },
  { id = "t3", seconds = 7200, label = "2h", message = "standup", paused = false, endsAt = __PARITY + 7200000 },
  { id = "t4", seconds = 86400, label = "1d", paused = false, endsAt = __PARITY + 86400000 },
  { id = "t5", seconds = 172800, label = "2d", paused = true, remainingMs = 5000000 },
}

__host.set_state({
  ["timed.state"] = { timers = timers },
  ["timed.lastCommand"] = 0,
})
__host.set_config({
  show_text = true, show_count = true, show_progress = true,
  hide_when_idle = false, use_urgency_colors = true,
  safe_color = "tertiary", warning_color = "secondary",
  urgent_color = "error", idle_color = "primary",
})
__host.set_now(__PARITY)

local bridge = assert(loadstring(__BRIDGE, "bridge"))()
bridge.setup("timed_indicator", {
  "show_text", "show_count", "show_progress", "hide_when_idle",
  "use_urgency_colors", "safe_color", "warning_color", "urgent_color",
  "idle_color",
})
__host.bridge_ready()
__host.reset()

local count = 0
local function prepare(now)
  count = count + 1
  __host.set_now(now)
end

print("OPS []")
__host.time(prepare, function()
  bridge.call("render", { source = "state" })
end)
