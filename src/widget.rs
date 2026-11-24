//! Timed bar widget. Shows the nearest timer as a progress ring with the timer
//! count inside, plus the estimated remaining time. Left-clicking opens the
//! panel; right-clicking pauses or resumes the nearest timer.

use std::f64::consts::TAU;
use std::fmt::Write;
use std::fs;
use std::mem;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use noctalia_rs::{Context, Op, Props, Result, ui};
use serde_json::{Value, json};

use crate::duration;
use crate::timer::{self, Timer};
use crate::urgency::{self, Colors};

const PANEL_ID: &str = "dontblameme/timed:panel";
const RUNNING_INTERVAL_MS: i64 = 1_000;
const IDLE_INTERVAL_MS: i64 = 60_000;
const HIDDEN_INTERVAL_MS: i64 = 86_400_000;

const RING_SIZE: u32 = 26;
const RING_VIEWBOX: i64 = 40;
const RING_RADIUS: i64 = 16;
const RING_STROKE: i64 = 4;
const RING_RADIUS_F: f64 = 16.0;
/// The arc is redrawn in `1 / RING_STEPS` of the circle. At 26 px that is a
/// third of a pixel, so quantizing saves the file write without a visible
/// change.
const RING_STEPS: u16 = 200;

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent one-time render flags"
)]
struct Widget {
    instance: Option<String>,
    paths: Option<(PathBuf, PathBuf)>,
    published: urgency::Published,
    ring_arc: Option<String>,
    ring_track: Option<String>,
    ring_step: u16,
    ring_count: Option<usize>,
    image_set: bool,
    fallback: bool,
    tooltip: Option<String>,
    text: Option<String>,
    color: Option<String>,
    interval: Option<i64>,
    memo: Memo,
}

#[derive(Default)]
struct Memo {
    valid: bool,
    state: Option<Value>,
    config: Option<Value>,
    colors: Option<Value>,
    bar: Option<Value>,
    setting: Option<Value>,
    timers: Vec<Timer>,
    tokens: Vec<u64>,
    scratch: Vec<u64>,
}

impl Widget {
    const fn new() -> Self {
        Self {
            instance: None,
            paths: None,
            published: urgency::Published::new(),
            ring_arc: None,
            ring_track: None,
            ring_step: 0,
            ring_count: None,
            image_set: false,
            fallback: false,
            tooltip: None,
            text: None,
            color: None,
            interval: None,
            memo: Memo {
                valid: false,
                state: None,
                config: None,
                colors: None,
                bar: None,
                setting: None,
                timers: Vec::new(),
                tokens: Vec::new(),
                scratch: Vec::new(),
            },
        }
    }
}

static WIDGET: Mutex<Widget> = Mutex::new(Widget::new());

fn lock() -> MutexGuard<'static, Widget> {
    WIDGET.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Returns true when the next render would emit the same picture. The cache
/// keys on the inputs the widget draws, so a state or theme change, or a move
/// of the drawn clock or ring, forces a fresh tree.
fn memo_hit(ctx: &Context, widget: &mut Widget) -> bool {
    let memo = &mut widget.memo;
    if ctx.event != "render"
        || ctx.payload_str("source").as_deref() == Some("load")
        || ctx.setting("shell.font_family").is_none()
    {
        memo.valid = false;
        return false;
    }

    let state = ctx.state.get("timers");
    let state_changed = memo.state.as_ref() != state;
    if state_changed {
        memo.state = state.cloned();
        memo.timers = timer::list(ctx);
    }
    let config_changed = memo.config.as_ref() != Some(&ctx.config);
    if config_changed {
        memo.config = Some(ctx.config.clone());
    }
    let colors = ctx.host.get("colors");
    let bar = ctx.host.get("bar");
    let setting = ctx.host.get("setting");
    let host_changed = memo.colors.as_ref() != colors
        || memo.bar.as_ref() != bar
        || memo.setting.as_ref() != setting;
    if host_changed {
        memo.colors = colors.cloned();
        memo.bar = bar.cloned();
        memo.setting = setting.cloned();
    }
    if state_changed || config_changed || host_changed {
        memo.valid = false;
    }

    let now = ctx.now_ms;
    memo.scratch.clear();
    for timer in &memo.timers {
        memo.scratch
            .push(duration::approx_token(timer.remaining_seconds(now)));
        memo.scratch.push(u64::from(timer.paused));
    }
    if let Some(nearest) = memo.timers.first() {
        memo.scratch
            .push(u64::from(nearest.fraction_step(now, RING_STEPS)));
        let urgency = if nearest.paused {
            0
        } else if nearest.within(now, 20) {
            1
        } else if nearest.within(now, 50) {
            2
        } else {
            3
        };
        memo.scratch.push(urgency);
    }
    memo.scratch
        .push(u64::from(memo.timers.iter().any(|timer| !timer.paused)));

    if memo.valid && memo.scratch == memo.tokens {
        return true;
    }
    mem::swap(&mut memo.scratch, &mut memo.tokens);
    memo.valid = true;
    false
}

