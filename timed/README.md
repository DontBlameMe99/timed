# Timed

<p align="center"><img src="thumbnail.webp" alt="Timed" width="480"></p>

Timed starts countdown timers from the Noctalia launcher and sends a
notification when each one finishes. Notifications stay visible until
dismissed.

## Installation

Requires a Noctalia release with plugin API 48, and `notify-send` from
`libnotify` for persistent notifications.

### Install as a plugin source

```sh
noctalia msg plugins source add timed git https://github.com/DontBlameMeDev/timed.git
noctalia msg plugins enable dontblameme/timed
```

Noctalia loads the plugin from the `timed/` directory in this repository.

### Local development

Clone the repository and link the plugin directory:

```sh
git clone https://github.com/DontBlameMeDev/timed.git ~/projects/timed
ln -sfn ~/projects/timed/timed ~/.local/share/noctalia/plugins/timed
noctalia msg plugins enable dontblameme/timed
```

Luau edits hot-reload. If `plugin.toml` changes, reload the config:

```sh
noctalia msg config-reload
```

### Update

For a source install:

```sh
noctalia msg plugins update timed
```

For a checkout linked with a symlink, pull the repository. Noctalia picks up
Luau changes automatically. Reload the config after manifest changes:

```sh
git -C ~/projects/timed pull
noctalia msg config-reload
```

### Uninstall

```sh
noctalia msg plugins disable dontblameme/timed
```

Remove the source if you added it:

```sh
noctalia msg plugins source remove timed
```

For a local symlink, remove the link:

```sh
rm ~/.local/share/noctalia/plugins/timed
```

## Usage

| Launcher input | Result |
| --- | --- |
| `/timer 1h5` | Starts a 1 hour 5 minute timer |
| `/timer 1m5` | Starts a 1 minute 5 second timer |
| `/timer 3h20m10s` | Starts a 3 hour 20 minute 10 second timer |
| `/timer 1s4h` | Starts a 4 hour 1 second timer |
| `/timer 1h5s` | Units may appear in any order |
| `/timer 4s10h3m` | Starts a 10 hour 3 minute 4 second timer |
| `/timer 3m1h3s` | Starts a 1 hour 3 minute 3 second timer |
| `/timer 1h1h` | Duplicate units are added together |
| `/timer 1y` | Starts a 1 year timer (365 days) |
| `/timer 5` | Starts a 5 minute timer |
| `/timer 3h2s something I want` | Starts a timer and uses the message in the notification |
| `/timer` | Lists running timers, soonest first |
| `/timer stop` | Stops all running timers |

Selecting a timer in the `/timer` list stops that timer. A bare number means
minutes. A bare number after a unit takes the next smaller unit, so `1h5`
means 1 hour 5 minutes and `1s4h5` means 4 hours 5 minutes 1 second. Durations
may contain `y`, `d`, `h`, `m`, and `s` in any order. The maximum is 100 years.

The duration is the first word, and everything after it is the optional
message, so `/timer 3h2s something I want` notifies you with
`something I want`. If the whole input is a valid duration, including spaced
forms such as `1h 5m`, it is used as the duration. Because of that, a message
that looks like a duration, such as `30`, extends the timer instead. Start such
a message with a word.

## Notifications

Completion notifications are sent through `notify-send` with an expiry time of
0, so they remain until dismissed. This requires `notify-send` from
`libnotify`. If it is unavailable, Timed falls back to Noctalia's own
notification, which uses the shell's default timeout.

## Restarts and reboots

Timers are saved to the plugin's data directory, so they survive a Noctalia
restart and a reboot. Remaining time is computed from each stored end time.

If a timer ends while Noctalia is not running, its notification appears when
the shell starts again.

## Performance

The service sets its update interval to the next timer expiry, capped at once
a day. A timer due in a year wakes the plugin once a day until it is close, and
the timer list is published only when it changes. Running timers cost nothing
between those checks.

## IPC

The service also accepts commands, which is handy for keybinds and for testing:

```sh
noctalia msg plugin dontblameme/timed:service all start 1m5
noctalia msg plugin dontblameme/timed:service all start "3h2s something I want"
noctalia msg plugin dontblameme/timed:service all list
noctalia msg plugin dontblameme/timed:service all stop-all
```

`start` takes the same duration syntax and optional message as the launcher.

## Test

Start a short timer and watch it finish:

```sh
noctalia msg plugin dontblameme/timed:service all start 5s
noctalia msg plugin dontblameme/timed:service all list
```

A notification titled `Timed` appears after 5 seconds and stays until you
dismiss it. For the launcher, open it and type `/timer 1m5`, then press Enter.
Open it again and type `/timer` to see the running timer, then select the row to
stop it. Try `/timer 3h2s something I want` to see the message in the
notification.
