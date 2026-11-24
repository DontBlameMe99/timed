//! Progress-based timer colors shared by the bar widget and the panel.

use std::sync::{Mutex, MutexGuard, PoisonError};

use noctalia_rs::{Context, Op};
use serde_json::{Value, json};

use crate::timer::{COLORS_KEY, Timer};

#[derive(Clone, Debug, PartialEq)]
pub struct Colors {
    pub safe: String,
    pub warning: String,
    pub urgent: String,
    pub idle: String,
    pub use_urgency: bool,
}

impl Colors {
    #[must_use]
    pub fn widget(ctx: &Context) -> Self {
        Self {
            safe: pick_role(
                ctx.config_str("safe_color").as_deref(),
                "tertiary",
            ),
            warning: pick_role(
                ctx.config_str("warning_color").as_deref(),
                "secondary",
            ),
            urgent: pick_role(
                ctx.config_str("urgent_color").as_deref(),
                "error",
            ),
            idle: pick_role(ctx.config_str("idle_color").as_deref(), "primary"),
            use_urgency: ctx.config_bool("use_urgency_colors", true),
        }
    }

    #[must_use]
    pub fn from_value(value: Option<&Value>) -> Self {
        let Some(saved) = value else {
            return Self::defaults();
        };
        Self {
            safe: pick_role(
                saved.get("safe").and_then(Value::as_str),
                "tertiary",
            ),
            warning: pick_role(
                saved.get("warning").and_then(Value::as_str),
                "secondary",
            ),
            urgent: pick_role(
                saved.get("urgent").and_then(Value::as_str),
                "error",
            ),
            idle: pick_role(
                saved.get("idle").and_then(Value::as_str),
                "primary",
            ),
            use_urgency: saved.get("useUrgency").and_then(Value::as_bool)
                != Some(false),
        }
    }

    #[must_use]
    pub fn defaults() -> Self {
        Self {
            safe: "tertiary".to_owned(),
            warning: "secondary".to_owned(),
            urgent: "error".to_owned(),
            idle: "primary".to_owned(),
            use_urgency: true,
        }
    }

    #[must_use]
    pub fn value(&self) -> Value {
        json!({
            "safe": self.safe,
            "warning": self.warning,
            "urgent": self.urgent,
            "idle": self.idle,
            "useUrgency": self.use_urgency,
        })
    }
}

fn pick_role(value: Option<&str>, fallback: &str) -> String {
    match value {
        Some(text) if !text.is_empty() => text.to_owned(),
        _ => fallback.to_owned(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Published {
    colors: Option<Colors>,
}

impl Published {
    #[must_use]
    pub const fn new() -> Self {
        Self { colors: None }
    }

    /// Publishes the widget colors for the panel, once per change.
    pub fn update(&mut self, colors: &Colors) -> Option<Op> {
        if self.colors.as_ref() == Some(colors) {
            return None;
        }
        self.colors = Some(colors.clone());
        Some(Op::state(COLORS_KEY, colors.value()))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

static MIRROR: Mutex<Option<Colors>> = Mutex::new(None);

/// Panel-side cache: the widget publishes its colors, so read them once and
/// follow changes instead of querying on every render.
#[must_use]
pub fn panel_colors(ctx: &Context) -> (Colors, Vec<Op>) {
    let mut guard = lock(&MIRROR);
    let mut ops = Vec::new();
    if guard.is_none() {
        let value = ctx.global_state(COLORS_KEY);
        *guard = Some(Colors::from_value(value.as_ref()));
        ops.push(Op::state_watch(COLORS_KEY, "colors"));
    }
    let colors = guard.clone().unwrap_or_else(Colors::defaults);
    (colors, ops)
}

pub fn store_colors(value: Option<&Value>) {
    *lock(&MIRROR) = Some(Colors::from_value(value));
}

#[must_use]
pub fn state(timer: &Timer, now: i64) -> &'static str {
    if timer.paused {
        return "paused";
    }
    if timer.within(now, 20) {
        return "urgent";
    }
    if timer.within(now, 50) {
        return "warning";
    }
    "safe"
}

#[must_use]
pub fn role_for_state(state: &str, colors: &Colors) -> String {
    match state {
        "urgent" => colors.urgent.clone(),
        "warning" => colors.warning.clone(),
        "paused" => "on_surface_variant".to_owned(),
        _ => colors.safe.clone(),
    }
}

#[must_use]
pub fn role(timer: &Timer, now: i64, colors: &Colors) -> String {
    role_ref(timer, now, colors).to_owned()
}

/// Borrowed form for renders that only pass the role to an op or a string
/// builder, so the common case allocates nothing.
#[must_use]
pub fn role_ref<'a>(timer: &Timer, now: i64, colors: &'a Colors) -> &'a str {
    let state = state(timer, now);
    if state == "paused" {
        return "on_surface_variant";
    }
    if !colors.use_urgency {
        return &colors.safe;
    }
    match state {
        "urgent" => &colors.urgent,
        "warning" => &colors.warning,
        _ => &colors.safe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer(seconds: i64, ends_at: i64) -> Timer {
        Timer {
            id: "one".to_owned(),
            seconds,
            label: "1m".to_owned(),
            message: None,
            paused: false,
            ends_at: Some(ends_at),
            remaining_ms: None,
        }
    }

    #[test]
    fn state_uses_remaining_percentage() {
        let timer = timer(100, 100_000);
        assert_eq!(state(&timer, 0), "safe");
        assert_eq!(state(&timer, 50_000), "warning");
        assert_eq!(state(&timer, 80_000), "urgent");
        assert_eq!(state(&timer, 90_001), "urgent");
    }

    #[test]
    fn paused_outranks_progress() {
        let mut timer = timer(100, 100_000);
        timer.paused = true;
        assert_eq!(state(&timer, 100_000), "paused");
        assert_eq!(
            role(&timer, 100_000, &Colors::defaults()),
            "on_surface_variant"
        );
    }

    #[test]
    fn urgency_can_be_disabled() {
        let timer = timer(100, 100_000);
        let colors = Colors {
            use_urgency: false,
            ..Colors::defaults()
        };
        assert_eq!(role(&timer, 90_000, &colors), "tertiary");
    }

    #[test]
    fn colors_fall_back_to_defaults() {
        let colors = Colors::from_value(Some(
            &json!({ "safe": "", "useUrgency": false }),
        ));
        assert_eq!(colors.safe, "tertiary");
        assert_eq!(colors.warning, "secondary");
        assert!(!colors.use_urgency);
    }
}
