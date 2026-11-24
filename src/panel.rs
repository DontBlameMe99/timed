//! Timed panel: list timers, pause or resume them, stop them, and start new
//! ones.

use std::mem;
use std::sync::Arc;
use std::sync::Mutex;

use noctalia_rs::{Context, Op, Props, Result, ui};
use serde_json::{Value, json};

use crate::duration;
use crate::i18n::tr;
use crate::sync::lock;
use crate::timer::{self, Clock, Timer};
use crate::urgency::{self, Colors, State};

/// The progress bar is drawn in `1 / PANEL_STEPS` of the timer. At the panel
/// width that is under a pixel per step, so most ticks draw the same picture.
const PANEL_STEPS: u16 = 400;

struct Panel {
    open: bool,
    new_text: String,
    input_key: i64,
    error: Option<String>,
    memo: PanelMemo,
}

#[derive(Default)]
struct PanelMemo {
    valid: bool,
    state: Option<Value>,
    revision: Option<i64>,
    colors: Option<Arc<Colors>>,
    time_format: Option<String>,
    date_format: Option<String>,
    input_key: i64,
    error: Option<String>,
    timers: Vec<Timer>,
    tokens: Vec<u64>,
    scratch: Vec<u64>,
}

impl Panel {
    const fn new() -> Self {
        Self {
            open: false,
            new_text: String::new(),
            input_key: 0,
            error: None,
            memo: PanelMemo {
                valid: false,
                state: None,
                revision: None,
                colors: None,
                time_format: None,
                date_format: None,
                input_key: 0,
                error: None,
                timers: Vec::new(),
                tokens: Vec::new(),
                scratch: Vec::new(),
            },
        }
    }
}

static PANEL: Mutex<Panel> = Mutex::new(Panel::new());

/// Skips a render whose rows would draw exactly the same tree. The state,
/// theme, input box, and the time each row displays are the only inputs the
/// panel uses, so a cheap token list can decide before the tree is built.
fn memo_hit(ctx: &Context, panel: &mut Panel, view: &View<'_>) -> bool {
    let clock = view.clock;
    let memo = &mut panel.memo;
    let state = ctx.state.get("timers");
    let revision = timer::state_revision(ctx);
    let state_changed = match (memo.revision, revision) {
        (Some(previous), Some(current)) => previous != current,
        _ => memo.state.as_ref() != state,
    };
    if state_changed {
        memo.revision = revision;
        memo.state = state.cloned();
        memo.timers = timer::list(ctx);
    }
    if ctx.event != "render" || !panel.open {
        memo.valid = false;
        return false;
    }

    let colors_changed = memo.colors.as_ref() != Some(view.colors);
    if colors_changed {
        memo.colors = Some(Arc::clone(view.colors));
    }
    let formats_changed = memo.time_format.as_deref()
        != Some(clock.time.as_str())
        || memo.date_format.as_deref() != Some(clock.date.as_str());
    if formats_changed {
        memo.time_format = Some(clock.time.clone());
        memo.date_format = Some(clock.date.clone());
    }
    let controls_changed =
        memo.input_key != panel.input_key || memo.error != panel.error;
    if controls_changed {
        memo.input_key = panel.input_key;
        memo.error = panel.error.clone();
    }
    if state_changed || colors_changed || formats_changed || controls_changed {
        memo.valid = false;
    }

    let now = clock.now;
    memo.scratch.clear();
    for timer in &memo.timers {
        let remaining = timer.remaining_ms(now);
        let state = State::of_remaining(timer, remaining).token();
        let long = u64::from(
            remaining > duration::DAY.saturating_sub(1).saturating_mul(1_000),
        );
        memo.scratch.push(
            u64::from(timer.fraction_step_ms(remaining, PANEL_STEPS))
                | ((state | (long << 2)) << 16),
        );
        memo.scratch.push(duration::approx_token_ms(remaining));
    }

    if memo.valid && memo.scratch == memo.tokens {
        return true;
    }
    mem::swap(&mut memo.scratch, &mut memo.tokens);
    memo.valid = true;
    false
}

/// Handles one panel event.
///
/// # Errors
/// The signature matches the entry contract; this handler does not fail.
pub fn update(ctx: &Context) -> Result<Vec<Op>> {
    let mut panel = lock(&PANEL);

    match ctx.event.as_str() {
        "open" => {
            panel.open = true;
            panel.new_text.clear();
            panel.error = None;
            panel.input_key = panel.input_key.saturating_add(1);
            Ok(render(ctx, &mut panel))
        }
        "close" => {
            panel.open = false;
            Ok(Vec::new())
        }
        "action" => Ok(action(ctx, &mut panel)),
        "host_callback" => {
            if ctx.payload_str("id").as_deref() == Some("colors") {
                urgency::store_colors(ctx.payload_arg(0));
            }
            Ok(Vec::new())
        }
        _ => {
            if panel.open {
                Ok(render(ctx, &mut panel))
            } else {
                Ok(Vec::new())
            }
        }
    }
}

