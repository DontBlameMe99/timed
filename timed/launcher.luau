--!nonstrict
-- Timed launcher provider.
--
--   /timer 1h5      start a 1 hour 5 minute timer
--   /timer 1s4h     units may appear in any order
--   /timer 3h2s message   use the message in the notification
--   /timer          show how to start a new timer
--   /timer pause    list running timers to pause
--   /timer resume   list paused timers to resume
--   /timer stop     list timers to stop
--   /timer stop all stop every timer

local duration = require("./lib/duration.luau")
local timersLib = require("./lib/timers.luau")

local lastQuery = nil

local function timerTitle(timer)
  return timer.message or noctalia.tr("launcher.timer", { duration = timer.label })
end

local function countText(count)
  if count == 1 then
    return noctalia.tr("launcher.one_timer")
  end
  return noctalia.tr("launcher.active_count", { count = count })
end

local function emptyRow()
  return {
    id = "empty",
    title = noctalia.tr("launcher.no_matching"),
    subtitle = noctalia.tr("launcher.hint"),
    glyph = "clock",
  }
end

local function startHintRow()
  return {
    id = "new",
    title = noctalia.tr("launcher.start_new"),
    subtitle = noctalia.tr("launcher.hint"),
    glyph = "plus",
    query = " ",
  }
end

local function startRowFor(query)
  local seconds, message = duration.split(query)
  if not seconds then
    return {
      id = "invalid",
      title = noctalia.tr("launcher.invalid"),
      subtitle = noctalia.tr("launcher.hint"),
      glyph = "alert-triangle",
    }
  end
  local subtitle
  if message then
    subtitle = noctalia.tr("launcher.message", {
      duration = duration.human(seconds),
      message = message,
    })
  else
    subtitle = duration.human(seconds)
  end
  return {
    id = "start:" .. seconds .. ":" .. (message or ""),
    title = noctalia.tr("launcher.start", { time = duration.human(seconds) }),
    subtitle = subtitle,
    glyph = "clock",
  }
end

local function timerActionRow(action, timer, now, timeFormat, dateFormat)
  local remaining = timersLib.remainingSeconds(timer, now)
  local subtitle
  if timer.paused then
    subtitle = noctalia.tr("launcher.paused_at", { duration = duration.approx(remaining) })
  else
    local format = remaining >= 86400 and dateFormat or timeFormat
    subtitle = noctalia.tr("launcher.ends_at", {
      time = noctalia.formatTime(format, timer.endsAt // 1000),
      duration = duration.approx(remaining),
    })
  end
  return {
    id = action .. ":" .. timer.id,
    title = noctalia.tr("launcher." .. action) .. " " .. timerTitle(timer),
    subtitle = subtitle,
    glyph = action == "pause" and "player-pause" or action == "resume" and "player-play" or "player-stop",
  }
end

local function actionRows(action, wantPaused)
  local rows = {}
  local now = noctalia.nowMs()
  local timeFormat = noctalia.timeFormat()
  local dateFormat = noctalia.dateFormat()
  for _, timer in ipairs(timersLib.list()) do
    if timer.paused == wantPaused then
      table.insert(rows, timerActionRow(action, timer, now, timeFormat, dateFormat))
    end
  end
  if #rows == 0 then
    return { emptyRow() }
  end
  return rows
end

local function stopRows()
  local rows = {}
  local now = noctalia.nowMs()
  local timeFormat = noctalia.timeFormat()
  local dateFormat = noctalia.dateFormat()
  for _, timer in ipairs(timersLib.list()) do
    table.insert(rows, timerActionRow("stop", timer, now, timeFormat, dateFormat))
  end
  if #rows == 0 then
    return { emptyRow() }
  end
  return rows
end

local function render(text)
  lastQuery = text
  local query = noctalia.string.trim(text)
  local rows

  if query == "" then
    rows = { startHintRow() }
  elseif query == "pause" then
    rows = actionRows("pause", false)
  elseif query == "resume" then
    rows = actionRows("resume", true)
  elseif query == "stop" then
    rows = stopRows()
  elseif query == "stop all" then
    local total = #timersLib.list()
    if total == 0 then
      rows = { emptyRow() }
    else
      rows = { {
        id = "stop-all",
        title = noctalia.tr("launcher.stop_all"),
        subtitle = countText(total),
        glyph = "player-stop",
      } }
    end
  else
    rows = { startRowFor(query) }
  end

  launcher.setResults(text, rows)
end

local function send(command)
  timersLib.send(command)
  lastQuery = nil
end

function onQuery(text)
  render(text)
end

function onActivate(id)
  if id == "new" or id == "empty" then
    return
  end

  if id == "stop-all" then
    send({ action = "stop-all" })
    return
  end

  local action, timerId = id:match("^(%a+):(.+)$")
  if action == "pause" or action == "resume" or action == "stop" then
    send({ action = action, id = timerId })
    return
  end

  local seconds, message = id:match("^start:(%d+):(.*)$")
  if seconds then
    send({
      action = "start",
      seconds = tonumber(seconds),
      message = message ~= "" and message or nil,
    })
  end
end

noctalia.state.watch("timed.list", function()
  if lastQuery ~= nil then
    render(lastQuery)
  end
end)
