//! `emit`, with the generated bridge trimmed to what this plugin reads.
//!
//! noctalia-rs snapshots retired color roles and many shell settings in its
//! bridge. The plugin validator scans every `.luau` file for the retired roles,
//! and the host warns on every event for settings that do not exist in the
//! active shell. Neither is needed here, so `emit` removes them. Drop this
//! module once noctalia-rs stops listing them.

use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use crate::build_plugin;

const RETIRED: [&str; 2] = ["surface_variant", "outline"];

/// Only the font and the two time formats are read from the host snapshot.
const UNUSED_SETTINGS: [&str; 15] = [
    "shell.offline_mode",
    "shell.lang",
    "shell.language",
    "shell.corner_radius_scale",
    "shell.show_close_buttons",
    "general.language",
    "bar",
    "bar.order",
    "widget",
    "desktop_widgets",
    "desktop_widgets.widget",
    "theme.source",
    "theme.pure_black_dark",
    "theme.wallpaper_scheme",
    "system.monitor.network_poll_seconds",
];

pub fn run() -> Option<ExitCode> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("emit") {
        return None;
    }
    let Some(dir) = args.next() else {
        return Some(failure("emit needs a target directory"));
    };
    let Some(helper) = args.next() else {
        return Some(failure("emit needs a helper name"));
    };

    Some(match emit(Path::new(&dir), &helper) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failure(&error.to_string()),
    })
}

fn failure(message: &str) -> ExitCode {
    eprintln!("noctalia: {message}");
    ExitCode::FAILURE
}

fn emit(dir: &Path, helper: &str) -> io::Result<()> {
    build_plugin().emit(dir, helper)?;

    let path = dir.join("lib/bridge.luau");
    let source = fs::read_to_string(&path)?;
    let fixed = prune_bridge(&source)?;
    if fixed != source {
        fs::write(&path, fixed)?;
    }

    Ok(())
}

fn prune_bridge(source: &str) -> io::Result<String> {
    let mut fixed = source.to_owned();
    for role in RETIRED {
        fixed = fixed.replace(&format!("  \"{role}\",\n"), "");
        if fixed.contains(&format!("\"{role}\"")) {
            return Err(io::Error::other(format!(
                "the bridge still lists {role}"
            )));
        }
    }
    for path in UNUSED_SETTINGS {
        fixed = fixed.replace(&format!("  \"{path}\",\n"), "");
        if fixed.contains(&format!("\"{path}\"")) {
            return Err(io::Error::other(format!(
                "the bridge still reads {path}"
            )));
        }
    }
    Ok(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_bridge_keeps_the_settings_the_plugin_reads() {
        let source = "local SETTING_PATHS = {\n  \"shell.time_format\",\n  \"shell.lang\",\n  \"bar\",\n}\n";
        assert_eq!(
            prune_bridge(source).expect("pruned"),
            "local SETTING_PATHS = {\n  \"shell.time_format\",\n}\n"
        );
    }
}
