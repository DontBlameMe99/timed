--!nonstrict
-- Timed panel: list timers, pause or resume them, stop them, and start new ones.

local duration = require("./lib/duration.luau")
local timersLib = require("./lib/timers.luau")
local urgency = require("./lib/urgency.luau")

local timers = {}
local newText = ""
local inputKey = 0
local errorText = nil
local open = false

local function timerTitle(timer)
  return timer.message or noctalia.tr("panel.timer", { duration = timer.label })
end

local function subtitle(timer, remaining, timeFormat, dateFormat)
  if timer.paused then
    return noctalia.tr("panel.paused_left")
  end
  local format = remaining >= 86400 and dateFormat or timeFormat
  return noctalia.tr("panel.ends_at", {
    time = noctalia.formatTime(format, timer.endsAt // 1000),
  })
end

local function toggle(timer)
  timersLib.send({
    action = timer.paused and "resume" or "pause",
    id = timer.id,
  })
end

local function stop(timer)
  timersLib.send({ action = "stop", id = timer.id })
end

local function timerRow(timer, now, colors, timeFormat, dateFormat)
  local paused = timer.paused
  local state = urgency.state(timer, now)
  local role = urgency.role(timer, now, colors, state)
  local remainingMs = timersLib.remainingMs(timer, now)
  local remaining = math.ceil(remainingMs / 1000)
  local progress = 0
  if timer.seconds > 0 then
    progress = math.clamp(remainingMs / (timer.seconds * 1000), 0, 1)
  end
  local timeColor = nil
  if state == "paused" then
    timeColor = "on_surface_variant"
  elseif colors.useUrgency and (state == "warning" or state == "urgent") then
    timeColor = role
  end
  local toggleGlyph = paused and "player-play" or "player-pause"
  local toggleTip = paused and noctalia.tr("panel.resume") or noctalia.tr("panel.pause")

  return ui.column({
    key = timer.id,
    gap = 8,
    padding = 10,
    fill = "surface_container/0.35",
    radius = 10,
  }, {
    ui.row({ gap = 10, align = "center" }, {
      ui.column({ gap = 2, flexGrow = 1 }, {
        ui.label({ text = timerTitle(timer), fontWeight = "semibold", maxLines = 1 }),
        ui.label({
          text = subtitle(timer, remaining, timeFormat, dateFormat),
          fontSize = 12,
          color = "on_surface_variant",
          maxLines = 1,
        }),
      }),
      ui.label({
        text = duration.approx(remaining),
        fontSize = 18,
        fontWeight = "bold",
        color = timeColor,
      }),
      ui.button({
        glyph = toggleGlyph,
        variant = "ghost",
        controlSize = "sm",
        tooltip = toggleTip,
        onClick = function()
          toggle(timer)
        end,
      }),
      ui.button({
        glyph = "player-stop",
        variant = "ghost",
        controlSize = "sm",
        tooltip = noctalia.tr("panel.stop"),
        onClick = function()
          stop(timer)
        end,
      }),
    }),
    ui.progress({
      height = 3,
      radius = 2,
      progress = progress,
      fill = role,
      track = "outline_variant",
    }),
  })
end

local render

local function start(text)
  local seconds, message = duration.split(text or "")
  if not seconds then
    errorText = noctalia.tr("panel.invalid")
    render()
    return
  end
  timersLib.send({ action = "start", seconds = seconds, message = message })
  newText = ""
  errorText = nil
  inputKey = inputKey + 1
  render()
end

render = function()
  local now = noctalia.nowMs()

  local headerContent = {
    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = noctalia.tr("panel.title"), fontSize = 18, fontWeight = "bold" }),
    }),
  }
  if #timers > 1 then
    table.insert(headerContent, ui.button({
      text = noctalia.tr("panel.stop_all"),
      variant = "ghost",
      controlSize = "sm",
      onClick = "onStopAll",
    }))
  end
  local header = ui.row({ align = "center", justify = "space_between", gap = 8 }, headerContent)

  local body
  if #timers == 0 then
    body = ui.column({ flexGrow = 1, justify = "center", align = "center", gap = 6 }, {
      ui.label({ text = noctalia.tr("panel.empty"), color = "on_surface_variant" }),
    })
  else
    local timeFormat = noctalia.timeFormat()
    local dateFormat = noctalia.dateFormat()
    local colors = urgency.colors()
    local rows = {}
    for _, timer in ipairs(timers) do
      table.insert(rows, timerRow(timer, now, colors, timeFormat, dateFormat))
    end
    body = ui.scroll({ flexGrow = 1, gap = 8 }, rows)
  end

  local startChildren = {
    ui.row({ gap = 8, align = "center" }, {
      ui.input({
        key = "new-" .. inputKey,
        value = newText,
        placeholder = noctalia.tr("panel.placeholder"),
        flexGrow = 1,
        focus = true,
        onChange = "onNewChange",
        onSubmit = "onStartSubmit",
      }),
      ui.button({
        text = noctalia.tr("panel.start"),
        variant = "primary",
        onClick = "onStart",
      }),
    }),
  }
  if errorText then
    table.insert(startChildren, ui.label({ text = errorText, color = "error", fontSize = 12 }))
  end

  panel.render(ui.column({ gap = 12, flexGrow = 1 }, {
    header,
    body,
    ui.column({ gap = 6 }, startChildren),
  }))
end

function onOpen()
  open = true
  timers = timersLib.list()
  newText = ""
  errorText = nil
  inputKey = inputKey + 1
  render()
end

function onClose()
  open = false
end

function update()
  if not open then
    return
  end
  timers = timersLib.list()
  render()
end

function onNewChange(value)
  newText = value
  if errorText then
    errorText = nil
    render()
  end
end

function onStart()
  start(newText)
end

function onStartSubmit(value)
  start(value)
end

function onStopAll()
  timersLib.send({ action = "stop-all" })
end

noctalia.state.watch("timed.list", function()
  if not open then
    return
  end
  timers = timersLib.list()
  render()
end)

panel.setWantsSecondTicks(true)
