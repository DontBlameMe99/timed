--!nonstrict
-- Tests for timed/lib/duration.luau. Run with: luau tests/duration_test.luau

local duration = require("../timed/lib/duration")

local failures = 0

local function check(name, actual, expected)
  if actual ~= expected then
    failures += 1
    print(string.format("FAIL %s: expected %s, got %s", name, tostring(expected), tostring(actual)))
  end
end

local function checkParse(input, expected)
  check("parse(" .. tostring(input) .. ")", duration.parse(input), expected)
end

checkParse("5", 300)
checkParse("1h5", 3900)
checkParse("1m5", 65)
checkParse("3h20m10s", 12010)
checkParse("1s4h", 14401)
checkParse("4s10h3m", 36184)
checkParse("1h 5m", 3900)
checkParse("1y", 31536000)
checkParse("1y2y", 94608000)
checkParse("100y", 3153600000)
checkParse("101y", nil)
checkParse("1s5", nil)
checkParse("0", nil)
checkParse("", nil)
checkParse("abc", nil)
checkParse("1x", nil)
checkParse("1.5h", nil)

check("split spaced duration", duration.split("1h 5m"), 3900)
check("split duration and message", duration.split("3h2s something I want"), 10802)
check("split bare minutes and message", duration.split("30 walk the dog"), 1800)

local seconds, message = duration.split("3h2s something I want")
check("split message", message, "something I want")

local extended, ignored = duration.split("1h5 30")
check("split duration-like message", extended, 35400)
check("split duration-like message is dropped", ignored, nil)

check("split empty", duration.split(""), nil)
check("split invalid", duration.split("nope"), nil)

check("clock zero", duration.clock(0), "0:00")
check("clock seconds", duration.clock(65), "1:05")
check("clock hour", duration.clock(3900), "1:05:00")
check("clock day", duration.clock(90061), "1d 1:01:01")
check("clock negative", duration.clock(-5), "0:00")

check("human zero", duration.human(0), "0s")
check("human hours", duration.human(3900), "1h 5m")
check("human year", duration.human(31536000), "1y")
check("human mixed", duration.human(90061), "1d 1h 1m 1s")

if failures > 0 then
  error(string.format("%d duration check(s) failed", failures))
end

print("Duration tests passed")