/// Handles one widget event.
///
/// # Errors
/// The signature matches the entry contract; this handler does not fail.
pub fn update(ctx: &Context) -> Result<Vec<Op>> {
    match ctx.event.as_str() {
        "click" => Ok(vec![Op::TogglePanel {
            id: PANEL_ID.to_owned(),
        }]),
        "right_click" => {
            let timers = timer::list(ctx);
            Ok(timer::toggle(ctx, timers.first()).into_iter().collect())
        }
        "exit" => {
            cleanup(ctx, &mut lock());
            Ok(Vec::new())
        }
        _ => Ok(render(ctx, &mut lock())),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one render pass from state to the final op list"
)]
fn render(ctx: &Context, widget: &mut Widget) -> Vec<Op> {
    if memo_hit(ctx, widget) {
        return Vec::new();
    }
    let config = Config::read(ctx);
    let colors = &config.colors;
    let mut ops = Vec::new();

    if let Some(op) = widget.published.update(colors) {
        ops.push(op);
    }

    let timers = timer::list(ctx);
    if config.hide_when_idle && timers.is_empty() {
        ops.push(Op::BarVisible { visible: false });
        set_interval(widget, HIDDEN_INTERVAL_MS, &mut ops);
        return ops;
    }
    ops.push(Op::BarVisible { visible: true });

    let vertical = ctx.bar_vertical();
    let nearest = timers.first();
    let idle = nearest.is_none();
    let count = timers.len();
    let now = ctx.now_ms;
    let state = nearest.map_or("idle", |timer| urgency::state(timer, now));
    let badge = badge_role(&config, state, idle);

    let fallback = Fallback {
        config: &config,
        nearest,
        count,
        badge,
        vertical,
        state,
        now,
    };
    if widget.fallback {
        ops.extend(fallback_tree(&fallback));
    } else {
        let showing = config.show_progress
            && widget.write_ring(
                ctx,
                &Ring {
                    timer: nearest,
                    count,
                    show_count: config.show_count,
                    colors,
                    idle,
                },
                &mut ops,
            );
        if showing {
            let spec = Label {
                nearest,
                vertical,
                now,
                state,
                badge,
            };
            let (text, color) = label(&config, &spec);
            set_label(widget, BarLabel { text, color }, &mut ops);
        } else {
            widget.fallback = true;
            widget.memo.valid = false;
            ops.extend(fallback_tree(&fallback));
        }
    }

    if let Some(value) = tooltip(
        ctx,
        widget,
        &Tooltip {
            timers: &timers,
            now,
        },
    ) {
        ops.push(Op::BarTooltip { value });
    }
    let running = timers.iter().any(|timer| !timer.paused);
    set_interval(
        widget,
        if running {
            RUNNING_INTERVAL_MS
        } else {
            IDLE_INTERVAL_MS
        },
        &mut ops,
    );
    ops
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per widget setting, mirroring the manifest"
)]
struct Config {
    show_text: bool,
    show_count: bool,
    show_progress: bool,
    hide_when_idle: bool,
    colors: Colors,
}

impl Config {
    fn read(ctx: &Context) -> Self {
        Self {
            show_text: ctx.config_bool("show_text", true),
            show_count: ctx.config_bool("show_count", true),
            show_progress: ctx.config_bool("show_progress", true),
            hide_when_idle: ctx.config_bool("hide_when_idle", false),
            colors: Colors::widget(ctx),
        }
    }
}

fn badge_role<'a>(config: &'a Config, state: &str, idle: bool) -> &'a str {
    if idle {
        return &config.colors.idle;
    }
    if state == "paused" {
        return "on_surface_variant";
    }
    if config.colors.use_urgency {
        return match state {
            "urgent" => &config.colors.urgent,
            "warning" => &config.colors.warning,
            _ => &config.colors.safe,
        };
    }
    &config.colors.safe
}

struct Label<'a> {
    nearest: Option<&'a Timer>,
    vertical: bool,
    now: i64,
    state: &'a str,
    badge: &'a str,
}