/// The parts of the render that come from the clock and the theme rather than
/// from the timer list.
struct View<'a> {
    colors: &'a Arc<Colors>,
    clock: &'a Clock,
}

/// One row's drawn values, shared by the change check and the tree.
struct Row<'a> {
    timer: &'a Timer,
    state: State,
    step: u16,
    time: String,
    role: String,
}

fn collect_rows<'a>(timers: &'a [Timer], view: &View<'_>) -> Vec<Row<'a>> {
    let now = view.clock.now;
    timers
        .iter()
        .map(|timer| {
            let remaining = timer.remaining_ms(now);
            let state = State::of_remaining(timer, remaining);
            Row {
                timer,
                state,
                step: timer.fraction_step_ms(remaining, PANEL_STEPS),
                time: duration::approx(timer.remaining_seconds(now)),
                role: state.role(view.colors),
            }
        })
        .collect()
}

fn render(ctx: &Context, panel: &mut Panel) -> Vec<Op> {
    let (colors, mut ops) = urgency::panel_colors(ctx);
    let clock = Clock::read(ctx);
    let view = View {
        colors: &colors,
        clock: &clock,
    };
    if memo_hit(ctx, panel, &view) {
        return ops;
    }
    let timers = mem::take(&mut panel.memo.timers);
    let rows = collect_rows(&timers, &view);
    let header = ui::row(
        Props::new()
            .text("align", "center")
            .text("justify", "space_between")
            .int("gap", 8),
        header_children(ctx, timers.len()),
    );
    let mut start = vec![controls_row(ctx, panel)];
    if let Some(error) = &panel.error {
        start.push(ui::label(
            Props::new()
                .text("text", error.clone())
                .text("color", "error")
                .int("fontSize", 12),
        ));
    }
    let controls = ui::column(Props::new().int("gap", 6), start);
    let body = body(ctx, &rows, &view);

    ops.push(Op::PanelRender {
        tree: ui::column(
            Props::new().int("gap", 12).int("flexGrow", 1),
            vec![header, body, controls],
        ),
    });
    panel.memo.timers = timers;
    ops
}

fn header_children(ctx: &Context, count: usize) -> Vec<ui::UiNode> {
    let mut children = vec![ui::row(
        Props::new().int("gap", 8).text("align", "center"),
        vec![ui::label(
            Props::new()
                .text("text", tr(ctx, "panel.title", &[]))
                .int("fontSize", 18)
                .text("fontWeight", "bold"),
        )],
    )];
    if count > 1 {
        children.push(ui::button(
            Props::new()
                .text("text", tr(ctx, "panel.stop_all", &[]))
                .text("variant", "ghost")
                .text("controlSize", "sm")
                .call("onClick", "stop_all"),
        ));
    }
    children
}

