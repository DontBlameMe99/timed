pub mod duration;
mod emit;
mod file;
mod i18n;
pub mod launcher;
mod math;
pub mod panel;
pub mod service;
mod sync;
pub mod timer;
mod translations;
pub mod urgency;
pub mod widget;

#[cfg(test)]
mod test_context;

use std::process::ExitCode;

use noctalia_rs::Plugin;

use self::launcher::global as launcher_global;
use self::launcher::update as launcher;
use self::panel::update as panel;
use self::service::update as service;
use self::translations::TRANSLATIONS;
use self::widget::update as widget;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

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
