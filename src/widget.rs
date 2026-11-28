//! Timed bar widget. Shows the nearest timer as a progress ring with the timer
//! count inside, plus the estimated remaining time. Left-clicking opens the
//! panel; right-clicking pauses or resumes the nearest timer.

use std::f64::consts::TAU;
use std::fmt::Write;
use std::fs;
use std::mem;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;

use noctalia_rs::{Context, Op, Props, Result, ui};
use serde_json::{Value, json};

use crate::duration;
use crate::file;
use crate::i18n::tr;
use crate::math::quotient;
use crate::sync::lock;
use crate::timer::{self, Timer};
use crate::urgency::{self, Colors, State};

const PANEL_ID: &str = "dontblameme/timed:panel";
const MIN_INTERVAL_MS: i64 = 1_000;
const IDLE_INTERVAL_MS: i64 = 60_000;
const HIDDEN_INTERVAL_MS: i64 = 86_400_000;

const RING_SIZE: u32 = 26;
const RING_VIEWBOX: u32 = 40;
const RING_RADIUS: u32 = 16;
const RING_STROKE: u32 = 4;
const ARC_FALLBACK: &str = "#CBA6F7";
const TRACK_FALLBACK: &str = "#4C4F69";
/// The arc is redrawn in `1 / RING_STEPS` of the circle. At 26 px that is a
/// third of a pixel, so quantizing saves the file write without a visible
/// change.
const RING_STEPS: u16 = 200;

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent one-time render flags"
)]
struct Widget {
    path: Option<PathBuf>,
    published: urgency::Published,
    ring: Option<Picture>,
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
    revision: Option<i64>,
    setting: Option<Value>,
    timers: Vec<Timer>,
    tokens: Vec<u64>,
    scratch: Vec<u64>,
}

impl Widget {
    const fn new() -> Self {
        Self {
            path: None,
            published: urgency::Published::new(),
            ring: None,
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
                revision: None,
                setting: None,
                timers: Vec::new(),
                tokens: Vec::new(),
                scratch: Vec::new(),
            },
        }
    }
}

static WIDGET: Mutex<Widget> = Mutex::new(Widget::new());

/// Returns true when the next render would emit the same picture. The cache
/// keys on the inputs the widget draws, so a state or theme change, or a move
/// of the drawn clock or ring, forces a fresh tree.
fn memo_hit(ctx: &Context, widget: &mut Widget) -> bool {
    let memo = &mut widget.memo;
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
    if ctx.event != "render"
        || ctx.render_source() == "load"
        || ctx.setting("shell.font_family").is_none()
    {
        memo.valid = false;
        return false;
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
            .push(duration::approx_detailed_token_ms(timer.remaining_ms(now)));
        memo.scratch.push(u64::from(timer.paused));
    }
    if let Some(nearest) = memo.timers.first() {
        let remaining = nearest.remaining_ms(now);
        memo.scratch
            .push(u64::from(nearest.fraction_step_ms(remaining, RING_STEPS)));
        memo.scratch
            .push(State::of_remaining(nearest, remaining).token());
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
            cleanup(&mut lock(&WIDGET));
            Ok(Vec::new())
        }
        _ => Ok(render(ctx, &mut lock(&WIDGET))),
    }
}

fn render(ctx: &Context, widget: &mut Widget) -> Vec<Op> {
    let config = Config::read(ctx);
    if memo_hit(ctx, widget) {
        let interval = next_interval(
            &widget.memo.timers,
            &config,
            ctx.now_ms,
            widget.fallback,
        );
        let mut ops = Vec::new();
        set_interval(widget, interval, &mut ops);
        return ops;
    }
    let timers = mem::take(&mut widget.memo.timers);
    let ops = render_timers(ctx, widget, &timers);
    widget.memo.timers = timers;
    ops
}