fn body(ctx: &Context, rows: &[Row<'_>], view: &View<'_>) -> ui::UiNode {
    if rows.is_empty() {
        return ui::column(
            Props::new()
                .int("flexGrow", 1)
                .text("justify", "center")
                .text("align", "center")
                .int("gap", 6),
            vec![ui::label(
                Props::new()
                    .text("text", tr(ctx, "panel.empty", &[]))
                    .text("color", "on_surface_variant"),
            )],
        );
    }

    let children = rows.iter().map(|row| timer_row(ctx, row, view)).collect();
    ui::scroll(Props::new().int("flexGrow", 1).int("gap", 8), children)
}

fn timer_row(ctx: &Context, row: &Row<'_>, view: &View<'_>) -> ui::UiNode {
    let timer = row.timer;
    let paused = timer.paused;
    let time_color = row.state.text_role(view.colors);
    let mut time = Props::new()
        .text("text", row.time.clone())
        .int("fontSize", 18)
        .text("fontWeight", "bold");
    if let Some(color) = time_color {
        time = time.text("color", color);
    }

    let mut children =
        vec![title_column(ctx, timer, view.clock), ui::label(time)];
    children.extend(timer_buttons(ctx, timer, paused));
    ui::column(
        Props::new()
            .text("key", timer.id.clone())
            .int("gap", 8)
            .int("padding", 10)
            .text("fill", "surface_container/0.35")
            .int("radius", 10),
        vec![
            ui::row(
                Props::new().int("gap", 10).text("align", "center"),
                children,
            ),
            ui::progress(
                Props::new()
                    .int("height", 3)
                    .int("radius", 2)
                    .num(
                        "progress",
                        f64::from(row.step) / f64::from(PANEL_STEPS),
                    )
                    .text("fill", row.role.clone())
                    .text("track", "outline_variant"),
            ),
        ],
    )
}

fn title_column(ctx: &Context, timer: &Timer, clock: &Clock) -> ui::UiNode {
    ui::column(
        Props::new().int("gap", 2).int("flexGrow", 1),
        vec![
            ui::label(
                Props::new()
                    .text("text", title(ctx, timer))
                    .text("fontWeight", "semibold")
                    .int("maxLines", 1),
            ),
            ui::label(
                Props::new()
                    .text("text", subtitle(ctx, timer, clock))
                    .int("fontSize", 12)
                    .text("color", "on_surface_variant")
                    .int("maxLines", 1),
            ),
        ],
    )
}

fn timer_buttons(
    ctx: &Context,
    timer: &Timer,
    paused: bool,
) -> Vec<ui::UiNode> {
    let (glyph, tip) = if paused {
        ("player-play", tr(ctx, "panel.resume", &[]))
    } else {
        ("player-pause", tr(ctx, "panel.pause", &[]))
    };
    vec![
        action_button(glyph, tip, &format!("toggle:{}", timer.id)),
        action_button(
            "player-stop",
            tr(ctx, "panel.stop", &[]),
            &format!("stop:{}", timer.id),
        ),
    ]
}

fn action_button(
    glyph: &'static str,
    tooltip: String,
    action: &str,
) -> ui::UiNode {
    ui::button(
        Props::new()
            .text("glyph", glyph)
            .text("variant", "ghost")
            .text("controlSize", "sm")
            .text("tooltip", tooltip)
            .json("onClick", ui::call(action)),
    )
}

fn title(ctx: &Context, timer: &Timer) -> String {
    timer.message.clone().unwrap_or_else(|| {
        tr(ctx, "panel.timer", &[("duration", timer.label.clone())])
    })
}

fn subtitle(ctx: &Context, timer: &Timer, clock: &Clock) -> String {
    if timer.paused {
        return tr(ctx, "panel.paused_left", &[]);
    }
    tr(
        ctx,
        "panel.ends_at",
        &[("time", timer.ends_at_text(ctx, clock))],
    )
}

fn controls_row(ctx: &Context, panel: &Panel) -> ui::UiNode {
    ui::row(
        Props::new().int("gap", 8).text("align", "center"),
        vec![
            ui::input(
                Props::new()
                    .text("key", format!("new-{}", panel.input_key))
                    .text("value", panel.new_text.clone())
                    .text("placeholder", tr(ctx, "panel.placeholder", &[]))
                    .int("flexGrow", 1)
                    .bool("focus", true)
                    .call("onChange", "draft")
                    .call("onSubmit", "submit"),
            ),
            ui::button(
                Props::new()
                    .text("text", tr(ctx, "panel.start", &[]))
                    .text("variant", "primary")
                    .call("onClick", "start"),
            ),
        ],
    )
}

fn action(ctx: &Context, panel: &mut Panel) -> Vec<Op> {
    let name = ctx.payload_str("name").unwrap_or_default();

    if let Some(id) = name.strip_prefix("toggle:") {
        let timers = timer::list(ctx);
        let timer = timers.iter().find(|timer| timer.id == id);
        return timer::toggle(ctx, timer).into_iter().collect();
    }
    if let Some(id) = name.strip_prefix("stop:") {
        return vec![timer::send(ctx, json!({ "action": "stop", "id": id }))];
    }

    match name.as_str() {
        "stop_all" => vec![timer::send(ctx, json!({ "action": "stop-all" }))],
        "draft" => {
            panel.new_text = argument(ctx, 0);
            if panel.error.take().is_some() {
                render(ctx, panel)
            } else {
                Vec::new()
            }
        }
        "start" => {
            let text = panel.new_text.clone();
            start(ctx, panel, &text)
        }
        "submit" => {
            let text = argument(ctx, 0);
            start(ctx, panel, &text)
        }
        _ => Vec::new(),
    }
}

fn argument(ctx: &Context, index: usize) -> String {
    match ctx.payload_arg(index) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(
            Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null,
        )
        | None => String::new(),
    }
}

