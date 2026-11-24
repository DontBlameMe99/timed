//! Timed launcher provider.
//!
//!   /timer 1h5      start a 1 hour 5 minute timer
//!   /timer 1s4h     units may appear in any order
//!   /timer 3h2s message   use the message in the notification
//!   /timer          show how to start a new timer
//!   /timer pause    list running timers to pause
//!   /timer resume   list paused timers to resume
//!   /timer stop     list timers to stop
//!   /timer stop all stop every timer

use std::mem;
use std::sync::Mutex;

use noctalia_rs::{Context, Op, Result};
use serde_json::{Value, json};

use crate::duration;
use crate::i18n::tr;
use crate::sync::lock;
use crate::timer::{self, Clock, Timer};

#[derive(Default)]
struct RowCache {
    query: String,
    revision: Option<i64>,
    state: Option<Value>,
    timers: Vec<Timer>,
    tokens: Vec<(u64, bool)>,
    scratch: Vec<(u64, bool)>,
    time: String,
    date: String,
    rows: Vec<Value>,
}

struct Launcher {
    suppressed: bool,
    cache: Option<RowCache>,
}

impl Launcher {
    const fn new() -> Self {
        Self {
            suppressed: false,
            cache: None,
        }
    }
}

static LAUNCHER: Mutex<Launcher> = Mutex::new(Launcher::new());

/// Handles one launcher event.
///
/// # Errors
/// The signature matches the entry contract; this handler does not fail.
pub fn update(ctx: &Context) -> Result<Vec<Op>> {
    match ctx.event.as_str() {
        "query" => {
            let text = ctx.payload_str("text").unwrap_or_default();
            let source = ctx.payload_str("source").unwrap_or_default();
            let mut launcher = lock(&LAUNCHER);
            let suppressed = launcher.suppressed;
            launcher.suppressed = false;
            if source == "state" && suppressed {
                return Ok(Vec::new());
            }
            let rows = rows(ctx, &text, &mut launcher.cache);
            Ok(vec![Op::LauncherResults { text, rows }])
        }
        "activate" => {
            let id = ctx.payload_str("id").unwrap_or_default();
            let ops = activate(ctx, &id);
            // The original clears its query after sending a command so the
            // following state change does not redraw stale rows.
            lock(&LAUNCHER).suppressed = !ops.is_empty();
            Ok(ops)
        }
        _ => Ok(Vec::new()),
    }
}

struct Action<'a> {
    name: &'a str,
    timer: &'a Timer,
}

fn title(ctx: &Context, timer: &Timer) -> String {
    timer.message.clone().unwrap_or_else(|| {
        tr(ctx, "launcher.timer", &[("duration", timer.label.clone())])
    })
}

fn count_text(ctx: &Context, count: usize) -> String {
    if count == 1 {
        return tr(ctx, "launcher.one_timer", &[]);
    }
    tr(
        ctx,
        "launcher.active_count",
        &[("count", count.to_string())],
    )
}

fn empty_row(ctx: &Context) -> Value {
    json!({
        "id": "empty",
        "title": tr(ctx, "launcher.no_matching", &[]),
        "subtitle": tr(ctx, "launcher.hint", &[]),
        "glyph": "clock",
    })
}

fn start_hint_row(ctx: &Context) -> Value {
    json!({
        "id": "new",
        "title": tr(ctx, "launcher.start_new", &[]),
        "subtitle": tr(ctx, "launcher.hint", &[]),
        "glyph": "plus",
        "query": " ",
    })
}

fn start_row(ctx: &Context, query: &str) -> Value {
    let Some((seconds, message)) = duration::split(query) else {
        return json!({
            "id": "invalid",
            "title": tr(ctx, "launcher.invalid", &[]),
            "subtitle": tr(ctx, "launcher.hint", &[]),
            "glyph": "alert-triangle",
        });
    };
    let label = duration::human(seconds);
    let subtitle = message.as_ref().map_or_else(
        || label.clone(),
        |text| {
            tr(
                ctx,
                "launcher.message",
                &[("duration", label.clone()), ("message", text.clone())],
            )
        },
    );
    json!({
        "id": format!("start:{seconds}:{}", message.unwrap_or_default()),
        "title": tr(ctx, "launcher.start", &[("time", label)]),
        "subtitle": subtitle,
        "glyph": "clock",
    })
}