fn render_timers(
    ctx: &Context,
    widget: &mut Widget,
    timers: &[Timer],
) -> Vec<Op> {
    let config = Config::read(ctx);
    let colors = &config.colors;
    let mut ops = Vec::new();

    if let Some(op) = widget.published.update(colors) {
        ops.push(op);
    }

    if config.hide_when_idle && timers.is_empty() {
        ops.push(Op::BarVisible { visible: false });
        set_interval(widget, HIDDEN_INTERVAL_MS, &mut ops);
        return ops;
    }
    ops.push(Op::BarVisible { visible: true });

    let vertical = ctx.bar_vertical();
    let nearest = timers.first();
    let count = timers.len();
    let now = ctx.now_ms;
    let badge = badge_role(nearest, now, colors);

    let fallback = Fallback {
        config: &config,
        nearest,
        count,
        badge,
        vertical,
        now,
    };
    if widget.fallback {
        ops.extend(fallback_tree(&fallback));
    } else {
        let ring = Ring {
            timer: nearest,
            count,
            show_count: config.show_count,
            colors,
        };
        let showing =
            config.show_progress && widget.write_ring(ctx, &ring, &mut ops);
        if showing {
            let spec = Label {
                nearest,
                vertical,
                now,
            };
            set_label(widget, label(&config, &spec), &mut ops);
        } else {
            widget.fallback = true;
            widget.memo.valid = false;
            ops.extend(fallback_tree(&fallback));
        }
    }

    if let Some(value) = tooltip(ctx, widget, &Tooltip { timers, now }) {
        ops.push(Op::BarTooltip { value });
    }
    set_interval(
        widget,
        next_interval(timers, &config, now, widget.fallback),
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

/// The badge and the ring share the timer's color, with one of its own while
/// no timer is running.
fn badge_role<'a>(
    nearest: Option<&Timer>,
    now: i64,
    colors: &'a Colors,
) -> &'a str {
    nearest.map_or_else(
        || colors.idle.as_str(),
        |timer| State::of(timer, now).role_ref(colors),
    )
}

struct Label<'a> {
    nearest: Option<&'a Timer>,
    vertical: bool,
    now: i64,
}

struct BarLabel {
    text: Option<String>,
    color: String,
}

impl BarLabel {
    /// No text, in the default text color.
    fn plain() -> Self {
        Self {
            text: None,
            color: "on_surface".to_owned(),
        }
    }
}

fn label(config: &Config, spec: &Label<'_>) -> BarLabel {
    let Some(timer) =
        spec.nearest.filter(|_| config.show_text && !spec.vertical)
    else {
        return BarLabel::plain();
    };
    let text = duration::approx_detailed(timer.remaining_seconds(spec.now));
    let color = State::of(timer, spec.now)
        .text_role(&config.colors)
        .unwrap_or_else(|| "on_surface".to_owned());
    BarLabel {
        text: Some(text),
        color,
    }
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

#[expect(
    clippy::too_many_arguments,
    reason = "the timer list, display settings, clock, and render mode"
)]
fn next_interval(
    timers: &[Timer],
    config: &Config,
    now: i64,
    fallback: bool,
) -> i64 {
    if timers.is_empty() && config.hide_when_idle {
        return HIDDEN_INTERVAL_MS;
    }
    if !timers.iter().any(|timer| !timer.paused) {
        return IDLE_INTERVAL_MS;
    }
    let mut delay = HIDDEN_INTERVAL_MS;
    for timer in timers.iter().filter(|timer| !timer.paused) {
        let remaining = timer.remaining_ms(now);
        delay = delay.min(remaining);
        if let Some(next) = duration::next_detailed_change_ms(remaining) {
            delay = delay.min(next);
        }
    }
    if let Some(nearest) = timers.first() {
        if let Some(next) =
            next_urgency_change_ms(nearest, now, config.colors.use_urgency)
        {
            delay = delay.min(next);
        }
        if config.show_progress
            && !fallback
            && let Some(next) = next_ring_change_ms(nearest, now)
        {
            delay = delay.min(next);
        }
    }
    delay.clamp(MIN_INTERVAL_MS, HIDDEN_INTERVAL_MS)
}

fn next_urgency_change_ms(
    timer: &Timer,
    now: i64,
    enabled: bool,
) -> Option<i64> {
    if !enabled {
        return None;
    }
    let total = timer.seconds.saturating_mul(1_000);
    let remaining = timer.remaining_ms(now);
    let mut next = None;
    for share in [50, 20] {
        let threshold =
            quotient(total.saturating_mul(share).saturating_add(99), 100);
        if remaining > threshold {
            let delay = remaining.saturating_sub(threshold);
            next = Some(next.map_or(delay, |value: i64| value.min(delay)));
        }
    }
    next
}

