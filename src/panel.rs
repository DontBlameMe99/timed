//! Timed panel: list timers, pause or resume them, stop them, and start new
//! ones.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::mem;
use std::sync::{Mutex, MutexGuard, PoisonError};

use noctalia_rs::{Context, Op, Props, Result, ui};
use serde_json::{Map, Value, json};

use crate::duration;
use crate::timer::{self, Timer};
use crate::urgency::{self, Colors};

/// The progress bar is drawn in `1 / PANEL_STEPS` of the timer. At the panel
/// width that is under a pixel per step, so most ticks draw the same picture.
const PANEL_STEPS: u16 = 400;

struct Panel {
    open: bool,
    new_text: String,
    input_key: i64,
    error: Option<String>,
    signature: Option<u64>,
    memo: PanelMemo,
}

#[derive(Default)]
struct PanelMemo {
    valid: bool,
    state: Option<Value>,
    colors: Option<Colors>,
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
            signature: None,
            memo: PanelMemo {
                valid: false,
                state: None,
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

fn lock() -> MutexGuard<'static, Panel> {
    PANEL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Skips a render whose rows would draw exactly the same tree. The state,
/// theme, input box, and the time each row displays are the only inputs the
/// panel uses, so a cheap token list can decide before the tree is built.
fn memo_hit(ctx: &Context, panel: &mut Panel, colors: &Colors) -> bool {
    let memo = &mut panel.memo;
    if ctx.event != "render" || !panel.open {
        memo.valid = false;
        return false;
    }

    let state = ctx.state.get("timers");
    let state_changed = memo.state.as_ref() != state;
    if state_changed {
        memo.state = state.cloned();
        memo.timers = timer::list(ctx);
    }
    let colors_changed = memo.colors.as_ref() != Some(colors);
    if colors_changed {
        memo.colors = Some(colors.clone());
    }
    let time_format = ctx.time_format().unwrap_or_default();
    let date_format = ctx.date_format().unwrap_or_default();
    let formats_changed = memo.time_format.as_deref()
        != Some(time_format.as_str())
        || memo.date_format.as_deref() != Some(date_format.as_str());
    if formats_changed {
        memo.time_format = Some(time_format.clone());
        memo.date_format = Some(date_format.clone());
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

    let now = ctx.now_ms;
    memo.scratch.clear();
    for timer in &memo.timers {
        memo.scratch
            .push(u64::from(timer.fraction_step(now, PANEL_STEPS)));
        memo.scratch
            .push(duration::approx_token(timer.remaining_seconds(now)));
        let state = match urgency::state(timer, now) {
            "paused" => 0,
            "urgent" => 1,
            "warning" => 2,
            _ => 3,
        };
        memo.scratch.push(state);
        let format = if timer.remaining_seconds(now) >= 86_400 {
            &date_format
        } else {
            &time_format
        };
        let mut hasher = DefaultHasher::new();
        timer::format_time(
            ctx,
            format,
            timer.ends_at.unwrap_or(0).div_euclid(1_000),
        )
        .hash(&mut hasher);
        memo.scratch.push(hasher.finish());
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
    let mut panel = lock();

    match ctx.event.as_str() {
        "open" => {
            panel.open = true;
            panel.new_text.clear();
            panel.error = None;
            panel.input_key = panel.input_key.saturating_add(1);
            panel.signature = None;
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

/// The parts of the render that come from the clock, the theme, and the shell
/// locale rather than from the timer list.
struct View<'a> {
    colors: &'a Colors,
    now: i64,
    time_format: &'a str,
    date_format: &'a str,
}

/// One row's drawn values, shared by the change check and the tree.
struct Row<'a> {
    timer: &'a Timer,
    state: &'static str,
    step: u16,
    time: String,
    role: String,
}

fn collect_rows<'a>(timers: &'a [Timer], view: &View<'_>) -> Vec<Row<'a>> {
    timers
        .iter()
        .map(|timer| Row {
            timer,
            state: urgency::state(timer, view.now),
            step: timer.fraction_step(view.now, PANEL_STEPS),
            time: duration::approx(timer.remaining_seconds(view.now)),
            role: urgency::role(timer, view.now, view.colors),
        })
        .collect()
}

/// A hash of everything the tree draws. The dashboard asks for a render every
/// second, and rebuilding a 55 node tree costs more than checking this.
fn signature(panel: &Panel, view: &View<'_>, rows: &[Row<'_>]) -> u64 {
    let mut hasher = DefaultHasher::new();
    panel.input_key.hash(&mut hasher);
    panel.error.hash(&mut hasher);
    view.time_format.hash(&mut hasher);
    view.date_format.hash(&mut hasher);
    view.colors.safe.hash(&mut hasher);
    view.colors.warning.hash(&mut hasher);
    view.colors.urgent.hash(&mut hasher);
    view.colors.idle.hash(&mut hasher);
    view.colors.use_urgency.hash(&mut hasher);
    rows.len().hash(&mut hasher);
    for row in rows {
        row.timer.id.hash(&mut hasher);
        row.timer.seconds.hash(&mut hasher);
        row.timer.label.hash(&mut hasher);
        row.timer.message.hash(&mut hasher);
        row.timer.paused.hash(&mut hasher);
        row.timer.ends_at.hash(&mut hasher);
        row.state.hash(&mut hasher);
        row.step.hash(&mut hasher);
        row.time.hash(&mut hasher);
        row.role.hash(&mut hasher);
    }
    hasher.finish()
}

fn render(ctx: &Context, panel: &mut Panel) -> Vec<Op> {
    let timers = timer::list(ctx);
    let (colors, mut ops) = urgency::panel_colors(ctx);
    if memo_hit(ctx, panel, &colors) {
        return ops;
    }
    let time_format = ctx.time_format().unwrap_or_default();
    let date_format = ctx.date_format().unwrap_or_default();
    let view = View {
        colors: &colors,
        now: ctx.now_ms,
        time_format: &time_format,
        date_format: &date_format,
    };
    let rows = collect_rows(&timers, &view);

    let signature = signature(panel, &view, &rows);
    if panel.signature == Some(signature) {
        return ops;
    }
    panel.signature = Some(signature);

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
    ops
}

fn header_children(ctx: &Context, count: usize) -> Vec<ui::UiNode> {
    let mut children = vec![ui::row(
        Props::new().int("gap", 8).text("align", "center"),
        vec![ui::label(
            Props::new()
                .text("text", ctx.tr("panel.title", &[]))
                .int("fontSize", 18)
                .text("fontWeight", "bold"),
        )],
    )];
    if count > 1 {
        children.push(ui::button(
            Props::new()
                .text("text", ctx.tr("panel.stop_all", &[]))
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
                    .text("text", ctx.tr("panel.empty", &[]))
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
    let state = row.state;
    let time_color = if state == "paused" {
        Some("on_surface_variant".to_owned())
    } else if view.colors.use_urgency
        && (state == "warning" || state == "urgent")
    {
        Some(row.role.clone())
    } else {
        None
    };
    let mut time = Props::new()
        .text("text", row.time.clone())
        .int("fontSize", 18)
        .text("fontWeight", "bold");
    if let Some(color) = time_color {
        time = time.text("color", color);
    }

    let mut children = vec![title_column(ctx, timer, view), ui::label(time)];
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

fn title_column(ctx: &Context, timer: &Timer, view: &View<'_>) -> ui::UiNode {
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
                    .text("text", subtitle(ctx, timer, view))
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
        ("player-play", ctx.tr("panel.resume", &[]))
    } else {
        ("player-pause", ctx.tr("panel.pause", &[]))
    };
    vec![
        action_button(glyph, tip, &format!("toggle:{}", timer.id)),
        action_button(
            "player-stop",
            ctx.tr("panel.stop", &[]),
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
        ctx.tr("panel.timer", &[("duration", timer.label.clone())])
    })
}

fn subtitle(ctx: &Context, timer: &Timer, view: &View<'_>) -> String {
    if timer.paused {
        return ctx.tr("panel.paused_left", &[]);
    }
    let remaining = timer.remaining_seconds(view.now);
    let format = if remaining >= 86_400 {
        view.date_format
    } else {
        view.time_format
    };
    let time = timer.ends_at.map_or_else(String::new, |end| {
        timer::format_time(ctx, format, end.div_euclid(1_000))
    });
    ctx.tr("panel.ends_at", &[("time", time)])
}

fn controls_row(ctx: &Context, panel: &Panel) -> ui::UiNode {
    ui::row(
        Props::new().int("gap", 8).text("align", "center"),
        vec![
            ui::input(
                Props::new()
                    .text("key", format!("new-{}", panel.input_key))
                    .text("value", panel.new_text.clone())
                    .text("placeholder", ctx.tr("panel.placeholder", &[]))
                    .int("flexGrow", 1)
                    .bool("focus", true)
                    .call("onChange", "draft")
                    .call("onSubmit", "submit"),
            ),
            ui::button(
                Props::new()
                    .text("text", ctx.tr("panel.start", &[]))
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
        panel.error = Some(ctx.tr("panel.invalid", &[]));
        return render(ctx, panel);
    };

    let mut command = Map::new();
    command.insert("action".to_owned(), json!("start"));
    command.insert("seconds".to_owned(), json!(seconds));
    if let Some(body) = message {
        command.insert("message".to_owned(), json!(body));
    }
    let mut ops = vec![timer::send(ctx, Value::Object(command))];

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
        let colors = Colors::defaults();
        let view = View {
            colors: &colors,
            now: 1_699_910_000_000,
            time_format: "%H:%M",
            date_format: "%Y-%m-%d",
        };
        let subtitle = subtitle(&ctx, &timer, &view);
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

        let changed =
            test_context::context("render", json!({}), timers("changed"), host);
        assert!(
            render(&changed, &mut panel)
                .iter()
                .any(|op| serde_json::to_value(op).expect("op")["op"]
                    == json!("panel_render"))
        );
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
