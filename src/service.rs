//! Timed service: owns the timer list, wakes at the next expiry, and notifies
//! when a timer finishes.
//!
//! Timers can be paused. A running timer stores its end time, a paused timer
//! stores its remaining time, so a pause survives a shell restart.
//!
//! The update interval follows the soonest running timer instead of ticking
//! every second, and state is published only when the list changes. A year-long
//! timer re-checks once a day instead of every second.
//!
//! The list is also saved to the plugin data directory, so timers survive a
//! shell restart and a reboot. A timer that expired while the shell was gone
//! notifies on the next start.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use noctalia_rs::{Context, Op, Result};
use serde_json::Value;

use crate::duration;
use crate::file;
use crate::i18n::tr;
use crate::math::quotient;
use crate::sync::lock;
use crate::timer::{
    COMMAND_KEY, FILE_NAME, LAST_COMMAND_KEY, STATE_KEY, Timer, seed_sequence,
    sort,
};

const MAX_INTERVAL_MS: i64 = 86_400_000;

struct Service {
    timers: Vec<Timer>,
    last_command: i64,
    ipc_seq: i64,
    saved: Option<String>,
    interval: Option<i64>,
    loaded: bool,
}

impl Service {
    const fn new() -> Self {
        Self {
            timers: Vec::new(),
            last_command: 0,
            ipc_seq: 0,
            saved: None,
            interval: None,
            loaded: false,
        }
    }
}

static SERVICE: Mutex<Service> = Mutex::new(Service::new());

/// Handles one service event.
///
/// # Errors
/// The signature matches the entry contract; this handler does not fail.
pub fn update(ctx: &Context) -> Result<Vec<Op>> {
    let mut service = lock(&SERVICE);
    let mut ops = Vec::new();

    if !service.loaded {
        start(ctx, &mut service, &mut ops);
        return Ok(ops);
    }
    // A command or IPC event can arrive after an end time but before the tick.
    // Expire first so no branch sees a timer that is already finished.
    let expired = expire(ctx, &mut service, &mut ops);
    let mut changed = false;

    match ctx.event.as_str() {
        "update" | "resume" => {
            if !expired {
                schedule(ctx, &mut service, &mut ops);
            }
        }
        "ipc" => changed = ipc(ctx, &mut service, &mut ops),
        "host_callback" => {
            if ctx.payload_str("id").as_deref() == Some("command")
                && let Some(command) = ctx.payload_arg(0)
            {
                changed = handle_command(ctx, &mut service, command, &mut ops);
            }
        }
        _ => {}
    }
    if expired || changed {
        commit(ctx, &mut service, &mut ops);
    }

    Ok(ops)
}

fn start(ctx: &Context, service: &mut Service, ops: &mut Vec<Op>) {
    service.loaded = true;
    service.last_command = ctx
        .host
        .get("timed_last_command")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    load(ctx, service, ops);
    sort(&mut service.timers);
    ops.push(Op::state_watch(COMMAND_KEY, "command"));

    if let Some(command) = ctx.host.get("timed_command") {
        handle_command(ctx, service, command, ops);
    }
    seed_sequence(service.last_command);
    expire(ctx, service, ops);
    commit(ctx, service, ops);
}

fn state_path(ctx: &Context) -> PathBuf {
    ctx.data_dir.join(FILE_NAME)
}

fn load(ctx: &Context, service: &mut Service, ops: &mut Vec<Op>) {
    let Ok(contents) = fs::read_to_string(state_path(ctx)) else {
        return;
    };
    let Some(saved) = serde_json::from_str::<Value>(&contents)
        .ok()
        .and_then(|value| value.get("timers").cloned())
        .and_then(|value| value.as_array().cloned())
    else {
        ops.push(Op::log("timed: ignoring unreadable saved timers"));
        return;
    };

    service.saved = Some(contents);
    service.timers = saved.iter().filter_map(normalize).collect();
}

/// Accepts only complete entries, so a half-written file cannot resurrect a
/// broken timer.
fn normalize(saved: &Value) -> Option<Timer> {
    let id = saved.get("id")?.as_str()?.to_owned();
    let seconds = saved.get("seconds")?.as_i64()?;
    let label = saved.get("label")?.as_str()?.to_owned();
    let message = match saved.get("message") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => return None,
    };
    let paused = saved.get("paused").and_then(Value::as_bool) == Some(true);

    let mut timer = Timer {
        id,
        seconds,
        label,
        message,
        paused,
        ends_at: None,
        remaining_ms: None,
    };
    if paused {
        timer.remaining_ms = Some(saved.get("remainingMs")?.as_i64()?.max(0));
    } else {
        timer.ends_at = Some(saved.get("endsAt")?.as_i64()?);
    }
    Some(timer)
}

