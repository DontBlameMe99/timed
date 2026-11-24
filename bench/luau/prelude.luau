--!nonstrict
-- Mock Noctalia host for the Luau side of the paired benchmark.
--
-- It implements the host surface the Timed entries use, records host calls as
-- ops in the same JSON shape noctalia-rs serializes, and keeps files in memory
-- so a case measures plugin logic instead of disk latency. The Rust side of the
-- same case runs the real handler and writes real files.

local OPS = {}
local FILES = {}
local DECODED = {}
local STATE = {}
local CONFIG = {}
local MODULES = {}
local NOW = 0
local VERTICAL = false
local OUTPUT = "DP-1"
local FONT = "sans-serif"
local BRIDGE_LINE = nil

local function marker(table)
  table.__array = true
  return table
end

local function quote(text)
  local escapes = {
    ['"'] = '\\"',
    ["\\"] = "\\\\",
    ["\n"] = "\\n",
    ["\r"] = "\\r",
    ["\t"] = "\\t",
  }
  local out = text:gsub('[%c"\\]', function(character)
    return escapes[character] or string.format("\\u%04x", string.byte(character))
  end)
  return '"' .. out .. '"'
end

local function is_array(value)
  if value.__array then
    return true
  end
  local count = 0
  for _ in pairs(value) do
    count = count + 1
  end
  return count > 0 and value[1] ~= nil
end

