//! Shared timer state, persistence shape, and command channel.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::sync::Mutex;
use std::thread_local;

use noctalia_rs::{Context, Op};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::duration::DAY;
use crate::math::{quotient, remainder};
use crate::sync::lock;

/// Key the service publishes the timer list to. This is the bridge's shared
/// plugin state key, so every entry renders when the list changes.
pub const STATE_KEY: &str = "timed.state";
pub const COMMAND_KEY: &str = "timed.command";
pub const LAST_COMMAND_KEY: &str = "timed.lastCommand";
pub const COLORS_KEY: &str = "timed.colors";

pub const FILE_NAME: &str = "timers.json";

#[derive(Clone, Debug, Default, PartialEq, Hash, Serialize, Deserialize)]
pub struct Timer {
    pub id: String,
    #[serde(default)]
    pub seconds: i64,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub paused: bool,
    #[serde(
        rename = "endsAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ends_at: Option<i64>,
    #[serde(
        rename = "remainingMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub remaining_ms: Option<i64>,
}

impl Timer {
    #[must_use]
    pub fn remaining_ms(&self, now: i64) -> i64 {
        if self.paused {
            return self.remaining_ms.unwrap_or(0).max(0);
        }
        self.ends_at.map_or(0, |end| end.saturating_sub(now)).max(0)
    }

    #[must_use]
    pub fn remaining_seconds(&self, now: i64) -> i64 {
        let remaining = self.remaining_ms(now);
        quotient(remaining, 1_000)
            .saturating_add(i64::from(remainder(remaining, 1_000) != 0))
    }

    /// The end time as the shell formats it: a clock for today, a date for a
    /// timer that runs past a day.
    #[must_use]
    pub fn ends_at_text(&self, ctx: &Context, clock: &Clock) -> String {
        let format = if self.remaining_seconds(clock.now) >= DAY {
            clock.date.as_str()
        } else {
            clock.time.as_str()
        };
        self.ends_at.map_or_else(String::new, |end| {
            format_time(ctx, format, end.div_euclid(1_000))
        })
    }

    /// Remaining fraction of the timer, quantized to `1 / steps` units and
    /// rounded to the nearest step. The ring and the progress bar are drawn
    /// from this, so a redraw happens only when the drawn value moves.
    #[must_use]
    pub fn fraction_step(&self, now: i64, steps: u16) -> u16 {
        self.fraction_step_ms(self.remaining_ms(now), steps)
    }

    #[must_use]
    pub(crate) fn fraction_step_ms(&self, remaining: i64, steps: u16) -> u16 {
        let total = self.seconds.saturating_mul(1_000);
        if total <= 0 {
            return steps;
        }
        let clamped = remaining.clamp(0, total);
        let total_steps = i64::from(steps);
        let scaled = clamped
            .saturating_mul(total_steps)
            .saturating_mul(2)
            .saturating_add(total);
        let divisor = total.saturating_mul(2);
        let step = quotient(scaled, divisor).clamp(0, total_steps);
        u16::try_from(step).unwrap_or(0)
    }

    /// Whether `fraction` of the timer is at or below `share`, in percent.
    #[must_use]
    pub fn within(&self, now: i64, share: i64) -> bool {
        self.within_ms(self.remaining_ms(now), share)
    }