fn publish(ctx: &Context, service: &mut Service, ops: &mut Vec<Op>) {
    #[derive(serde::Serialize)]
    struct Saved<'a> {
        timers: &'a [Timer],
    }
    let list = Saved {
        timers: &service.timers,
    };
    ops.push(Op::state(
        STATE_KEY,
        serde_json::to_value(&list).unwrap_or(Value::Null),
    ));

    let Ok(encoded) = serde_json::to_string(&list) else {
        return;
    };
    if service.saved.as_deref() == Some(encoded.as_str()) {
        return;
    }

    match file::write_atomic(&state_path(ctx), &encoded) {
        Ok(()) => service.saved = Some(encoded),
        Err(error) => {
            ops.push(Op::log(format!("timed: cannot save timers: {error}")));
        }
    }
}

fn schedule(ctx: &Context, service: &mut Service, ops: &mut Vec<Op>) {
    let delay = service
        .timers
        .iter()
        .find(|timer| !timer.paused)
        .and_then(|timer| timer.ends_at)
        .map_or(MAX_INTERVAL_MS, |end| {
            end.saturating_sub(ctx.now_ms)
                .saturating_add(50)
                .clamp(100, MAX_INTERVAL_MS)
        });
    if service.interval == Some(delay) {
        return;
    }
    service.interval = Some(delay);
    ops.push(Op::set_interval(delay.unsigned_abs()));
}

/// Notifies and drops every finished timer, reporting whether any were. The
/// list is sorted, so the finished ones are the prefix before the first live
/// timer.
fn expire(ctx: &Context, service: &mut Service, ops: &mut Vec<Op>) -> bool {
    let now = ctx.now_ms;
    let live = service
        .timers
        .iter()
        .position(|timer| {
            timer.paused || timer.ends_at.is_some_and(|end| end > now)
        })
        .unwrap_or(service.timers.len());
    if live == 0 {
        return false;
    }

    let finished: Vec<Timer> = service.timers.drain(..live).collect();
    for timer in &finished {
        notify_finished(ctx, timer, ops);
        ops.push(Op::log(format!("timed: finished {}", timer.id)));
    }
    true
}

fn notify_finished(ctx: &Context, timer: &Timer, ops: &mut Vec<Op>) {
    ops.push(Op::Notify {
        title: tr(ctx, "notify.title", &[]),
        body: timer.message.clone().unwrap_or_else(|| {
            tr(ctx, "notify.body", &[("duration", timer.label.clone())])
        }),
        options: None,
    });
}

fn stop_by_id(service: &mut Service, id: &str) -> bool {
    let before = service.timers.len();
    service.timers.retain(|timer| timer.id != id);
    service.timers.len() != before
}

fn pause_by_id(ctx: &Context, service: &mut Service, id: &str) -> bool {
    let now = ctx.now_ms;
    let Some(timer) = service
        .timers
        .iter_mut()
        .find(|timer| timer.id == id && !timer.paused)
    else {
        return false;
    };
    let remaining = timer.ends_at.map_or(0, |end| end.saturating_sub(now));
    if remaining <= 0 {
        return false;
    }
    timer.paused = true;
    timer.remaining_ms = Some(remaining);
    timer.ends_at = None;
    true
}

fn stop_all(service: &mut Service) -> bool {
    let had_timers = !service.timers.is_empty();
    service.timers.clear();
    had_timers
}

/// Sorts, publishes, and schedules the next expiry after a change.
fn commit(ctx: &Context, service: &mut Service, ops: &mut Vec<Op>) {
    sort(&mut service.timers);
    publish(ctx, service, ops);
    schedule(ctx, service, ops);
}

fn resume_by_id(ctx: &Context, service: &mut Service, id: &str) -> bool {
    let Some(timer) = service
        .timers
        .iter_mut()
        .find(|timer| timer.id == id && timer.paused)
    else {
        return false;
    };
    let remaining = timer.remaining_ms.unwrap_or(0);
    timer.paused = false;
    timer.ends_at = Some(ctx.now_ms.saturating_add(remaining));
    timer.remaining_ms = None;
    true
}

#[expect(
    clippy::too_many_arguments,
    reason = "the context, the owner, the new timer, and the op sink"
)]
fn start_timer(
    ctx: &Context,
    service: &mut Service,
    seconds: i64,
    id: &str,
    message: Option<String>,
    ops: &mut Vec<Op>,
) -> bool {
    if seconds < 1 {
        return false;
    }
    let label = duration::human(seconds);
    service.timers.push(Timer {
        id: id.to_owned(),
        seconds,
        label: label.clone(),
        message,
        paused: false,
        ends_at: Some(ctx.now_ms.saturating_add(seconds.saturating_mul(1_000))),
        remaining_ms: None,
    });
    ops.push(Op::log(format!("timed: started {id} for {label}")));
    true
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "seconds are floored and bounded well below 2^53"
)]
fn command_seconds(command: &Value) -> Option<i64> {
    let value = command.get("seconds")?;
    let number = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse().ok()?,
        Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null => {
            return None;
        }
    };
    if !number.is_finite() {
        return None;
    }
    Some(number.floor() as i64)
}