fn label(config: &Config, spec: &Label<'_>) -> (Option<String>, String) {
    let Label {
        nearest,
        vertical,
        now,
        state,
        badge,
    } = *spec;
    let Some(timer) = nearest else {
        return (None, "on_surface".to_owned());
    };
    if !config.show_text || vertical {
        return (None, "on_surface".to_owned());
    }
    let text = duration::approx(timer.remaining_seconds(now));
    if state == "paused" {
        return (Some(text), "on_surface_variant".to_owned());
    }
    if config.colors.use_urgency && (state == "warning" || state == "urgent") {
        return (Some(text), badge.to_owned());
    }
    (Some(text), "on_surface".to_owned())
}

struct BarLabel {
    text: Option<String>,
    color: String,
}

fn set_label(widget: &mut Widget, label: BarLabel, ops: &mut Vec<Op>) {
    let BarLabel { text, color } = label;
    let label = text.unwrap_or_default();
    if widget.text.as_deref() != Some(label.as_str()) {
        ops.push(Op::BarText {
            text: label.clone(),
        });
        widget.text = Some(label);
    }
    if widget.color.as_deref() != Some(color.as_str()) {
        widget.color = Some(color.clone());
        ops.push(Op::BarColor { role: color });
    }
}

fn set_interval(widget: &mut Widget, ms: i64, ops: &mut Vec<Op>) {
    if widget.interval == Some(ms) {
        return;
    }
    widget.interval = Some(ms);
    ops.push(Op::set_interval(ms.unsigned_abs()));
}

struct Fallback<'a> {
    config: &'a Config,
    nearest: Option<&'a Timer>,
    count: usize,
    badge: &'a str,
    vertical: bool,
    state: &'a str,
    now: i64,
}

fn fallback_tree(spec: &Fallback<'_>) -> Vec<Op> {
    let Fallback {
        config,
        nearest,
        count,
        badge,
        vertical,
        state,
        now,
    } = *spec;
    let mut children = Vec::new();

    if config.show_count {
        children.push(count_badge(count, badge));
    }
    if let Some(timer) = nearest
        && config.show_text
        && !vertical
    {
        let mut label = Props::new()
            .text("text", duration::approx(timer.remaining_seconds(now)));
        let color = if state == "paused" {
            Some("on_surface_variant")
        } else if config.colors.use_urgency
            && (state == "warning" || state == "urgent")
        {
            Some(badge)
        } else {
            None
        };
        if let Some(role) = color {
            label = label.text("color", role.to_owned());
        }
        children.push(ui::label(label));
    }

    let props = Props::new()
        .int("gap", if vertical { 3 } else { 6 })
        .text("align", "center");
    let tree = if vertical {
        ui::column(props, children)
    } else {
        ui::row(props, children)
    };
    vec![Op::BarRender { tree }]
}

fn count_badge(count: usize, role: &str) -> ui::UiNode {
    ui::row(
        Props::new()
            .text("fill", format!("{role}/0.18"))
            .int("radius", 6)
            .int("paddingH", 5)
            .text("align", "center"),
        vec![ui::label(
            Props::new()
                .text("text", count.to_string())
                .text("color", role.to_owned())
                .text("fontWeight", "bold")
                .int("fontSize", 12),
        )],
    )
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "two independent render flags from the widget state"
)]
struct Ring<'a> {
    timer: Option<&'a Timer>,
    count: usize,
    show_count: bool,
    colors: &'a Colors,
    idle: bool,
}