fn next_ring_change_ms(timer: &Timer, now: i64) -> Option<i64> {
    let remaining = timer.remaining_ms(now);
    let step = i64::from(timer.fraction_step_ms(remaining, RING_STEPS));
    if step == 0 {
        return None;
    }
    let total = timer.seconds.saturating_mul(1_000);
    let steps = i64::from(RING_STEPS);
    // The rounded step drops when remaining crosses (step - 1/2) / steps.
    let boundary = quotient(
        total
            .saturating_mul(step.saturating_mul(2).saturating_sub(1))
            .saturating_sub(1),
        steps.saturating_mul(2),
    );
    (boundary < remaining).then_some(remaining.saturating_sub(boundary))
}

struct Fallback<'a> {
    config: &'a Config,
    nearest: Option<&'a Timer>,
    count: usize,
    badge: &'a str,
    vertical: bool,
    now: i64,
}

fn fallback_tree(spec: &Fallback<'_>) -> Vec<Op> {
    let Fallback {
        config,
        nearest,
        count,
        badge,
        vertical,
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
        let mut label = Props::new().text(
            "text",
            duration::approx_detailed(timer.remaining_seconds(now)),
        );
        if let Some(role) = State::of(timer, now).text_role(&config.colors) {
            label = label.text("color", role);
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

struct Ring<'a> {
    timer: Option<&'a Timer>,
    count: usize,
    show_count: bool,
    colors: &'a Colors,
}

impl Ring<'_> {
    fn picture(&self, ctx: &Context) -> Picture {
        let now = ctx.now_ms;
        let role = self.timer.map_or_else(
            || self.colors.idle.as_str(),
            |timer| State::of(timer, now).role_ref(self.colors),
        );
        Picture {
            arc: color(ctx, role, ARC_FALLBACK).to_owned(),
            track: color(ctx, "outline_variant", TRACK_FALLBACK).to_owned(),
            step: self
                .timer
                .filter(|timer| timer.seconds > 0)
                .map_or(RING_STEPS, |timer| {
                    timer.fraction_step(now, RING_STEPS)
                }),
            count: self.show_count.then_some(self.count),
        }
    }
}

/// What the ring draws. Keeping it lets a render with the same picture skip
/// the file write.
#[derive(Debug, PartialEq)]
struct Picture {
    arc: String,
    track: String,
    step: u16,
    count: Option<usize>,
}

impl Picture {
    fn svg(&self, ctx: &Context) -> String {
        let circumference = TAU * f64::from(RING_RADIUS);
        let fraction = f64::from(self.step) / f64::from(RING_STEPS);
        let offset = circumference * (1.0 - fraction);
        let count = count_markup(ctx, &self.arc, self.count);
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{RING_VIEWBOX}\" height=\"{RING_VIEWBOX}\" viewBox=\"0 0 {RING_VIEWBOX} {RING_VIEWBOX}\">\
             <circle cx=\"20\" cy=\"20\" r=\"{RING_RADIUS}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{RING_STROKE}\"/>\
             <circle cx=\"20\" cy=\"20\" r=\"{RING_RADIUS}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{RING_STROKE}\" stroke-dasharray=\"{circumference:.2}\" stroke-dashoffset=\"{offset:.2}\" stroke-linecap=\"round\" transform=\"rotate(-90 20 20)\"/>\
             {count}</svg>",
            self.track, self.arc
        )
    }
}