fn command_id(command: &Value) -> Option<String> {
    let value = command.get("id").or_else(|| command.get("n"))?;
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null => {
            None
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the context, the owner, the command, and the op sink"
)]
fn handle_command(
    ctx: &Context,
    service: &mut Service,
    command: &Value,
    ops: &mut Vec<Op>,
) -> bool {
    let Some(sequence) = command.get("n").and_then(Value::as_i64) else {
        return false;
    };
    if sequence <= service.last_command {
        return false;
    }
    service.last_command = sequence;
    ops.push(Op::state(LAST_COMMAND_KEY, Value::from(sequence)));
    apply(ctx, service, command, ops)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the context, the owner, the command, and the op sink"
)]
fn apply(
    ctx: &Context,
    service: &mut Service,
    command: &Value,
    ops: &mut Vec<Op>,
) -> bool {
    let Some(action) = command.get("action").and_then(Value::as_str) else {
        return false;
    };
    let target = command_id(command);
    let id = target.as_deref();
    let mut changed = false;

    match action {
        "start" => {
            if let (Some(seconds), Some(new)) = (command_seconds(command), id) {
                let message = command
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                changed = start_timer(ctx, service, seconds, new, message, ops);
            }
        }
        "stop" => {
            changed = id.is_some_and(|one| stop_by_id(service, one));
            if let Some(one) = id.filter(|_| changed) {
                ops.push(Op::log(format!("timed: stopped {one}")));
            }
        }
        "pause" => {
            changed = id.is_some_and(|one| pause_by_id(ctx, service, one));
            if let Some(one) = id.filter(|_| changed) {
                ops.push(Op::log(format!("timed: paused {one}")));
            }
        }
        "resume" => {
            changed = id.is_some_and(|one| resume_by_id(ctx, service, one));
            if let Some(one) = id.filter(|_| changed) {
                ops.push(Op::log(format!("timed: resumed {one}")));
            }
        }
        "stop-all" => {
            changed = stop_all(service);
            if changed {
                ops.push(Op::log("timed: stopped all"));
            }
        }
        _ => {}
    }

    changed
}

const fn state_name(timer: &Timer) -> &'static str {
    if timer.paused { "paused" } else { "running" }
}

fn ipc(ctx: &Context, service: &mut Service, ops: &mut Vec<Op>) -> bool {
    let event = ctx.payload_str("event").unwrap_or_default();
    let payload = ctx.payload_str("payload").unwrap_or_default();

    match event.as_str() {
        "start" => {
            let Some((seconds, message)) = duration::split(&payload) else {
                return false;
            };
            service.ipc_seq = service.ipc_seq.saturating_add(1);
            let id = format!("{}-{}", ctx.now_ms, service.ipc_seq);
            start_timer(ctx, service, seconds, &id, message, ops)
        }
        "stop-all" => {
            let changed = stop_all(service);
            if changed {
                ops.push(Op::log("timed: stopped all"));
            }
            changed
        }
        "stop" => {
            let changed = stop_by_id(service, &payload);
            if changed {
                ops.push(Op::log(format!("timed: stopped {payload}")));
            }
            changed
        }
        "pause" => {
            let changed = pause_by_id(ctx, service, &payload);
            if changed {
                ops.push(Op::log(format!("timed: paused {payload}")));
            }
            changed
        }
        "resume" => {
            let changed = resume_by_id(ctx, service, &payload);
            if changed {
                ops.push(Op::log(format!("timed: resumed {payload}")));
            }
            changed
        }
        "list" => {
            ops.push(Op::log(format!(
                "timed: {} timer(s)",
                service.timers.len()
            )));
            let now = ctx.now_ms;
            for timer in &service.timers {
                let remaining = quotient(timer.remaining_ms(now), 1_000);
                let state = state_name(timer);
                ops.push(Op::log(format!(
                    "timed: {} {} {} left",
                    timer.id,
                    state,
                    duration::clock(remaining)
                )));
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_rejects_incomplete_entries() {
        let complete = serde_json::json!({
            "id": "one",
            "seconds": 60,
            "label": "1m",
            "paused": false,
            "endsAt": 60_000
        });
        assert!(normalize(&complete).is_some());

        let paused = serde_json::json!({
            "id": "one",
            "seconds": 60,
            "label": "1m",
            "paused": true,
            "endsAt": 60_000
        });
        assert!(normalize(&paused).is_none());

        let untyped = serde_json::json!({
            "id": 1,
            "seconds": 60,
            "label": "1m",
            "paused": false,
            "endsAt": 60_000
        });
        assert!(normalize(&untyped).is_none());
    }

    #[test]
    fn command_seconds_floors_and_rejects_bad_values() {
        let seconds = |value: Value| {
            command_seconds(&serde_json::json!({ "seconds": value }))
        };
        assert_eq!(seconds(serde_json::json!(65.9)), Some(65));
        assert_eq!(seconds(serde_json::json!("65")), Some(65));
        assert_eq!(seconds(serde_json::json!("nope")), None);
        assert_eq!(seconds(serde_json::json!(null)), None);
    }
}