local function encode(value)
  local kind = type(value)
  if value == nil then
    return "null"
  end
  if kind == "boolean" then
    return value and "true" or "false"
  end
  if kind == "number" then
    if value ~= value or value == math.huge or value == -math.huge then
      return "null"
    end
    if value == math.floor(value) and math.abs(value) < 1e15 then
      return string.format("%d", value)
    end
    return string.format("%.17g", value)
  end
  if kind == "string" then
    return quote(value)
  end
  if kind ~= "table" then
    return "null"
  end

  local parts = {}
  if is_array(value) then
    for index = 1, #value do
      parts[index] = encode(value[index])
    end
    return "[" .. table.concat(parts, ",") .. "]"
  end

  local keys = {}
  for key in pairs(value) do
    if type(key) == "string" and key:sub(1, 2) ~= "__" then
      keys[#keys + 1] = key
    end
  end
  table.sort(keys)
  for index, key in ipairs(keys) do
    parts[index] = quote(key) .. ":" .. encode(value[key])
  end
  return "{" .. table.concat(parts, ",") .. "}"
end

local function push(op)
  OPS[#OPS + 1] = op
end

local function node(kind, props, children)
  return {
    __node = true,
    kind = kind,
    props = props or {},
    children = children and marker(children) or marker({}),
  }
end

local function tr(key, params)
  local text = __TRANSLATIONS[key] or key
  if params then
    for name, value in pairs(params) do
      text = text:gsub("{" .. name .. "}", tostring(value))
    end
  end
  return text
end

-- Module names are unique in Timed, so a suffix match resolves the relative
-- names the plugin uses (`./timers.luau` from inside `lib/`).
require = function(name)
  local key = name:gsub("^%./", ""):gsub("%.luau$", "")
  if MODULES[key] ~= nil then
    return MODULES[key]
  end
  local source = __SOURCES[key]
  if source == nil then
    for candidate, body in pairs(__SOURCES) do
      if candidate == key or candidate:sub(-(#key + 1)) == "/" .. key then
        key = candidate
        source = body
        break
      end
    end
  end
  assert(source, "bench: missing module " .. name)
  local chunk = assert(loadstring(source, key))
  local value = chunk()
  MODULES[key] = value
  return value
end

noctalia = {
  log = function(message)
    push({ op = "log", message = tostring(message) })
  end,
  notify = function(title, body)
    push({ op = "notify", title = tostring(title), body = tostring(body) })
  end,
  notifyError = function(title, body)
    push({ op = "notify_error", title = tostring(title), body = tostring(body) })
  end,
  setUpdateInterval = function(ms)
    push({ op = "set_interval", ms = ms })
  end,
  togglePanel = function(id)
    push({ op = "toggle_panel", id = id })
  end,
  nowMs = function()
    return NOW
  end,
  getColor = function(role)
    return __COLORS[role]
  end,
  getSetting = function(path)
    if path == "shell.font_family" then
      return FONT
    end
    return nil
  end,
  getConfig = function(key)
    return CONFIG[key]
  end,
  pluginDataDir = function()
    return "/data"
  end,
  pluginDir = function()
    return "/plugin"
  end,
  mkdirAll = function()
    return true
  end,
  runStream = function(_, onLine)
    BRIDGE_LINE = onLine
    return true
  end,
  readFile = function(path)
    return FILES[path]
  end,
  writeFile = function(path, contents)
    FILES[path] = contents
    return true
  end,
  renameFile = function(from, to)
    FILES[to] = FILES[from]
    FILES[from] = nil
    return true
  end,
  removeFile = function(path)
    FILES[path] = nil
    return true
  end,
  formatTime = function(format, stamp)
    return os.date(format, stamp)
  end,
  timeFormat = function()
    return "%H:%M"
  end,
  dateFormat = function()
    return "%Y-%m-%d"
  end,
  tr = tr,
  string = {
    trim = function(text)
      return (tostring(text or ""):gsub("^%s+", ""):gsub("%s+$", ""))
    end,
  },
  json = {
    encode = encode,
    decode = function(text)
      return DECODED[text]
    end,
  },
  state = {
    get = function(key)
      return STATE[key]
    end,
    set = function(key, value)
      STATE[key] = value
      push({ op = "state_set", key = key, value = value })
    end,
    watch = function(key, callback)
      push({ op = "state_watch", key = key })
      STATE["__watch:" .. key] = callback
    end,
  },
}

barWidget = {
  isVertical = function()
    return VERTICAL
  end,
  outputName = function()
    return OUTPUT
  end,
  setText = function(text)
    push({ op = "bar_text", text = text })
  end,
  setColor = function(role)
    push({ op = "bar_color", role = role })
  end,
  setVisible = function(visible)
    push({ op = "bar_visible", visible = visible })
  end,
  setTooltip = function(value)
    push({ op = "bar_tooltip", value = value })
  end,
  setImage = function(path, preserve, width, height)
    push({
      op = "bar_image",
      path = path,
      preserve = preserve,
      width = width,
      height = height,
    })
  end,
  render = function(tree)
    push({ op = "bar_render", tree = tree })
  end,
}

panel = {
  render = function(tree)
    push({ op = "panel_render", tree = tree })
  end,
  close = function() end,
  setWantsSecondTicks = function() end,
}

launcher = {
  setResults = function(text, rows)
    push({ op = "launcher_results", text = text, rows = rows })
  end,
  setQuery = function() end,
}

ui = {
  column = function(props, children)
    return node("column", props, children)
  end,
  row = function(props, children)
    return node("row", props, children)
  end,
  scroll = function(props, children)
    return node("scroll", props, children)
  end,
  label = function(props)
    return node("label", props)
  end,
  button = function(props)
    return node("button", props)
  end,
  input = function(props)
    return node("input", props)
  end,
  progress = function(props)
    return node("progress", props)
  end,
}

__host = {}

function __host.set_now(now)
  NOW = now
end

function __host.set_config(config)
  CONFIG = config
end

function __host.set_state(state)
  STATE = state
end

function __host.set_vertical(vertical)
  VERTICAL = vertical
end

function __host.seed_file(path, value)
  local text = encode(value)
  FILES[path] = text
  DECODED[text] = value
end

function __host.load_entry(name)
  local source = __SOURCES[name]
  assert(source, "bench: missing entry " .. name)
  assert(loadstring(source, name))()
  return OPS
end

function __host.reset()
  OPS = {}
end

function __host.ops()
  return OPS
end

function __host.watch(key, ...)
  local callback = STATE["__watch:" .. key]
  if callback then
    callback(...)
  end
end

function __host.encode(value)
  return encode(value)
end

-- Feeds the real bridge its first host line, which flushes the queued request
-- and unlocks the snapshot path.
function __host.bridge_ready()
  assert(BRIDGE_LINE, "bridge did not start its stream")
  local message = { control = "ready", boot = "bench", spool = "/spool" }
  local text = encode(message)
  DECODED[text] = message
  BRIDGE_LINE(text)
end

-- Warmup, then timed batches of `__BATCH` dispatches with the clock advanced by
-- one second each. Mirrors the Rust timing loop exactly.
function __host.time(prepare, dispatch)
  local count = 0
  for _ = 1, __WARMUP do
    count = count + 1
    prepare(__START + count * 1000)
    __host.reset()
    dispatch()
  end

  local times = marker({})
  for sample = 1, __SAMPLES do
    local started = os.clock()
    for _ = 1, __BATCH do
      count = count + 1
      prepare(__START + count * 1000)
      __host.reset()
      dispatch()
    end
    times[sample] = (os.clock() - started) * 1000000 / __BATCH
  end
  print("TIMES " .. encode(times))
end

-- Parity dispatch on a fresh module, then timing.
function __host.run(parity_now, prepare, dispatch, step)
  prepare(parity_now)
  __host.reset()
  dispatch()
  print("OPS " .. encode(OPS))
  __host.time(prepare, step or dispatch)
end
