--!nonstrict
-- Progress-based timer colors shared by the bar widget and the panel.

local timersLib = require("./timers.luau")

local M = {}

local DEFAULTS = {
  safe = "tertiary",
  warning = "secondary",
  urgent = "error",
  idle = "primary",
  useUrgency = true,
}

local function roleName(value, fallback)
  if type(value) == "string" and value ~= "" then
    return value
  end
  return fallback
end

function M.colors()
  local saved = noctalia.state.get("timed.colors")
  if type(saved) ~= "table" then
    return {
      safe = DEFAULTS.safe,
      warning = DEFAULTS.warning,
      urgent = DEFAULTS.urgent,
      idle = DEFAULTS.idle,
      useUrgency = DEFAULTS.useUrgency,
    }
  end
  return {
    safe = roleName(saved.safe, DEFAULTS.safe),
    warning = roleName(saved.warning, DEFAULTS.warning),
    urgent = roleName(saved.urgent, DEFAULTS.urgent),
    idle = roleName(saved.idle, DEFAULTS.idle),
    useUrgency = saved.useUrgency ~= false,
  }
end

function M.publish(colors)
  noctalia.state.set("timed.colors", {
    safe = colors.safe,
    warning = colors.warning,
    urgent = colors.urgent,
    idle = colors.idle,
    useUrgency = colors.useUrgency,
  })
end

function M.state(timer, now)
  if timer.paused then
    return "paused"
  end
  local fraction = 0
  if timer.seconds > 0 then
    fraction = timersLib.remainingMs(timer, now) / (timer.seconds * 1000)
  end
  if fraction <= 0.2 then
    return "urgent"
  end
  if fraction <= 0.5 then
    return "warning"
  end
  return "safe"
end

function M.roleForState(state, colors)
  if state == "urgent" then
    return colors.urgent
  end
  if state == "warning" then
    return colors.warning
  end
  if state == "paused" then
    return "on_surface_variant"
  end
  return colors.safe
end

function M.role(timer, now, colors)
  local state = M.state(timer, now)
  if state == "paused" then
    return "on_surface_variant"
  end
  if not colors.useUrgency then
    return colors.safe
  end
  return M.roleForState(state, colors)
end

return M
