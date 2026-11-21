--!nonstrict
-- Tests for shared timer commands. Run with:
-- luau tests/timers_test.luau -a "$(cat timed/lib/timers.luau)"

local failures = 0

local function check(name, actual, expected)
  if actual ~= expected then
    failures += 1
    print(string.format("FAIL %s: expected %s, got %s", name, tostring(expected), tostring(actual)))
  end
end

local source = ...
if type(source) ~= "string" then
  error("pass timed/lib/timers.luau as a program argument")
end

local timers = {
  { id = "nearest", paused = false },
}
local command = nil

local env = {}
for key, value in pairs(getfenv(1)) do
  env[key] = value
end
env.noctalia = {
  nowMs = function()
    return 1000
  end,
  state = {
    watch = function() end,
    get = function(key)
      if key == "timed.list" then
        return { timers = timers }
      end
      if key == "timed.lastCommand" then
        return 0
      end
      return nil
    end,
    set = function(key, value)
      if key == "timed.command" then
        command = value
      end
    end,
  },
}
env.tonumber = tonumber
env.type = type
env.math = math
env.table = table

local loadModule = loadstring("return function()\n" .. source .. "\nend", "timers.luau")
local moduleFactory = loadModule()
setfenv(moduleFactory, env)
local timersLib = moduleFactory()

timersLib.toggle(timers[1])
check("toggle pauses a running timer", command and command.action, "pause")
check("toggle keeps the timer id", command and command.id, "nearest")

timers[1].paused = true
command = nil

timersLib.toggle(timers[1])
check("toggle resumes a paused timer", command and command.action, "resume")
check("resume keeps the timer id", command and command.id, "nearest")

command = nil

timersLib.toggle(nil)
check("toggle with no timer sends no command", command, nil)

if failures > 0 then
  error(string.format("%d timer check(s) failed", failures))
end

print("Timer tests passed")
