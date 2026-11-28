//! Progress-based timer colors shared by the bar widget and the panel.

use noctalia_rs::{Context, Op};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::sync::lock;
use crate::timer::{COLORS_KEY, Timer};

/// How much of a timer is left, from safe to urgent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Safe,
    Warning,
    Urgent,
    Paused,
}

impl State {
    #[must_use]
    pub fn of(timer: &Timer, now: i64) -> Self {
        Self::of_remaining(timer, timer.remaining_ms(now))
    }

    #[must_use]
    pub(crate) const fn of_remaining(timer: &Timer, remaining: i64) -> Self {
        if timer.paused {
            return Self::Paused;
        }
        if timer.within_ms(remaining, 20) {
            return Self::Urgent;
        }
        if timer.within_ms(remaining, 50) {
            return Self::Warning;
        }
        Self::Safe
    }

    /// Distinct number for the render caches that compare states by value.
    #[must_use]
    pub const fn token(self) -> u64 {
        match self {
            Self::Paused => 0,
            Self::Urgent => 1,
            Self::Warning => 2,
            Self::Safe => 3,
        }
    }

    /// Color role for the ring, the progress bar, and the count badge.
    #[must_use]
    pub fn role_ref(self, colors: &Colors) -> &str {
        match self {
            Self::Paused => "on_surface_variant",
            Self::Warning if colors.use_urgency => &colors.warning,
            Self::Urgent if colors.use_urgency => &colors.urgent,
            Self::Safe | Self::Warning | Self::Urgent => &colors.safe,
        }
    }

    #[must_use]
    pub fn role(self, colors: &Colors) -> String {
        self.role_ref(colors).to_owned()
    }

    /// Color role for the remaining time, or `None` for the default text color.
    #[must_use]
    pub fn text_role(self, colors: &Colors) -> Option<String> {
        match self {
            Self::Paused => Some("on_surface_variant".to_owned()),
            Self::Warning | Self::Urgent if colors.use_urgency => {
                Some(self.role(colors))
            }
            Self::Safe | Self::Warning | Self::Urgent => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
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

static MIRROR: Mutex<Option<Arc<Colors>>> = Mutex::new(None);

/// Panel-side cache: the widget publishes its colors, so read them once and
/// follow changes instead of querying on every render.
#[must_use]
pub fn panel_colors(ctx: &Context) -> (Arc<Colors>, Vec<Op>) {
    let mut mirror = lock(&MIRROR);
    let mut ops = Vec::new();
    if mirror.is_none() {
        let value = ctx.host.get("timed_colors");
        *mirror = Some(Arc::new(Colors::from_value(value)));
        ops.push(Op::state_watch(COLORS_KEY, "colors"));
    }
    let colors = mirror
        .clone()
        .unwrap_or_else(|| Arc::new(Colors::defaults()));
    (colors, ops)
}

pub fn store_colors(value: Option<&Value>) {
    *lock(&MIRROR) = Some(Arc::new(Colors::from_value(value)));
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
        assert_eq!(State::of(&timer, 0), State::Safe);
        assert_eq!(State::of(&timer, 50_000), State::Warning);
        assert_eq!(State::of(&timer, 80_000), State::Urgent);
        assert_eq!(State::of(&timer, 90_001), State::Urgent);
    }

    #[test]
    fn paused_outranks_progress() {
        let mut timer = timer(100, 100_000);
        timer.paused = true;
        assert_eq!(State::of(&timer, 100_000), State::Paused);
        assert_eq!(
            State::Paused.role(&Colors::defaults()),
            "on_surface_variant"
        );
    }

    #[test]
    fn urgency_can_be_disabled() {
        let colors = Colors {
            use_urgency: false,
            ..Colors::defaults()
        };
        assert_eq!(State::Urgent.role(&colors), "tertiary");
        assert_eq!(State::Urgent.text_role(&colors), None);
    }

    #[test]
    fn text_role_follows_pause_and_urgency() {
        let colors = Colors::defaults();
        assert_eq!(State::Safe.text_role(&colors), None);
        assert_eq!(
            State::Warning.text_role(&colors),
            Some("secondary".to_owned())
        );
        assert_eq!(State::Urgent.text_role(&colors), Some("error".to_owned()));
        assert_eq!(
            State::Paused.text_role(&colors),
            Some("on_surface_variant".to_owned())
        );
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
