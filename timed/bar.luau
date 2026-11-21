--!nonstrict
-- Timed bar widget. Shows the nearest timer as a progress ring with the timer
-- count inside, plus the estimated remaining time. Clicking opens the panel.

local duration = require("./lib/duration.luau")
local timersLib = require("./lib/timers.luau")
local urgency = require("./lib/urgency.luau")

local PANEL_ID = "dontblameme/timed:panel"
local RUNNING_INTERVAL_MS = 1000
local IDLE_INTERVAL_MS = 60 * 1000
local HIDDEN_INTERVAL_MS = 24 * 60 * 60 * 1000

local RING_SIZE = 26
local RING_VIEWBOX = 40
local RING_RADIUS = 16
local RING_STROKE = 4
local RING_CIRCUMFERENCE = 2 * math.pi * RING_RADIUS

local FONT_FAMILY = noctalia.getSetting("shell.font_family") or "sans-serif"
local dataDir = noctalia.pluginDataDir()
local instanceId = nil

local function instanceKey()
  if not instanceId then
    local output = barWidget.outputName() or "bar"
    instanceId = (output .. "-" .. tostring(noctalia.nowMs())):gsub("[^%w_-]", "_")
  end
  return instanceId
end

local ringPath = dataDir and (dataDir .. "/ring-" .. instanceKey() .. ".svg")
local tmpPath = ringPath and (ringPath .. ".tmp")

local cachedConfig = nil
local timers = {}
local ringSvg = nil
local imageSet = false
local usingFallback = false
local lastTooltip = nil
local lastText = nil
local lastTextColor = nil
local currentInterval = nil

local function setting(key, default)
  local value = noctalia.getConfig(key)
  if value == nil then
    return default
  end
  return value
end

local function config()
  if cachedConfig then
    return cachedConfig
  end
  cachedConfig = {
    showText = setting("show_text", true),
    showCount = setting("show_count", true),
    showProgress = setting("show_progress", true),
    hideWhenIdle = setting("hide_when_idle", false),
    safe = setting("safe_color", "tertiary"),
    warning = setting("warning_color", "secondary"),
    urgent = setting("urgent_color", "error"),
    idle = setting("idle_color", "primary"),
    useUrgency = setting("use_urgency_colors", true),
  }
  return cachedConfig
end

local function xmlEscape(text)
  return tostring(text)
    :gsub("&", "&amp;")
    :gsub('"', "&quot;")
    :gsub("<", "&lt;")
    :gsub(">", "&gt;")
end

local function setInterval(ms)
  if ms == currentInterval then
    return
  end
  currentInterval = ms
  noctalia.setUpdateInterval(ms)
end

local function applyInterval()
  local running = false
  for _, timer in ipairs(timers) do
    if not timer.paused then
      running = true
      break
    end
  end
  setInterval(running and RUNNING_INTERVAL_MS or IDLE_INTERVAL_MS)
end

local function timerTitle(timer)
  if timer.message then
    return timer.message
  end
  return noctalia.tr("bar.timer", { duration = timer.label })
end

local function color(role, fallback)
  return noctalia.getColor(role) or fallback
end

local function setLabel(text, role)
  if text ~= lastText then
    barWidget.setText(text)
    lastText = text
  end
  if role ~= lastTextColor then
    barWidget.setColor(role)
    lastTextColor = role
  end
end

local function countBadge(count, role)
  return ui.row({
    fill = role .. "/0.18",
    radius = 6,
    paddingH = 5,
    align = "center",
  }, {
    ui.label({ text = tostring(count), color = role, fontWeight = "bold", fontSize = 12 }),
  })
end

local function renderFallback(nearest, count, cfg, badgeRole, vertical, now, state)
  local children = {}
  if cfg.showCount then
    table.insert(children, countBadge(count, badgeRole))
  end
  if nearest and cfg.showText and not vertical then
    local timeLabel = { text = duration.approx(timersLib.remainingSeconds(nearest, now)) }
    if state == "paused" then
      timeLabel.color = "on_surface_variant"
    elseif cfg.useUrgency and (state == "warning" or state == "urgent") then
      timeLabel.color = badgeRole
    end
    table.insert(children, ui.label(timeLabel))
  end
  local container = vertical and ui.column or ui.row
  barWidget.render(container({ gap = vertical and 3 or 6, align = "center" }, children))
end