fn start(ctx: &Context, panel: &mut Panel, text: &str) -> Vec<Op> {
    let Some((seconds, message)) = duration::split(text) else {
        panel.error = Some(tr(ctx, "panel.invalid", &[]));
        return render(ctx, panel);
    };

    let mut ops = vec![timer::start_command(ctx, seconds, message.as_deref())];
    panel.new_text.clear();
    panel.error = None;
    panel.input_key = panel.input_key.saturating_add(1);
    ops.extend(render(ctx, panel));
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_context;

    #[test]
    fn subtitle_uses_date_format_for_long_timers() {
        let ctx = test_context::context(
            "render",
            json!({}),
            json!({ "timers": [] }),
            json!({}),
        );
        let timer = Timer {
            id: "one".to_owned(),
            seconds: 90_000,
            label: "1d 1h".to_owned(),
            message: None,
            paused: false,
            ends_at: Some(1_700_000_000_000),
            remaining_ms: None,
        };
        let clock = Clock {
            now: 1_699_910_000_000,
            time: "%H:%M".to_owned(),
            date: "%Y-%m-%d".to_owned(),
        };
        let subtitle = subtitle(&ctx, &timer, &clock);
        assert!(subtitle.contains("Ends at"));
        assert!(subtitle.contains("2023"), "{subtitle}");
    }

    #[test]
    fn memo_skips_an_identical_render_and_wakes_on_state_change() {
        let now: i64 = 1_800_000_000_000;
        urgency::store_colors(Some(&json!({ "useUrgency": true })));
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
        let mut panel = Panel::new();
        panel.open = true;
        panel.input_key = 1;

        let context = test_context::context(
            "render",
            json!({}),
            timers("10m"),
            host.clone(),
        );
        assert!(
            render(&context, &mut panel)
                .iter()
                .any(|op| serde_json::to_value(op).expect("op")["op"]
                    == json!("panel_render"))
        );
        assert!(render(&context, &mut panel).is_empty());

        let mut changed_host = host;
        changed_host["timed_state_version"] = json!(1);
        let changed = test_context::context(
            "render",
            json!({}),
            timers("changed"),
            changed_host,
        );
        assert!(
            render(&changed, &mut panel)
                .iter()
                .any(|op| serde_json::to_value(op).expect("op")["op"]
                    == json!("panel_render"))
        );
    }
    #[test]
    fn memo_wakes_when_the_end_time_switches_to_a_date() {
        let now: i64 = 1_800_000_000_000;
        let end = now
            .saturating_add(
                duration::DAY.saturating_sub(1).saturating_mul(1_000),
            )
            .saturating_add(500);
        let state = json!({ "timers": [{
            "id": "one",
            "seconds": 90_000,
            "label": "long",
            "paused": false,
            "endsAt": end,
        }] });
        let host = json!({
            "now_ms": now,
            "timed_state_version": 0,
            "setting": {
                "shell.time_format": "%H:%M",
                "shell.date_format": "%Y-%m-%d",
            },
        });
        let ctx = test_context::context("render", json!({}), state, host);
        let clock = Clock {
            now,
            time: "%H:%M".to_owned(),
            date: "%Y-%m-%d".to_owned(),
        };
        let colors = Arc::new(Colors::defaults());
        let view = View {
            colors: &colors,
            clock: &clock,
        };
        let mut panel = Panel::new();
        panel.open = true;
        assert!(!memo_hit(&ctx, &mut panel, &view));
        assert!(memo_hit(&ctx, &mut panel, &view));

        let mut later = ctx;
        later.now_ms = now.saturating_add(1_000);
        let later_clock = Clock {
            now: later.now_ms,
            ..clock
        };
        let view = View {
            colors: &colors,
            clock: &later_clock,
        };
        assert!(!memo_hit(&later, &mut panel, &view));
    }

    #[test]
    fn memo_wakes_when_the_rounded_countdown_changes() {
        let now: i64 = 1_800_000_000_000;
        urgency::store_colors(Some(&json!({ "useUrgency": true })));
        let state = json!({ "timers": [{
            "id": "one",
            "seconds": 100_000,
            "label": "long",
            "paused": false,
            "endsAt": now + 48_610_000,
        }] });
        let host = json!({
            "now_ms": now,
            "setting": {
                "shell.time_format": "%H:%M",
                "shell.date_format": "%Y-%m-%d",
            },
        });
        let mut panel = Panel::new();
        panel.open = true;
        panel.input_key = 1;

        let first = test_context::context(
            "render",
            json!({}),
            state.clone(),
            host.clone(),
        );
        let ops = render(&first, &mut panel);
        assert!(ops.iter().any(|op| {
            serde_json::to_value(op)
                .expect("op")
                .to_string()
                .contains("14h")
        }));

        let mut later = test_context::context("render", json!({}), state, host);
        later.now_ms = now.saturating_add(20_000);
        let ops = render(&later, &mut panel);
        assert!(ops.iter().any(|op| {
            serde_json::to_value(op)
                .expect("op")
                .to_string()
                .contains("13h")
        }));
    }
}
