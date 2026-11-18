--!nonstrict
-- Timed service: owns the timer list, wakes at the next expiry, and notifies
-- when a timer finishes. The launcher sends commands through noctalia.state.
--
-- The update interval follows the soonest timer instead of ticking every
-- second, and state is published only when the list changes. A year-long
-- timer re-checks once a day instead of every second.

local duration = require("./lib/duration.luau")

local MAX_INTERVAL_MS = 24 * 60 * 60 * 1000

local pluginDir = noctalia.pluginDir()
local icon = pluginDir and (pluginDir .. "/tie.png")
local NOTIFY_ICON = (icon and noctalia.fileExists(icon)) and icon or "clock"

local timers = {}
local lastCommand = tonumber(noctalia.state.get("timed.lastCommand")) or 0
local ipcSeq = 0

local saved = noctalia.state.get("timed.list")
if type(saved) == "table" and type(saved.timers) == "table" then
  for _, timer in ipairs(saved.timers) do
    if
      type(timer) == "table"
      and type(timer.id) == "string"
      and type(timer.endsAt) == "number"
      and type(timer.seconds) == "number"
      and type(timer.label) == "string"
      and (timer.message == nil or type(timer.message) == "string")
    then
      table.insert(timers, timer)
    end
  end
end

local function publish()
  local copy = {}
  for i, timer in ipairs(timers) do
    copy[i] = {
      id = timer.id,
      endsAt = timer.endsAt,
      seconds = timer.seconds,
      label = timer.label,
      message = timer.message,
    }
  end
  noctalia.state.set("timed.list", { timers = copy })
end

-- A timer that finishes has to stay on screen until dismissed, so it goes
-- through notify-send with expire-time 0. noctalia.notify has no timeout knob
-- and is only the fallback when notify-send cannot start.
local function notifyFinished(timer)
  local title = noctalia.tr("notify.title")
  local body = timer.message or noctalia.tr("notify.body", { duration = timer.label })
  local launched = noctalia.runAsync({
    "notify-send",
    "-a",
    "Timed",
    "-t",
    "0",
    "-i",
    NOTIFY_ICON,
    "--",
    title,
    body,
  })
  if not launched then
    noctalia.notify(title, body)
  end
end

local function schedule()
  if #timers == 0 then
    noctalia.setUpdateInterval(MAX_INTERVAL_MS)
    return
  end
  local delay = timers[1].endsAt - noctalia.nowMs() + 50
  if delay < 100 then
    delay = 100
  elseif delay > MAX_INTERVAL_MS then
    delay = MAX_INTERVAL_MS
  end
  noctalia.setUpdateInterval(delay)
end

local function stopById(id)
  for i = #timers, 1, -1 do
    if timers[i].id == id then
      table.remove(timers, i)
      return true
    end
  end
  return false
end

local function startTimer(seconds, id, message)
  seconds = math.floor(seconds)
  if seconds < 1 then
    return false
  end
  local endsAt = noctalia.nowMs() + seconds * 1000
  table.insert(timers, {
    id = tostring(id),
    endsAt = endsAt,
    seconds = seconds,
    label = duration.human(seconds),
    message = message,
  })
  table.sort(timers, function(a, b)
    return a.endsAt < b.endsAt
  end)
  local note = message and (" (" .. message .. ")") or ""
  noctalia.log("timed: started " .. tostring(id) .. " for " .. duration.human(seconds) .. note)
  return true
end

local function apply(command)
  if type(command) ~= "table" then
    return
  end

  local changed = false
  if command.action == "start" then
    local seconds = tonumber(command.seconds)
    local id = command.id or command.n
    if seconds and seconds > 0 and seconds < math.huge and id ~= nil then
      changed = startTimer(seconds, id, command.message)
    end
  elseif command.action == "stop" then
    changed = stopById(tostring(command.id))
    if changed then
      noctalia.log("timed: stopped " .. tostring(command.id))
    end
  elseif command.action == "stop-all" then
    changed = #timers > 0
    timers = {}
    if changed then
      noctalia.log("timed: stopped all")
    end
  end

  if changed then
    publish()
    schedule()
  end
end

local function handleCommand(command)
  if type(command) ~= "table" then
    return
  end
  local n = tonumber(command.n)
  if not n or n <= lastCommand then
    return
  end
  lastCommand = n
  noctalia.state.set("timed.lastCommand", n)
  apply(command)
end

local function expire()
  local now = noctalia.nowMs()
  local kept = {}
  local changed = false
  for _, timer in ipairs(timers) do
    if timer.endsAt <= now then
      notifyFinished(timer)
      noctalia.log("timed: finished " .. timer.id)
      changed = true
    else
      table.insert(kept, timer)
    end
  end
  if changed then
    timers = kept
  end
  return changed
end

noctalia.state.watch("timed.command", handleCommand)
handleCommand(noctalia.state.get("timed.command"))
publish()
schedule()

function update()
  if expire() then
    publish()
  end
  schedule()
end

function onResume()
  if expire() then
    publish()
  end
  schedule()
end

function onIpc(event, payload)
  if event == "start" then
    local seconds, message = duration.split(payload)
    if seconds then
      ipcSeq = ipcSeq + 1
      local id = tostring(noctalia.nowMs()) .. "-" .. ipcSeq
      if startTimer(seconds, id, message) then
        publish()
        schedule()
      end
    end
  elseif event == "stop-all" then
    if #timers > 0 then
      timers = {}
      publish()
      schedule()
      noctalia.log("timed: stopped all")
    end
  elseif event == "list" then
    noctalia.log("timed: " .. #timers .. " timer(s)")
    for _, timer in ipairs(timers) do
      local remaining = math.max(0, (timer.endsAt - noctalia.nowMs()) // 1000)
      local note = timer.message and (" (" .. timer.message .. ")") or ""
      noctalia.log("timed: " .. timer.id .. " " .. duration.clock(remaining) .. " left" .. note)
    end
  end
end