impl Widget {
    #[expect(
        clippy::too_many_arguments,
        reason = "the host context, the render spec, and the op sink"
    )]
    #[expect(
        clippy::excessive_nesting,
        reason = "one guarded redraw with its file error path"
    )]
    fn write_ring(
        &mut self,
        ctx: &Context,
        ring: &Ring<'_>,
        ops: &mut Vec<Op>,
    ) -> bool {
        let now = ctx.now_ms;
        let step = match ring.timer {
            Some(timer) if !ring.idle && timer.seconds > 0 => {
                timer.fraction_step(now, RING_STEPS)
            }
            _ => RING_STEPS,
        };
        let role = if ring.idle {
            ring.colors.idle.as_str()
        } else {
            ring.timer.map_or(ring.colors.idle.as_str(), |timer| {
                urgency::role_ref(timer, now, ring.colors)
            })
        };
        let arc = color(ctx, role, "#CBA6F7");
        let track = color(ctx, "outline_variant", "#4C4F69");
        let count = ring.show_count.then_some(ring.count);

        // The file is only rewritten when the picture changes, and the arc is
        // quantized first, so a long timer stops rewriting it every second.
        let unchanged = self.ring_arc.as_deref() == Some(arc)
            && self.ring_track.as_deref() == Some(track)
            && self.ring_step == step
            && self.ring_count == count;
        if !unchanged {
            let circumference = TAU * RING_RADIUS_F;
            let fraction = f64::from(step) / f64::from(RING_STEPS);
            let offset = circumference * (1.0 - fraction);
            let count_text = count_markup(ctx, arc, count);
            let svg = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{RING_VIEWBOX}\" height=\"{RING_VIEWBOX}\" viewBox=\"0 0 {RING_VIEWBOX} {RING_VIEWBOX}\">\
             <circle cx=\"20\" cy=\"20\" r=\"{RING_RADIUS}\" fill=\"none\" stroke=\"{track}\" stroke-width=\"{RING_STROKE}\"/>\
             <circle cx=\"20\" cy=\"20\" r=\"{RING_RADIUS}\" fill=\"none\" stroke=\"{arc}\" stroke-width=\"{RING_STROKE}\" stroke-dasharray=\"{circumference:.2}\" stroke-dashoffset=\"{offset:.2}\" stroke-linecap=\"round\" transform=\"rotate(-90 20 20)\"/>\
             {count_text}</svg>"
            );
            if !write_ring_file(ctx, self, &svg) {
                return false;
            }
            self.ring_arc = Some(arc.to_owned());
            self.ring_track = Some(track.to_owned());
            self.ring_step = step;
            self.ring_count = count;
        }

        if !self.image_set {
            ensure_paths(ctx, self);
            let path = self.paths.as_ref().map_or_default(|(path, _)| {
                path.to_string_lossy().into_owned()
            });
            ops.push(Op::BarImage {
                path,
                preserve: true,
                width: RING_SIZE,
                height: RING_SIZE,
            });
            self.image_set = true;
        }
        true
    }
}

fn write_ring_file(ctx: &Context, widget: &mut Widget, svg: &str) -> bool {
    ensure_paths(ctx, widget);
    let Some((path, temporary)) = widget.paths.as_ref() else {
        return false;
    };
    fs::write(temporary, svg)
        .and_then(|()| fs::rename(temporary, path))
        .is_ok()
}

fn ensure_paths(ctx: &Context, widget: &mut Widget) {
    if widget.paths.is_some() {
        return;
    }
    let name =
        format!("{}-{}", ctx.bar_output_name().unwrap_or("bar"), ctx.now_ms);
    let instance = widget.instance.get_or_insert_with(|| sanitize(&name));
    let path = ctx.data_dir.join(format!("ring-{instance}.svg"));
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    widget.paths = Some((path, temporary));
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || character == '_'
                || character == '-'
            {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn cleanup(ctx: &Context, widget: &mut Widget) {
    ensure_paths(ctx, widget);
    if let Some((path, temporary)) = widget.paths.as_ref() {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(temporary);
    }
}

struct Tooltip<'a> {
    timers: &'a [Timer],
    now: i64,
}

fn tooltip(
    ctx: &Context,
    widget: &mut Widget,
    source: &Tooltip<'_>,
) -> Option<Value> {
    let Tooltip { timers, now } = *source;
    if timers.is_empty() {
        let empty = ctx.tr("bar.empty", &[]);
        if widget.tooltip.as_deref() == Some(empty.as_str()) {
            return None;
        }
        widget.tooltip = Some(empty.clone());
        return Some(Value::String(empty));
    }

    let mut keys = String::new();
    for timer in timers {
        if !keys.is_empty() {
            keys.push('|');
        }
        let value = if timer.paused {
            let left = duration::approx(timer.remaining_seconds(now));
            ctx.tr("bar.paused", &[("time", left)])
        } else {
            duration::approx(timer.remaining_seconds(now))
        };
        let _ = write!(keys, "{}={value}", timer.id);
    }
    if widget.tooltip.as_deref() == Some(keys.as_str()) {
        return None;
    }
    widget.tooltip = Some(keys);

    let rows = timers
        .iter()
        .map(|timer| {
            let left = duration::approx(timer.remaining_seconds(now));
            let title = timer.message.clone().unwrap_or_else(|| {
                ctx.tr("bar.timer", &[("duration", timer.label.clone())])
            });
            let value = if timer.paused {
                ctx.tr("bar.paused", &[("time", left)])
            } else {
                left
            };
            json!({ "key": title, "value": value })
        })
        .collect();
    Some(Value::Array(rows))
}

fn color<'a>(ctx: &'a Context, role: &str, fallback: &'a str) -> &'a str {
    ctx.color(role).and_then(Value::as_str).unwrap_or(fallback)
}

static FONT: Mutex<Option<String>> = Mutex::new(None);

fn font_family(ctx: &Context) -> String {
    let mut font = FONT.lock().unwrap_or_else(PoisonError::into_inner);
    font.get_or_insert_with(|| {
        ctx.setting_value("shell.font_family")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "sans-serif".to_owned())
    })
    .clone()
}

