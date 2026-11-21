# Timed

Timed starts countdown timers from the Noctalia bar or launcher and posts a
notification when each one finishes. The optional message says what the timer
was for.

![Starting a 1 minute 5 second timer from the launcher](screenshot.webp)

## Plugin

| Field | Value |
| --- | --- |
| ID | `dontblameme/timed` |
| Entries | Bar widget: `timed_indicator`; launcher provider: `launcher`; panel: `panel`; service: `service` |
| Launcher Prefix | `/timer` |

## Requirements

The plugin requires a Noctalia release with plugin API 48 or newer.

## Bar widget

Add the `dontblameme/timed:timed_indicator` widget to a bar. It shows the
timer count inside a progress ring for the nearest timer, plus the remaining
time. Hover it to list every timer with its message and remaining time. Left-click
it to open the timer panel. Right-click it to pause or resume the nearest timer.

The ring uses the theme color. With timer colors enabled, it switches from the
safe color to the warning and urgent colors based on how much of the timer is
left. The widget settings can hide the time, count, or ring, turn timer colors
off, choose the safe, warning, urgent, and idle theme colors, and hide the
widget when no timers are running. The time is a coarse estimate, such as 4h,
30m, or 1m, and never drops below 1m.

The panel lists every timer with pause or resume and stop buttons. It also has
an input to start a new timer.

## Panel

Open the timer panel from the command line:

```sh
noctalia msg panel-toggle dontblameme/timed:panel
```

## Settings

The bar widget has these per-instance settings:

| Setting | What it does |
| --- | --- |
| Show time | Show the estimated remaining time next to the ring. |
| Show count | Show the number of timers inside the ring. |
| Show progress ring | Draw the ring for the nearest timer. |
| Hide when idle | Hide the widget when no timers are running. |
| Timer colors | Switch the ring color with timer progress. |
| Safe color | Theme color while more than half the timer remains. |
| Warning color | Theme color at half or less. |
| Urgent color | Theme color at a fifth or less. |
| Idle color | Theme color when no timers are running. |

## Usage

Open the launcher and type `/timer` followed by a duration.

| Launcher input | Result |
| --- | --- |
| `/timer 1h5` | Starts a 1 hour 5 minute timer |
| `/timer 1m5` | Starts a 1 minute 5 second timer |
| `/timer 3h20m10s` | Starts a 3 hour 20 minute 10 second timer |
| `/timer 1s4h` | Starts a 4 hour 1 second timer |
| `/timer 4s10h3m` | Starts a 10 hour 3 minute 4 second timer |
| `/timer 1y` | Starts a 1 year timer (365 days) |
| `/timer 5` | Starts a 5 minute timer |
| `/timer 3h2s something I want` | Starts a timer and uses the message in the notification |
| `/timer` | Shows how to start a new timer |
| `/timer pause` | Lists running timers to pause |
| `/timer resume` | Lists paused timers to resume |
| `/timer stop` | Lists timers to stop |
| `/timer stop all` | Stops all timers |

Each timer has a `Pause`, `Resume`, or `Stop` row with the optional message.
The start row keeps the launcher open on the `/timer` prompt.

Durations may contain `y`, `d`, `h`, `m`, and `s` in any order, and units may
repeat. A bare number means minutes. A bare number after a unit takes the next
smaller unit, so `1h5` means 1 hour 5 minutes and `1s4h5` means 4 hours 5
minutes 1 second. The maximum is 100 years.

The duration is the first word, and everything after it is the optional message,
so `/timer 3h2s something I want` notifies you with `something I want`. When the
whole input is a valid duration, including spaced forms such as `1h 5m`, it is
used as the duration. Because of that, a message that looks like a duration,
such as `30`, extends the timer instead. Start such a message with a word.

## IPC

The service accepts commands, which is handy for keybinds and for testing:

```sh
noctalia msg plugin dontblameme/timed:service all start 1m5
noctalia msg plugin dontblameme/timed:service all start "3h2s something I want"
noctalia msg plugin dontblameme/timed:service all list
noctalia msg plugin dontblameme/timed:service all pause <id>
noctalia msg plugin dontblameme/timed:service all resume <id>
noctalia msg plugin dontblameme/timed:service all stop <id>
noctalia msg plugin dontblameme/timed:service all stop-all
```

`start` takes the same duration syntax and optional message as the launcher.
`list` prints each timer id, which the pause, resume, and stop commands use.

## Notes

Timers are saved to the plugin data directory, so they survive a Noctalia
restart and a reboot. Remaining time is computed from each stored end time. A
paused timer stores its remaining time. A timer that ends while Noctalia is not
running notifies when the shell starts again.

Completion notifications use Noctalia's own notification, so they follow the
notification timeout configured in the shell.

The service sets its update interval to the next timer expiry, capped at once a
day. A timer due in a year wakes the plugin once a day until it is close, and
the timer list is published only when it changes. Running timers cost nothing
between those checks.