impl Widget {
    #[expect(
        clippy::too_many_arguments,
        reason = "the context, the render spec, and the op sink"
    )]
    fn write_ring(
        &mut self,
        ctx: &Context,
        ring: &Ring<'_>,
        ops: &mut Vec<Op>,
    ) -> bool {
        let picture = ring.picture(ctx);
        if self.ring.as_ref() == Some(&picture) {
            self.show_image(ctx, ops);
            return true;
        }
        if !write_ring_file(ctx, self, &picture.svg(ctx)) {
            return false;
        }
        self.ring = Some(picture);
        self.show_image(ctx, ops);
        true
    }

    fn show_image(&mut self, ctx: &Context, ops: &mut Vec<Op>) {
        if self.image_set {
            return;
        }
        let path = ring_path(ctx, self).map_or_else(String::new, |path| {
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
}

fn write_ring_file(ctx: &Context, widget: &mut Widget, svg: &str) -> bool {
    ring_path(ctx, widget)
        .is_some_and(|path| file::write_atomic(path, svg).is_ok())
}

/// The ring file keeps the name it got on the first render, so the image op
/// always points at the same file.
fn ring_path<'a>(ctx: &Context, widget: &'a mut Widget) -> Option<&'a Path> {
    if widget.path.is_none() {
        // Each bar runs its helper with the same data directory. When the
        // output name is unavailable at load, a clock alone can name the
        // same file for two processes starting in the same millisecond.
        let name = format!(
            "{}-{}-{}",
            ctx.bar_output_name().unwrap_or("bar"),
            process::id(),
            ctx.now_ms
        );
        widget.path =
            Some(ctx.data_dir.join(format!("ring-{}.svg", sanitize(&name))));
    }
    widget.path.as_deref()
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

fn cleanup(widget: &mut Widget) {
    if let Some(path) = widget.path.take() {
        let _ = fs::remove_file(path);
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
        let empty = tr(ctx, "bar.empty", &[]);
        if widget.tooltip.as_deref() == Some(empty.as_str()) {
            return None;
        }
        widget.tooltip = Some(empty.clone());
        return Some(Value::String(empty));
    }

    let mut keys = String::new();
    let mut values = Vec::with_capacity(timers.len());
    for timer in timers {
        if !keys.is_empty() {
            keys.push('|');
        }
        let value = if timer.paused {
            let left = duration::approx_detailed(timer.remaining_seconds(now));
            tr(ctx, "bar.paused", &[("time", left)])
        } else {
            duration::approx_detailed(timer.remaining_seconds(now))
        };
        let _ = write!(keys, "{}={value}", timer.id);
        values.push(value);
    }
    if widget.tooltip.as_deref() == Some(keys.as_str()) {
        return None;
    }
    widget.tooltip = Some(keys);

    let rows = timers
        .iter()
        .zip(values)
        .map(|(timer, value)| {
            let title = timer.message.clone().unwrap_or_else(|| {
                tr(ctx, "bar.timer", &[("duration", timer.label.clone())])
            });
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
    let mut font = lock(&FONT);
    font.get_or_insert_with(|| {
        ctx.setting("shell.font_family")
            .and_then(Value::as_str)
            .unwrap_or("sans-serif")
            .to_owned()
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
    use std::slice;

    fn running() -> Timer {
        Timer {
            id: "one".to_owned(),
            seconds: 100,
            label: "1m 40s".to_owned(),
            message: None,
            paused: false,
            ends_at: Some(1_100_000),
            remaining_ms: None,
        }
    }

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
        let colors = Colors::defaults();
        assert_eq!(badge_role(None, 1_090_000, &colors), "primary");

        let mut paused = running();
        paused.paused = true;
        assert_eq!(
            badge_role(Some(&paused), 1_090_000, &colors),
            "on_surface_variant"
        );
        assert_eq!(badge_role(Some(&running()), 1_090_000, &colors), "error");
    }

    #[test]
    fn label_hides_when_vertical_or_disabled() {
        let timer = running();
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
        };
        let hidden = label(&config, &spec);
        assert_eq!(hidden.text, None);
        assert_eq!(hidden.color, "on_surface");

        let vertical = Label {
            vertical: true,
            ..spec
        };
        assert_eq!(label(&config, &vertical).text, None);
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
                "timed_state_version": 0,
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

        let mut changed_host = host("#111111");
        changed_host["timed_state_version"] = json!(1);
        let changed = test_context::context(
            "render",
            config.clone(),
            timers("changed"),
            changed_host,
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
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "the fixture mirrors two independent widget settings"
    )]
    fn config(show_progress: bool, use_urgency: bool) -> Config {
        Config {
            show_text: true,
            show_count: true,
            show_progress,
            hide_when_idle: false,
            colors: Colors {
                use_urgency,
                ..Colors::defaults()
            },
        }
    }

    #[test]
    fn interval_waits_for_the_next_visible_change() {
        let now: i64 = 1_800_000_000_000;
        let long = Timer {
            seconds: 86_400,
            ends_at: Some(now.saturating_add(86_400_000)),
            ..running()
        };
        assert_eq!(
            next_interval(
                slice::from_ref(&long),
                &config(true, true),
                now,
                false
            ),
            216_001
        );

        let short = Timer {
            seconds: 120,
            ends_at: Some(now.saturating_add(120_000)),
            ..running()
        };
        let timers = [long, short];
        assert_eq!(
            next_interval(&timers, &config(false, true), now, false),
            31_000
        );

        let mut paused = running();
        paused.paused = true;
        paused.ends_at = None;
        paused.remaining_ms = Some(60_000);
        assert_eq!(
            next_interval(
                slice::from_ref(&paused),
                &config(true, true),
                now,
                false
            ),
            IDLE_INTERVAL_MS
        );

        let mut hidden = config(true, true);
        hidden.hide_when_idle = true;
        assert_eq!(next_interval(&[], &hidden, now, false), HIDDEN_INTERVAL_MS);
    }

    #[test]
    fn interval_tracks_urgency_only_when_it_changes_color() {
        let now = 1_000;
        let mut timer = running();
        timer.seconds = 300;
        timer.ends_at = Some(152_000);
        assert_eq!(
            next_interval(
                slice::from_ref(&timer),
                &config(false, true),
                now,
                false
            ),
            1_000
        );
        assert_eq!(
            next_interval(
                slice::from_ref(&timer),
                &config(false, false),
                now,
                false
            ),
            2_000
        );
    }

    #[test]
    fn ring_change_lands_on_the_quantization_boundary() {
        let timer = Timer {
            seconds: 601,
            ends_at: Some(602_000),
            ..running()
        };
        let delay = next_ring_change_ms(&timer, 1_000).expect("ring change");
        assert_eq!(delay, 1_503);
        assert_eq!(timer.fraction_step_ms(601_000_i64, RING_STEPS), 200);
        assert_eq!(
            timer.fraction_step_ms(
                601_000_i64.saturating_sub(delay),
                RING_STEPS
            ),
            199
        );
        assert_eq!(
            timer.fraction_step_ms(
                601_000_i64.saturating_sub(delay).saturating_add(1),
                RING_STEPS
            ),
            200
        );
    }

    #[test]
    #[expect(
        clippy::excessive_nesting,
        reason = "the test replays the ring through every quantized step"
    )]
    fn ring_scheduling_visits_every_quantization_step() {
        for seconds in [1_i64, 10, 59, 60, 61, 599, 600, 601, 3_600, 86_400] {
            let mut remaining = seconds.saturating_mul(1_000);
            let mut timer = Timer {
                seconds,
                ends_at: Some(remaining.saturating_add(1)),
                ..running()
            };
            while remaining > 0 {
                let step = timer.fraction_step_ms(remaining, RING_STEPS);
                let Some(delay) = next_ring_change_ms(&timer, 1) else {
                    break;
                };
                assert!(delay > 0 && delay <= remaining, "seconds {seconds}");
                let boundary = remaining.saturating_sub(delay);
                assert!(
                    timer.fraction_step_ms(boundary, RING_STEPS) < step,
                    "seconds {seconds}"
                );
                assert_eq!(
                    timer.fraction_step_ms(
                        boundary.saturating_add(1),
                        RING_STEPS
                    ),
                    step,
                    "seconds {seconds}"
                );
                remaining = boundary;
                timer.ends_at = Some(remaining.saturating_add(1));
            }
        }
    }

    #[test]
    fn memo_hit_recomputes_a_shortened_interval() {
        let now: i64 = 1_800_000_000_000;
        let state = json!({ "timers": [{
            "id": "one",
            "seconds": 600,
            "label": "10m",
            "paused": false,
            "endsAt": now + 600_000
        }] });
        let host = json!({
            "now_ms": now,
            "bar": { "vertical": false, "output_name": "DP-1" },
            "colors": {},
            "setting": { "shell.font_family": "sans-serif" },
            "timed_state_version": 0,
        });
        let mut ctx = test_context::context(
            "render",
            json!({ "show_progress": true, "use_urgency_colors": true }),
            state,
            host,
        );
        let mut widget = Widget::new();
        let first = render(&ctx, &mut widget);
        assert!(first.iter().any(|op| {
            serde_json::to_value(op).expect("op")["ms"] == json!(1_501)
        }));
        ctx.now_ms = now.saturating_add(10);
        let earlier = render(&ctx, &mut widget);
        assert!(earlier.iter().any(|op| {
            serde_json::to_value(op).expect("op")["ms"] == json!(1_491)
        }));
    }
}