fn action_row(ctx: &Context, action: &Action<'_>, clock: &Clock) -> Value {
    let timer = action.timer;
    let remaining = timer.remaining_seconds(clock.now);
    let subtitle = if timer.paused {
        tr(
            ctx,
            "launcher.paused_at",
            &[("duration", duration::approx(remaining))],
        )
    } else {
        tr(
            ctx,
            "launcher.ends_at",
            &[
                ("time", timer.ends_at_text(ctx, clock)),
                ("duration", duration::approx(remaining)),
            ],
        )
    };
    let glyph = match action.name {
        "pause" => "player-pause",
        "resume" => "player-play",
        "stop" | "stop-all" => "player-stop",
        _ => "clock",
    };
    json!({
        "id": format!("{}:{}", action.name, timer.id),
        "title": format!("{} {}", tr(ctx, &format!("launcher.{}", action.name), &[]), title(ctx, timer)),
        "subtitle": subtitle,
        "glyph": glyph,
    })
}

fn tokens_into(timers: &[Timer], now: i64, tokens: &mut Vec<(u64, bool)>) {
    tokens.clear();
    for timer in timers {
        let remaining = timer.remaining_ms(now);
        let long =
            remaining > duration::DAY.saturating_sub(1).saturating_mul(1_000);
        tokens.push((duration::approx_token_ms(remaining), long));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the context, action, filter, and row cache"
)]
fn rows_for(
    ctx: &Context,
    name: &str,
    paused: Option<bool>,
    cache: &mut Option<RowCache>,
) -> Vec<Value> {
    let clock = Clock::read(ctx);
    let revision = timer::state_revision(ctx);
    let state = ctx.state.get("timers");
    if let Some(hit) = cache.as_mut()
        && hit.query == name
        && match (hit.revision, revision) {
            (Some(previous), Some(current)) => previous == current,
            _ => hit.state.as_ref() == state,
        }
        && hit.time == clock.time
        && hit.date == clock.date
    {
        tokens_into(&hit.timers, clock.now, &mut hit.scratch);
        if hit.scratch == hit.tokens {
            return hit.rows.clone();
        }
        mem::swap(&mut hit.tokens, &mut hit.scratch);
    }

    let timers = timer::list(ctx);
    let mut tokens = Vec::with_capacity(timers.len());
    tokens_into(&timers, clock.now, &mut tokens);
    let rows: Vec<Value> = timers
        .iter()
        .filter(|timer| paused.is_none_or(|want| timer.paused == want))
        .map(|timer| action_row(ctx, &Action { name, timer }, &clock))
        .collect();
    if rows.is_empty() {
        return vec![empty_row(ctx)];
    }
    *cache = Some(RowCache {
        query: name.to_owned(),
        revision,
        state: state.cloned(),
        timers,
        tokens,
        scratch: Vec::new(),
        time: clock.time,
        date: clock.date,
        rows: rows.clone(),
    });
    rows
}

fn rows(ctx: &Context, text: &str, cache: &mut Option<RowCache>) -> Vec<Value> {
    let query = text.trim_ascii();
    match query {
        "" => vec![start_hint_row(ctx)],
        "pause" => rows_for(ctx, "pause", Some(false), cache),
        "resume" => rows_for(ctx, "resume", Some(true), cache),
        "stop" => rows_for(ctx, "stop", None, cache),
        "stop all" => {
            let total = timer::list(ctx).len();
            if total == 0 {
                vec![empty_row(ctx)]
            } else {
                vec![json!({
                    "id": "stop-all",
                    "title": tr(ctx, "launcher.stop_all", &[]),
                    "subtitle": count_text(ctx, total),
                    "glyph": "player-stop",
                })]
            }
        }
        _ => vec![start_row(ctx, query)],
    }
}

