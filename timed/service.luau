--!nonstrict
-- Timed service: owns the timer list, wakes at the next expiry, and notifies
-- when a timer finishes. The bar widget, panel, and launcher send commands
-- through noctalia.state.
--
-- Timers can be paused. A running timer stores its end time, a paused timer
-- stores its remaining time, so a pause survives a shell restart.
--
-- The update interval follows the soonest running timer instead of ticking
-- every second, and state is published only when the list changes. A year-long
-- timer re-checks once a day instead of every second.
--
-- The list is also saved to the plugin data directory, so timers survive a
-- shell restart and a reboot. A timer that expired while the shell was gone
-- notifies on the next start.

local duration = require("./lib/duration.luau")

local MAX_INTERVAL_MS = 24 * 60 * 60 * 1000

local dataDir = noctalia.pluginDataDir()
local statePath = dataDir and (dataDir .. "/timers.json")

local timers = {}
local lastCommand = tonumber(noctalia.state.get("timed.lastCommand")) or 0
local ipcSeq = 0
local savedContents = nil
local currentInterval = nil

local function setInterval(ms)
  if ms == currentInterval then
    return
  end
  currentInterval = ms
  noctalia.setUpdateInterval(ms)
end

local function normalize(saved)
  if type(saved) ~= "table" then
    return nil
  end
  if type(saved.id) ~= "string"
    or type(saved.seconds) ~= "number"
    or type(saved.label) ~= "string"
    or (saved.message ~= nil and type(saved.message) ~= "string") then
    return nil
  end

  local timer = {
    id = saved.id,
    seconds = saved.seconds,
    label = saved.label,
    message = saved.message,
    paused = saved.paused == true,
  }
  if timer.paused then
    if type(saved.remainingMs) ~= "number" then
      return nil
    end
    timer.remainingMs = math.max(0, saved.remainingMs)
  else
    if type(saved.endsAt) ~= "number" then
      return nil
    end
    timer.endsAt = saved.endsAt
  end
  return timer
end

local function loadTimers()
  if not statePath then
    return
  end
  local contents = noctalia.readFile(statePath)
  if type(contents) ~= "string" then
    return
  end
  local decoded = noctalia.json.decode(contents)
  if type(decoded) ~= "table" or type(decoded.timers) ~= "table" then
    noctalia.log("timed: ignoring unreadable saved timers")
    return
  end
  savedContents = contents
  for _, saved in ipairs(decoded.timers) do
    local timer = normalize(saved)
    if timer then
      table.insert(timers, timer)
    end
  end
end

local function sortTimers()
  table.sort(timers, function(a, b)
    if a.paused ~= b.paused then
      return not a.paused
    end
    if a.paused then
      return a.remainingMs < b.remainingMs
    end
    return a.endsAt < b.endsAt
  end)
end

local function publish()
  local copy = {}
  for i, timer in ipairs(timers) do
    local item = {
      id = timer.id,
      seconds = timer.seconds,
      label = timer.label,
      message = timer.message,
      paused = timer.paused,
    }
    if timer.paused then
      item.remainingMs = timer.remainingMs
    else
      item.endsAt = timer.endsAt
    end
    copy[i] = item
  end
  noctalia.state.set("timed.list", { timers = copy })
  if statePath then
    local encoded = noctalia.json.encode({ timers = copy })
    if encoded and encoded ~= savedContents then
      -- A crash mid-write must not truncate the list the next start reads.
      local tmpPath = statePath .. ".tmp"
      local ok, err = noctalia.writeFile(tmpPath, encoded)
      if ok then
        ok, err = noctalia.renameFile(tmpPath, statePath)
      end
      if not ok then
        noctalia.log("timed: cannot save timers: " .. tostring(err))
      else
        savedContents = encoded
      end
    end
  end
end

local function notifyFinished(timer)
  local title = noctalia.tr("notify.title")
  local body = timer.message or noctalia.tr("notify.body", { duration = timer.label })
  noctalia.notify(title, body)
end