fn count_markup(ctx: &Context, arc: &str, count: Option<usize>) -> String {
    count.map_or_else(String::new, |total| {
        format!(
            "<text x=\"20\" y=\"26\" text-anchor=\"middle\" font-family=\"{}\" font-size=\"19\" font-weight=\"bold\" fill=\"{arc}\">{total}</text>",
            xml_escape(&font_family(ctx))
        )
    })
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_context;
    use crate::timer::COLORS_KEY;

    #[test]
    fn sanitize_keeps_safe_name_characters() {
        assert_eq!(sanitize("DP-1-1700"), "DP-1-1700");
        assert_eq!(sanitize("a b/c"), "a_b_c");
    }

    #[test]
    fn xml_escape_matches_plugin_output() {
        assert_eq!(xml_escape("a&b\"<c>"), "a&amp;b&quot;&lt;c&gt;");
    }

    #[test]
    fn badge_role_prefers_idle_then_paused() {
        let ctx = test_context::context(
            "render",
            json!({ "hide_when_idle": false }),
            json!({}),
            json!({ "now_ms": 1_000, "colors": { "primary": "#111" } }),
        );
        let config = Config::read(&ctx);
        assert_eq!(badge_role(&config, "safe", true), "primary");
        assert_eq!(badge_role(&config, "paused", false), "on_surface_variant");
        assert_eq!(badge_role(&config, "urgent", false), "error");
    }

    #[test]
    fn label_hides_when_vertical_or_disabled() {
        let timer = Timer {
            id: "one".to_owned(),
            seconds: 600,
            label: "10m".to_owned(),
            message: None,
            paused: false,
            ends_at: Some(601_000),
            remaining_ms: None,
        };
        let ctx = test_context::context(
            "render",
            json!({ "show_text": false }),
            json!({}),
            json!({ "now_ms": 1_000 }),
        );
        let config = Config::read(&ctx);
        let spec = Label {
            nearest: Some(&timer),
            vertical: false,
            now: 1_000,
            state: "safe",
            badge: "primary",
        };
        assert_eq!(label(&config, &spec), (None, "on_surface".to_owned()));
        let vertical = Label {
            vertical: true,
            ..spec
        };
        assert_eq!(label(&config, &vertical), (None, "on_surface".to_owned()));
    }

    #[test]
    fn memo_skips_unchanged_renders_and_wakes_on_updates() {
        let now: i64 = 1_800_000_000_000;
        let config = json!({
            "show_text": true,
            "show_count": true,
            "show_progress": true,
            "hide_when_idle": false,
            "use_urgency_colors": true,
        });
        let timers = |label: &str| {
            json!({ "timers": [{
                "id": "one",
                "seconds": 10,
                "label": label,
                "paused": false,
                "endsAt": now + 10_000,
            }] })
        };
        let host = |color: &str| {
            json!({
                "now_ms": now,
                "bar": { "vertical": false, "output_name": "DP-1" },
                "colors": { "tertiary": color },
                "setting": { "shell.font_family": "sans-serif" },
            })
        };
        let mut widget = Widget::new();

        let context = test_context::context(
            "render",
            config.clone(),
            timers("10s"),
            host("#111111"),
        );
        assert!(!render(&context, &mut widget).is_empty());
        assert!(render(&context, &mut widget).is_empty());

        let changed = test_context::context(
            "render",
            config.clone(),
            timers("changed"),
            host("#111111"),
        );
        assert!(!render(&changed, &mut widget).is_empty());

        let mut later = test_context::context(
            "render",
            config.clone(),
            timers("changed"),
            host("#111111"),
        );
        later.now_ms = now.saturating_add(200);
        assert!(!render(&later, &mut widget).is_empty());

        let mut themed_config = config;
        themed_config["safe_color"] = json!("primary");
        let themed = test_context::context(
            "render",
            themed_config,
            timers("changed"),
            host("#222222"),
        );
        assert!(render(&themed, &mut widget).iter().any(|op| {
            serde_json::to_value(op).expect("op")
                == json!({
                    "op": "state_set",
                    "key": COLORS_KEY,
                    "value": {
                        "safe": "primary",
                        "warning": "secondary",
                        "urgent": "error",
                        "idle": "primary",
                        "useUrgency": true,
                    },
                })
        }));
    }
}