fn activate(ctx: &Context, id: &str) -> Vec<Op> {
    if id == "new" || id == "empty" {
        return Vec::new();
    }
    if id == "stop-all" {
        return vec![timer::send(ctx, json!({ "action": "stop-all" }))];
    }

    let Some((action, rest)) = id.split_once(':') else {
        return Vec::new();
    };
    match action {
        "pause" | "resume" | "stop" => {
            vec![timer::send(ctx, json!({ "action": action, "id": rest }))]
        }
        "start" => {
            let Some((raw_seconds, message)) = rest.split_once(':') else {
                return Vec::new();
            };
            let Ok(seconds) = raw_seconds.parse::<i64>() else {
                return Vec::new();
            };
            let body = (!message.is_empty()).then_some(message);
            vec![timer::start_command(ctx, seconds, body)]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_context;

    #[test]
    fn start_row_reports_duration_and_message() {
        let ctx =
            test_context::context("query", json!({}), json!({}), json!({}));
        let row = start_row(&ctx, "1h5 wake up");
        assert_eq!(row["id"], json!("start:3900:wake up"));
        assert_eq!(row["title"], json!("Start timer 1h 5m"));
        assert_eq!(row["subtitle"], json!("1h 5m - wake up"));
    }

    #[test]
    fn start_row_reports_invalid_input() {
        let ctx =
            test_context::context("query", json!({}), json!({}), json!({}));
        let row = start_row(&ctx, "nope");
        assert_eq!(row["id"], json!("invalid"));
    }

    #[test]
    fn rows_cover_every_query() {
        let mut launcher = Launcher::new();
        let ctx =
            test_context::context("query", json!({}), json!({}), json!({}));
        assert_eq!(rows(&ctx, "", &mut launcher.cache)[0]["id"], json!("new"));
        assert_eq!(
            rows(&ctx, "pause", &mut launcher.cache)[0]["id"],
            json!("empty")
        );
        assert_eq!(
            rows(&ctx, "resume", &mut launcher.cache)[0]["id"],
            json!("empty")
        );
        assert_eq!(
            rows(&ctx, "stop", &mut launcher.cache)[0]["id"],
            json!("empty")
        );
        assert_eq!(
            rows(&ctx, "stop all", &mut launcher.cache)[0]["id"],
            json!("empty")
        );
        assert_eq!(
            rows(&ctx, "5", &mut launcher.cache)[0]["id"],
            json!("start:300:")
        );
    }
    #[test]
    fn cache_tracks_the_values_the_rows_show() {
        let now: i64 = 1_800_000_000_000;
        let timers = |label: &str| {
            json!({ "timers": [{
                "id": "one",
                "seconds": 600,
                "label": label,
                "paused": false,
                "endsAt": now + 600_000,
            }] })
        };
        let host = json!({
            "now_ms": now,
            "setting": {
                "shell.time_format": "%H:%M",
                "shell.date_format": "%Y-%m-%d",
            },
        });
        let mut ctx =
            test_context::context("query", json!({}), timers("10m"), host);
        let mut launcher = Launcher::new();

        let first = rows(&ctx, "pause", &mut launcher.cache);
        ctx.now_ms = now.saturating_add(1_000);
        assert_eq!(rows(&ctx, "pause", &mut launcher.cache), first);

        ctx.now_ms = now.saturating_add(31_000);
        let later = rows(&ctx, "pause", &mut launcher.cache);
        assert_ne!(later, first);
        assert!(serde_json::to_string(&later).expect("row").contains("9m"));

        ctx.now_ms = now;
        ctx.state = timers("changed");
        let changed = rows(&ctx, "pause", &mut launcher.cache);
        assert_ne!(changed, first);
        assert!(
            serde_json::to_string(&changed)
                .expect("row")
                .contains("changed")
        );
    }
}
