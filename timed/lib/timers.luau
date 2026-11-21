--!nonstrict
-- Shared state helpers for the Timed bar widget, panel, and launcher.

local M = {}
local empty = {}
local cached = nil

noctalia.state.watch("timed.list", function(value)
  if type(value) == "table" and type(value.timers) == "table" then
    cached = value.timers
  else
    cached = empty
  end
end)

function M.list()
  if cached then
    return cached
  end
  local state = noctalia.state.get("timed.list")
  if type(state) == "table" and type(state.timers) == "table" then
    cached = state.timers
    return cached
  end
  cached = empty
  return cached
end

local seq = tonumber(noctalia.state.get("timed.lastCommand")) or 0

function M.send(command)
  seq = math.max(noctalia.nowMs(), seq + 1)
  command.n = seq
  noctalia.state.set("timed.command", command)
end
function M.toggle(timer)
  if not timer then
    return
  end
  M.send({
    action = timer.paused and "resume" or "pause",
    id = timer.id,
  })
end

function M.remainingMs(timer, now)
  if timer.paused then
    return math.max(0, timer.remainingMs or 0)
  end
  return math.max(0, (timer.endsAt or now) - now)
end

function M.remainingSeconds(timer, now)
  return math.ceil(M.remainingMs(timer, now) / 1000)
end

return M