local function schedule(now)
  local nextEnd = nil
  for _, timer in ipairs(timers) do
    if not timer.paused then
      nextEnd = timer.endsAt
      break
    end
  end
  if nextEnd == nil then
    setInterval(MAX_INTERVAL_MS)
    return
  end
  local delay = nextEnd - (now or noctalia.nowMs()) + 50
  if delay < 100 then
    delay = 100
  elseif delay > MAX_INTERVAL_MS then
    delay = MAX_INTERVAL_MS
  end
  setInterval(delay)
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

local function pauseById(id)
  local now = noctalia.nowMs()
  for _, timer in ipairs(timers) do
    if timer.id == id and not timer.paused then
      local remaining = timer.endsAt - now
      if remaining <= 0 then
        return false
      end
      timer.paused = true
      timer.remainingMs = remaining
      timer.endsAt = nil
      return true
    end
  end
  return false
end

local function resumeById(id)
  local now = noctalia.nowMs()
  for _, timer in ipairs(timers) do
    if timer.id == id and timer.paused then
      timer.paused = false
      timer.endsAt = now + timer.remainingMs
      timer.remainingMs = nil
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
  local label = duration.human(seconds)
  table.insert(timers, {
    id = tostring(id),
    endsAt = noctalia.nowMs() + seconds * 1000,
    seconds = seconds,
    label = label,
    message = message,
    paused = false,
  })
  noctalia.log("timed: started " .. tostring(id) .. " for " .. label)
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
  elseif command.action == "pause" then
    changed = pauseById(tostring(command.id))
    if changed then
      noctalia.log("timed: paused " .. tostring(command.id))
    end
  elseif command.action == "resume" then
    changed = resumeById(tostring(command.id))
    if changed then
      noctalia.log("timed: resumed " .. tostring(command.id))
    end
  elseif command.action == "stop-all" then
    changed = #timers > 0
    timers = {}
    if changed then
      noctalia.log("timed: stopped all")
    end
  end

  if changed then
    if command.action ~= "stop" and command.action ~= "stop-all" then
      sortTimers()
    end
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

local function expire(now)
  now = now or noctalia.nowMs()
  local kept = {}
  local changed = false
  for i, timer in ipairs(timers) do
    if timer.paused then
      table.insert(kept, timer)
    elseif timer.endsAt > now then
      for j = i, #timers do
        table.insert(kept, timers[j])
      end
      break
    else
      notifyFinished(timer)
      noctalia.log("timed: finished " .. timer.id)
      changed = true
    end
  end
  if changed then
    timers = kept
  end
  return changed
end

loadTimers()
sortTimers()
noctalia.state.watch("timed.command", handleCommand)
handleCommand(noctalia.state.get("timed.command"))
expire()
publish()
schedule()

function update()
  local now = noctalia.nowMs()
  if expire(now) then
    publish()
  end
  schedule(now)
end

function onResume()
  local now = noctalia.nowMs()
  if expire(now) then
    publish()
  end
  schedule(now)
end

function onIpc(event, payload)
  if event == "start" then
    local seconds, message = duration.split(payload)
    if seconds then
      ipcSeq = ipcSeq + 1
      local id = tostring(noctalia.nowMs()) .. "-" .. ipcSeq
      if startTimer(seconds, id, message) then
        sortTimers()
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
  elseif event == "stop" then
    if stopById(tostring(payload)) then
      publish()
      schedule()
      noctalia.log("timed: stopped " .. tostring(payload))
    end
  elseif event == "pause" then
    if pauseById(tostring(payload)) then
      sortTimers()
      publish()
      schedule()
      noctalia.log("timed: paused " .. tostring(payload))
    end
  elseif event == "resume" then
    if resumeById(tostring(payload)) then
      sortTimers()
      publish()
      schedule()
      noctalia.log("timed: resumed " .. tostring(payload))
    end
  elseif event == "list" then
    noctalia.log("timed: " .. #timers .. " timer(s)")
    local now = noctalia.nowMs()
    for _, timer in ipairs(timers) do
      local remaining = math.max(0, (timer.paused and timer.remainingMs or timer.endsAt - now)) // 1000
      local state = timer.paused and "paused" or "running"
      noctalia.log("timed: " .. timer.id .. " " .. state .. " " .. duration.clock(remaining) .. " left")
    end
  end
end
