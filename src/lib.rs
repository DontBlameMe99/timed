pub mod duration;
mod emit;
pub mod launcher;
mod math;
pub mod panel;
pub mod service;
pub mod timer;
pub mod urgency;
pub mod widget;

#[cfg(test)]
mod test_context;

use std::process::ExitCode;

use noctalia_rs::Plugin;

use self::launcher::update as launcher;
use self::panel::update as panel;
use self::service::update as service;
use self::widget::update as widget;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

const TRANSLATIONS: &[(&str, &str)] = &[
    ("notify.title", "Timed"),
    ("notify.body", "{duration} timer finished"),
    ("bar.empty", "No timers"),
    ("bar.timer", "{duration} timer"),
    ("bar.paused", "Paused - {time}"),
    ("settings.show_text.label", "Show time"),
    (
        "settings.show_text.description",
        "Show the estimated remaining time next to the ring.",
    ),
    ("settings.show_count.label", "Show count"),
    (
        "settings.show_count.description",
        "Show the number of timers inside the ring.",
    ),
    ("settings.show_progress.label", "Show progress ring"),
    (
        "settings.show_progress.description",
        "Draw a progress ring for the nearest timer.",
    ),
    ("settings.use_urgency_colors.label", "Timer colors"),
    (
        "settings.use_urgency_colors.description",
        "Switch the ring from the safe color to warning and urgent as the timer runs out.",
    ),
    ("settings.safe_color.label", "Safe color"),
    (
        "settings.safe_color.description",
        "Ring color while more than half the timer remains, or the only color when timer colors are off.",
    ),
    ("settings.warning_color.label", "Warning color"),
    (
        "settings.warning_color.description",
        "Ring color when half or less of the timer remains.",
    ),
    ("settings.urgent_color.label", "Urgent color"),
    (
        "settings.urgent_color.description",
        "Ring color when a fifth or less of the timer remains.",
    ),
    ("settings.idle_color.label", "Idle color"),
    (
        "settings.idle_color.description",
        "Ring color when no timers are running.",
    ),
    ("settings.hide_when_idle.label", "Hide when idle"),
    (
        "settings.hide_when_idle.description",
        "Hide the widget when no timers are running.",
    ),
    ("settings.color.primary", "Primary"),
    ("settings.color.secondary", "Secondary"),
    ("settings.color.tertiary", "Tertiary"),
    ("settings.color.error", "Error"),
    ("settings.color.on_surface", "Text"),
    ("settings.color.on_surface_variant", "Muted text"),
    ("panel.title", "Timed"),
    ("panel.empty", "No timers running"),
    ("panel.timer", "{duration} timer"),
    ("panel.ends_at", "Ends at {time}"),
    ("panel.paused_left", "Paused"),
    ("panel.pause", "Pause"),
    ("panel.resume", "Resume"),
    ("panel.stop", "Stop"),
    ("panel.stop_all", "Stop all"),
    ("panel.placeholder", "Duration, e.g. 1h5 or 5"),
    ("panel.start", "Start"),
    ("panel.invalid", "Invalid duration"),
    ("launcher.hint", "Try 1h5, 1y, 4s10h3m, or 5 for 5 minutes"),
    ("launcher.start", "Start timer {time}"),
    ("launcher.start_new", "Start a new timer"),
    ("launcher.timer", "{duration} timer"),
    ("launcher.message", "{duration} - {message}"),
    ("launcher.stop", "Stop"),
    ("launcher.pause", "Pause"),
    ("launcher.resume", "Resume"),
    ("launcher.no_matching", "No timers here"),
    ("launcher.ends_at", "Ends at {time} - {duration}"),
    ("launcher.paused_at", "Paused - {duration} left"),
    ("launcher.stop_all", "Stop all timers"),
    ("launcher.one_timer", "1 timer"),
    ("launcher.active_count", "{count} timers"),
    ("launcher.invalid", "Invalid duration"),
];

#[must_use]
pub fn build_plugin() -> Plugin {
    let mut plugin = plugin();
    for (key, value) in TRANSLATIONS {
        plugin = plugin.tr(*key, *value);
    }
    plugin
        .asset("README.md", include_bytes!("../assets/README.md"))
        .asset("thumbnail.webp", include_bytes!("../assets/thumbnail.webp"))
        .asset(
            "screenshot.webp",
            include_bytes!("../assets/screenshot.webp"),
        )
}

#[must_use = "the exit code must be returned to the caller"]
pub fn run() -> ExitCode {
    if let Some(code) = emit::run() {
        return code;
    }
    noctalia_rs::run(&build_plugin())
}
