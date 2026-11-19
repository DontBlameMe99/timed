--!nonstrict
-- Shared state helpers for the Timed bar widget, panel, and launcher.

local M = {}

local seq = tonumber(noctalia.state.get("timed.lastCommand")) or 0

function M.list()
  local state = noctalia.state.get("timed.list")
  if type(state) == "table" and type(state.timers) == "table" then
    return state.timers
  end
  return {}
end

function M.send(command)
  seq = math.max(noctalia.nowMs(), seq + 1)
  command.n = seq
  noctalia.state.set("timed.command", command)
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