local function writeRing(timer, now, count, cfg, idle, state)
  if not ringPath then
    return false
  end
  local fraction = 1
  if not idle and timer.seconds > 0 then
    fraction = math.clamp(timersLib.remainingMs(timer, now) / (timer.seconds * 1000), 0, 1)
  end
  local role = idle and cfg.idle or urgency.role(timer, now, cfg, state)
  local arcColor = color(role, "#CBA6F7")
  local trackColor = color("outline_variant", "#4C4F69")
  local offset = RING_CIRCUMFERENCE * (1 - fraction)
  local countText = ""
  if cfg.showCount then
    countText = string.format(
      '<text x="20" y="26" text-anchor="middle" font-family="%s" font-size="19" font-weight="bold" fill="%s">%d</text>',
      xmlEscape(FONT_FAMILY), arcColor, count)
  end
  local svg = string.format(
    '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" viewBox="0 0 %d %d">' ..
    '<circle cx="20" cy="20" r="%d" fill="none" stroke="%s" stroke-width="%d"/>' ..
    '<circle cx="20" cy="20" r="%d" fill="none" stroke="%s" stroke-width="%d" stroke-dasharray="%.2f" stroke-dashoffset="%.2f" stroke-linecap="round" transform="rotate(-90 20 20)"/>' ..
    '%s</svg>',
    RING_VIEWBOX, RING_VIEWBOX, RING_VIEWBOX, RING_VIEWBOX,
    RING_RADIUS, trackColor, RING_STROKE,
    RING_RADIUS, arcColor, RING_STROKE, RING_CIRCUMFERENCE, offset,
    countText)
  if svg ~= ringSvg then
    local ok = noctalia.writeFile(tmpPath, svg)
    if ok then
      ok = noctalia.renameFile(tmpPath, ringPath)
    end
    if not ok then
      return false
    end
    ringSvg = svg
  end
  if not imageSet then
    barWidget.setImage(ringPath, true, RING_SIZE, RING_SIZE)
    imageSet = true
  end
  return true
end

local function setTooltip(value, signature)
  if signature == lastTooltip then
    return
  end
  lastTooltip = signature
  barWidget.setTooltip(value)
end

local function tooltip(now)
  if #timers == 0 then
    local value = noctalia.tr("bar.empty")
    setTooltip(value, value)
    return
  end
  local rows = {}
  local parts = {}
  for _, timer in ipairs(timers) do
    local left = duration.approx(timersLib.remainingSeconds(timer, now))
    if timer.paused then
      left = noctalia.tr("bar.paused", { time = left })
    end
    table.insert(rows, { key = timerTitle(timer), value = left })
    table.insert(parts, timer.id .. "=" .. left)
  end
  setTooltip(rows, table.concat(parts, "|"))
end

local function render()
  local cfg = config()
  urgency.publish(cfg)

  if cfg.hideWhenIdle and #timers == 0 then
    barWidget.setVisible(false)
    setInterval(HIDDEN_INTERVAL_MS)
    return
  end
  barWidget.setVisible(true)

  local vertical = barWidget.isVertical()
  local nearest = timers[1]
  local idle = nearest == nil
  local count = idle and 0 or #timers
  local now = noctalia.nowMs()
  local state = nearest and urgency.state(nearest, now) or "idle"
  local badgeRole
  if idle then
    badgeRole = cfg.idle
  elseif state == "paused" then
    badgeRole = "on_surface_variant"
  elseif cfg.useUrgency then
    badgeRole = urgency.roleForState(state, cfg)
  else
    badgeRole = cfg.safe
  end

  if usingFallback then
    renderFallback(nearest, count, cfg, badgeRole, vertical, now, state)
  else
    local showingRing = cfg.showProgress and writeRing(nearest, now, count, cfg, idle, state)
    if not showingRing then
      usingFallback = true
      renderFallback(nearest, count, cfg, badgeRole, vertical, now, state)
    else
      local text = ""
      local textColor = "on_surface"
      if nearest and cfg.showText and not vertical then
        text = duration.approx(timersLib.remainingSeconds(nearest, now))
        if state == "paused" then
          textColor = "on_surface_variant"
        elseif cfg.useUrgency and (state == "warning" or state == "urgent") then
          textColor = badgeRole
        end
      end
      setLabel(text, textColor)
    end
  end

  tooltip(now)
  applyInterval()
end

function update()
  timers = timersLib.list()
  render()
end

noctalia.state.watch("timed.list", function()
  timers = timersLib.list()
  render()
end)

function onClick()
  noctalia.togglePanel(PANEL_ID)
end

function onExit()
  if ringPath then
    noctalia.removeFile(ringPath)
    noctalia.removeFile(tmpPath)
  end
end