    #[must_use]
    pub(crate) const fn within_ms(&self, remaining: i64, share: i64) -> bool {
        if self.seconds <= 0 {
            return true;
        }
        remaining.saturating_mul(100)
            <= self.seconds.saturating_mul(1_000).saturating_mul(share)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct List {
    #[serde(default)]
    pub timers: Vec<Timer>,
}

#[must_use]
pub fn list(ctx: &Context) -> Vec<Timer> {
    ctx.state_value::<Vec<Timer>>("timers").unwrap_or_default()
}

#[must_use]
pub fn state_revision(ctx: &Context) -> Option<i64> {
    ctx.host.get("timed_state_version").and_then(Value::as_i64)
}

type TimeCache = Vec<(String, i64, String)>;

thread_local! {
    static TIMES: RefCell<TimeCache> = const { RefCell::new(Vec::new()) };
}

/// Formats a timer end time. The shell asks for the same end time on every
/// render, so keep the last few results instead of formatting through chrono
/// again each second.
#[must_use]
pub fn format_time(ctx: &Context, format: &str, seconds: i64) -> String {
    if let Some(cached) = TIMES.with(|times| {
        times
            .borrow()
            .iter()
            .find(|(pattern, at, _)| pattern == format && *at == seconds)
            .map(|(_, _, text)| text.clone())
    }) {
        return cached;
    }
    let text = ctx.format_time_at(format, Some(seconds), None);
    TIMES.with(|slot| {
        let mut cache = slot.borrow_mut();
        if cache.len() >= 64 {
            cache.remove(0);
        }
        cache.push((format.to_owned(), seconds, text.clone()));
    });
    text
}

/// The render clock and the shell's time and date formats, read once per
/// event.
#[derive(Clone, Debug, Default, Hash)]
pub struct Clock {
    pub now: i64,
    pub time: String,
    pub date: String,
}

impl Clock {
    #[must_use]
    pub fn read(ctx: &Context) -> Self {
        Self {
            now: ctx.now_ms,
            time: ctx
                .setting("shell.time_format")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            date: ctx
                .setting("shell.date_format")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        }
    }
}

pub fn sort(timers: &mut [Timer]) {
    timers.sort_by(|left, right| match (left.paused, right.paused) {
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        (true, true) => left
            .remaining_ms
            .unwrap_or(0)
            .cmp(&right.remaining_ms.unwrap_or(0)),
        (false, false) => {
            left.ends_at.unwrap_or(0).cmp(&right.ends_at.unwrap_or(0))
        }
    });
}

static SEQUENCE: Mutex<Option<i64>> = Mutex::new(None);

pub(crate) fn seed_sequence(value: i64) {
    *lock(&SEQUENCE) = Some(value);
}

fn next_sequence(ctx: &Context) -> i64 {
    let mut sequence = lock(&SEQUENCE);
    let current = sequence.unwrap_or_else(|| {
        ctx.global_state(LAST_COMMAND_KEY)
            .and_then(|value| value.as_i64())
            .unwrap_or(0)
    });
    let next = ctx.now_ms.max(current.saturating_add(1));
    *sequence = Some(next);
    next
}

/// Publishes a command for the service. Every command carries a sequence
/// number so a stale write is ignored.
#[must_use]
pub fn send(ctx: &Context, mut command: Value) -> Op {
    if let Some(object) = command.as_object_mut() {
        object.insert("n".to_owned(), json!(next_sequence(ctx)));
    }
    Op::state(COMMAND_KEY, command)
}

/// Starts a timer, carrying the message into its notification.
#[must_use]
pub fn start_command(ctx: &Context, seconds: i64, message: Option<&str>) -> Op {
    let mut command = json!({ "action": "start", "seconds": seconds });
    if let (Some(body), Some(object)) = (message, command.as_object_mut()) {
        object.insert("message".to_owned(), json!(body));
    }
    send(ctx, command)
}

#[must_use]
pub fn toggle(ctx: &Context, timer: Option<&Timer>) -> Option<Op> {
    let entry = timer?;
    let action = if entry.paused { "resume" } else { "pause" };
    Some(send(ctx, json!({ "action": action, "id": entry.id })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> Timer {
        Timer {
            id: "one".to_owned(),
            seconds: 600,
            label: "10m".to_owned(),
            message: None,
            paused: false,
            ends_at: Some(601_000),
            remaining_ms: None,
        }
    }

    #[test]
    fn remaining_ms_follows_end_time_or_pause() {
        assert_eq!(running().remaining_ms(1_000), 600_000);
        assert_eq!(running().remaining_ms(700_000), 0);

        let paused = Timer {
            paused: true,
            ends_at: None,
            remaining_ms: Some(12_345),
            ..running()
        };
        assert_eq!(paused.remaining_ms(0), 12_345);
    }

    #[test]
    fn remaining_seconds_rounds_up() {
        let timer = running();
        assert_eq!(timer.remaining_seconds(600_001), 1);
        assert_eq!(timer.remaining_seconds(0), 601);
    }

    #[test]
    fn sort_puts_running_first_and_by_end_time() {
        let mut late = Timer {
            id: "late".to_owned(),
            ends_at: Some(9_000),
            ..running()
        };
        let early = Timer {
            id: "early".to_owned(),
            ends_at: Some(5_000),
            ..running()
        };
        let paused = Timer {
            id: "paused".to_owned(),
            paused: true,
            ends_at: None,
            remaining_ms: Some(1_000),
            ..running()
        };
        let mut timers = vec![late.clone(), paused, early];
        sort(&mut timers);
        assert_eq!(
            timers
                .iter()
                .map(|timer| timer.id.as_str())
                .collect::<Vec<_>>(),
            vec!["early", "late", "paused"]
        );
        late.paused = true;
    }

    #[test]
    fn fraction_step_quantizes_the_remaining_fraction() {
        let timer = running();
        assert_eq!(timer.fraction_step(1_000, 200), 200);
        assert_eq!(timer.fraction_step(301_000, 200), 100);
        assert_eq!(timer.fraction_step(599_000, 200), 1);
        assert_eq!(timer.fraction_step(600_000, 200), 0);
        assert_eq!(timer.fraction_step(700_000, 200), 0);
    }

    #[test]
    fn fraction_step_is_stable_within_a_step() {
        let timer = running();
        // 600 s over 200 steps is 3 s per step.
        assert_eq!(
            timer.fraction_step(100_000, 200),
            timer.fraction_step(100_900, 200)
        );
        assert_ne!(
            timer.fraction_step(100_000, 200),
            timer.fraction_step(103_100, 200)
        );
    }

    #[test]
    fn within_uses_remaining_percentage() {
        let timer = running();
        assert!(!timer.within(0, 20));
        assert!(timer.within(481_000, 20));
        assert!(!timer.within(480_999, 20));
        assert!(timer.within(301_000, 50));
        assert!(!timer.within(300_999, 50));
    }

    #[test]
    fn serialized_shape_matches_saved_file() {
        let list = List {
            timers: vec![running()],
        };
        let value = serde_json::to_value(&list).expect("value");
        assert_eq!(
            value,
            json!({ "timers": [{
                "id": "one",
                "seconds": 600,
                "label": "10m",
                "paused": false,
                "endsAt": 601_000
            }]})
        );
    }
}
