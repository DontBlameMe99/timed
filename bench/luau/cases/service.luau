--!nonstrict
local timers = {
  { id = "s1", seconds = 3600, label = "1h", message = "tea", paused = false, endsAt = __PARITY + 3600000 },
  { id = "s2", seconds = 7200, label = "2h", paused = false, endsAt = __PARITY + 7200000 },
  { id = "s3", seconds = 14400, label = "4h", message = "bake", paused = false, endsAt = __PARITY + 14400000 },
  { id = "s4", seconds = 28800, label = "8h", paused = false, endsAt = __PARITY + 28800000 },
  { id = "s5", seconds = 86400, label = "1d", paused = false, endsAt = __PARITY + 86400000 },
  { id = "s6", seconds = 172800, label = "2d", message = "soak", paused = false, endsAt = __PARITY + 172800000 },
  { id = "s7", seconds = 604800, label = "7d", paused = false, endsAt = __PARITY + 604800000 },
  { id = "s8", seconds = 1209600, label = "2w", paused = false, endsAt = __PARITY + 1209600000 },
  { id = "s9", seconds = 2592000, label = "30d", paused = true, remainingMs = 1800000 },
  { id = "s10", seconds = 8640000, label = "100d", paused = false, endsAt = __PARITY + 8640000000 },
}

__host.set_now(__PARITY)
__host.seed_file("/data/timers.json", { timers = timers })
local load_ops = __host.load_entry("service")
print("OPS " .. __host.encode(load_ops))

local function prepare(now)
  __host.set_now(now)
end

__host.time(prepare, function() update() end)
