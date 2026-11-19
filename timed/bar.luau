--!nonstrict
-- Timed bar widget. Shows the nearest timer as a progress ring with the timer
-- count inside, plus the estimated remaining time. Clicking opens the panel.

local duration = require("./lib/duration.luau")
local timersLib = require("./lib/timers.luau")
local urgency = require("./lib/urgency.luau")

local PANEL_ID = "dontblameme/timed:panel"
local RUNNING_INTERVAL_MS = 1000
local IDLE_INTERVAL_MS = 60 * 1000

local RING_SIZE = 26
local RING_VIEWBOX = 40
local RING_RADIUS = 16
local RING_STROKE = 4
local RING_CIRCUMFERENCE = 2 * math.pi * RING_RADIUS

local FONT_FAMILY = noctalia.getSetting("shell.font_family") or "sans-serif"

local timers = {}
local ringSeq = 0
local ringPath = nil
local instanceId = nil

local function setting(key, default)
  local value = noctalia.getConfig(key)
  if value == nil then
    return default
  end
  return value
end

local function config()
  return {
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
end

local function xmlEscape(text)
  return tostring(text)
    :gsub("&", "&amp;")
    :gsub('"', "&quot;")
    :gsub("<", "&lt;")
    :gsub(">", "&gt;")
end

local function instanceKey()
  if not instanceId then
    local output = barWidget.outputName() or "bar"
    instanceId = (output .. "-" .. tostring(noctalia.nowMs())):gsub("[^%w_-]", "_")
  end
  return instanceId
end

local function applyInterval()
  local running = false
  for _, timer in ipairs(timers) do
    if not timer.paused then
      running = true
      break
    end
  end
  noctalia.setUpdateInterval(running and RUNNING_INTERVAL_MS or IDLE_INTERVAL_MS)
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

local function writeRing(timer, now, count, cfg, idle)
  local dataDir = noctalia.pluginDataDir()
  if not dataDir then
    return nil
  end
  local fraction = 1
  if not idle and timer.seconds > 0 then
    fraction = math.clamp(timersLib.remainingMs(timer, now) / (timer.seconds * 1000), 0, 1)
  end
  local role = idle and cfg.idle or urgency.role(timer, now, cfg)
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
  ringSeq = ringSeq + 1
  local path = dataDir .. "/ring-" .. instanceKey() .. "-" .. ringSeq .. ".svg"
  local ok = noctalia.writeFile(path, svg)
  if not ok then
    return nil
  end
  if ringPath then
    noctalia.removeFile(ringPath)
  end
  ringPath = path
  return path
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

local function tooltip()
  if #timers == 0 then
    return noctalia.tr("bar.empty")
  end
  local now = noctalia.nowMs()
  local rows = {}
  for _, timer in ipairs(timers) do
    local left = duration.approx(timersLib.remainingSeconds(timer, now))
    if timer.paused then
      left = noctalia.tr("bar.paused", { time = left })
    end
    table.insert(rows, { key = timerTitle(timer), value = left })
  end
  return rows
end

local function render()
  local cfg = config()
  urgency.publish(cfg)

  if cfg.hideWhenIdle and #timers == 0 then
    barWidget.setVisible(false)
    applyInterval()
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
  local children = {}

  if cfg.showProgress then
    local path = writeRing(nearest, now, count, cfg, idle)
    if path then
      table.insert(children, ui.image({
        path = path,
        width = RING_SIZE,
        height = RING_SIZE,
        fit = "contain",
      }))
    elseif cfg.showCount then
      table.insert(children, countBadge(count, badgeRole))
    end
  elseif cfg.showCount then
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
  barWidget.setTooltip(tooltip())
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
    ringPath = nil
  end
end
