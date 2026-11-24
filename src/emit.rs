//! `emit`, with the generated bridge trimmed to what this plugin reads.
//!
//! noctalia-rs snapshots retired color roles and many shell settings in its
//! bridge. The plugin validator scans every `.luau` file for the retired roles,
//! and the host warns on every event for settings that do not exist in the
//! active shell. Neither is needed here, so `emit` removes them. It also keeps
//! only the host fields Timed reads and includes the font in the snapshot so
//! the widget cache works without a synchronous query. Drop this module once
//! noctalia-rs stops listing them.

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

const USED_COLORS: &str = "local COLOR_ROLES = {
  \"primary\",
  \"secondary\",
  \"tertiary\",
  \"error\",
  \"on_surface\",
  \"on_surface_variant\",
  \"outline_variant\",
}";

const UNUSED_HOST_FIELDS: [&str; 11] = [
    "dark = invoke(noctalia, \"isDarkMode\"),",
    "outputs = invoke(noctalia, \"outputs\"),",
    "focused_output_name = invoke(noctalia, \"focusedOutputName\"),",
    "pluginDir = invoke(noctalia, \"pluginDir\"),",
    "pluginDataDir = invoke(noctalia, \"pluginDataDir\"),",
    "clipboard = invoke(noctalia, \"clipboardText\"),",
    "calendar = invoke(noctalia, \"calendarEvents\"),",
    "media = invoke(noctalia, \"mediaPlayer\"),",
    "stats = invoke(noctalia, \"systemStats\"),",
    "nightlight = invoke(noctalia, \"nightlight.state\"),",
    "compositor = { name = invoke(noctalia, \"compositor.name\") },",
];

const REQUEST_ENCODING: &str = r#"  local field_count = 0
  for _ in pairs(request) do field_count += 1 end
  local head = noctalia.json.encode({
    boot = request.boot,
    entry = request.entry,
    event = request.event,
    payload = request.payload or {},
  }) or "{}"
  local host = noctalia.json.encode(M.host) or "{}"
  local text
  if field_count == 7
    and head:sub(1, 1) == "{" and head:sub(-1) == "}"
    and host:sub(1, 1) == "{" and host:sub(-1) == "}" then
    text = string.sub(head, 1, -2)
      .. ',"config":' .. CONFIG_JSON
      .. ',"state":' .. STATE_JSON
      .. ',"host":' .. host .. "}"
  else
    text = noctalia.json.encode(encodeBytes(request))
  end
  local ok, err = noctalia.writeFile(temporary, text)"#;

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
    let panel = dir.join("panel.luau");
    let source = fs::read_to_string(&panel)?;
    let fixed = prune_panel(&source)?;
    if fixed != source {
        fs::write(&panel, fixed)?;
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
    let time_format = "  \"shell.time_format\",\n";
    if fixed.contains(time_format)
        && !fixed.contains("  \"shell.font_family\",\n")
    {
        fixed = fixed.replace(
            time_format,
            &format!("{time_format}  \"shell.font_family\",\n"),
        );
    }
    if let Some(start) = fixed.find("local COLOR_ROLES = {") {
        let Some(offset) = fixed[start..].find("\n}\n") else {
            return Err(io::Error::other("cannot find the end of COLOR_ROLES"));
        };
        let end = start.saturating_add(offset).saturating_add(3);
        fixed.replace_range(start..end, &format!("{USED_COLORS}\n"));
    }
    for field in UNUSED_HOST_FIELDS {
        fixed = fixed.replace(&format!("    {field}\n"), "");
    }
    fixed = fixed
        .replace(
            "  local ok, err = noctalia.writeFile(temporary, noctalia.json.encode(encodeBytes(request)))",
            REQUEST_ENCODING,
        )
        .replace(
            "noctalia.json.encode(encodeBytes(response))",
            "noctalia.json.encode(response)",
        )
        .replace("  M.host = snapshot()\n  startStream()", "  startStream()");
    fixed = fixed.replacen(
        "M.state = {}\n",
        "M.state = {}\nlocal CONFIG_JSON = \"{}\"\nlocal STATE_JSON = \"{}\"\nlocal STATE_VERSION = 0\n",
        1,
    );
    fixed = fixed.replace(
        "  M.state = noctalia.state.get(STATE_KEY) or {}\n",
        "  M.state = noctalia.state.get(STATE_KEY) or {}\n  STATE_VERSION += 1\n  CONFIG_JSON = noctalia.json.encode(M.config) or \"{}\"\n  STATE_JSON = noctalia.json.encode(M.state) or \"{}\"\n",
    );
    fixed = fixed.replace(
        "    M.state = value or {}\n",
        "    M.state = value or {}\n    STATE_VERSION += 1\n    STATE_JSON = noctalia.json.encode(M.state) or \"{}\"\n",
    );
    fixed = fixed.replace(
        "    now_ms = invoke(noctalia, \"nowMs\") or 0,\n",
        "    now_ms = invoke(noctalia, \"nowMs\") or 0,\n    timed_state_version = STATE_VERSION,\n",
    );
    Ok(fixed)
}

fn prune_panel(source: &str) -> io::Result<String> {
    let fixed = source
        .replace(
            "\npanel.setWantsSecondTicks(true)\nbridge.call(\"render\"",
            "\nbridge.call(\"render\"",
        )
        .replace(
            "function onOpen(context) bridge.call(\"open\", { context = context }) end",
            "function onOpen(context)\n  panel.setWantsSecondTicks(true)\n  bridge.call(\"open\", { context = context })\nend",
        )
        .replace(
            "function onClose() bridge.call(\"close\") end",
            "function onClose()\n  panel.setWantsSecondTicks(false)\n  bridge.call(\"close\")\nend",
        );
    if fixed.contains("panel.setWantsSecondTicks(true)")
        && fixed
            .contains("panel.setWantsSecondTicks(true)\nbridge.call(\"render\"")
    {
        return Err(io::Error::other("the panel still ticks while closed"));
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
            "local SETTING_PATHS = {\n  \"shell.time_format\",\n  \"shell.font_family\",\n}\n"
        );
    }
    #[test]
    fn prune_bridge_keeps_only_the_host_fields_timed_reads() {
        let source = "local COLOR_ROLES = {\n  \"surface_variant\",\n  \"outline\",\n  \"primary\",\n  \"shadow\",\n}\nlocal SETTING_PATHS = {\n  \"shell.time_format\",\n}\nlocal function snapshot()\n  local host = {\n    now_ms = invoke(noctalia, \"nowMs\") or 0,\n    dark = invoke(noctalia, \"isDarkMode\"),\n    outputs = invoke(noctalia, \"outputs\"),\n    pluginDir = invoke(noctalia, \"pluginDir\"),\n    setting = {},\n    bar = {},\n    colors = {},\n  }\nend\nlocal function writeRequest(request)\n  M.host = snapshot()\nend\nfunction M.setup(entry, configKeys)\n  M.host = snapshot()\n  startStream()\nend\n";
        let pruned = prune_bridge(source).expect("pruned");
        assert!(pruned.contains(USED_COLORS));
        assert!(!pruned.contains("shadow"));
        assert!(!pruned.contains("isDarkMode"));
        assert!(!pruned.contains("pluginDir = invoke"));
        assert!(pruned.contains("\"shell.font_family\""));
        assert!(pruned.contains("timed_state_version = STATE_VERSION"));
        assert!(pruned.contains(
            "local function writeRequest(request)\n  M.host = snapshot()"
        ));
        assert!(
            pruned.contains(
                "function M.setup(entry, configKeys)\n  startStream()"
            )
        );
    }

    #[test]
    fn prune_bridge_encodes_json_values_without_copying_them() {
        let source = "  local ok, err = noctalia.writeFile(temporary, noctalia.json.encode(encodeBytes(request)))\nnoctalia.json.encode(encodeBytes(response))\nM.state = {}\n  M.state = noctalia.state.get(STATE_KEY) or {}\n    M.state = value or {}\n";
        let pruned = prune_bridge(source).expect("pruned");
        assert!(pruned.contains(REQUEST_ENCODING));
        assert!(pruned.contains("noctalia.json.encode(response)"));
        assert!(pruned.contains("local STATE_VERSION = 0"));
        assert!(pruned.contains("local CONFIG_JSON = \"{}\""));
        assert!(
            pruned.contains("CONFIG_JSON = noctalia.json.encode(M.config)")
        );
        assert_eq!(
            pruned
                .matches("STATE_JSON = noctalia.json.encode(M.state)")
                .count(),
            2
        );
        assert_eq!(pruned.matches("STATE_VERSION += 1").count(), 2);
    }

    #[test]
    fn prune_panel_ticks_only_while_open() {
        let source = "function onOpen(context) bridge.call(\"open\", { context = context }) end\nfunction onClose() bridge.call(\"close\") end\npanel.setWantsSecondTicks(true)\nbridge.call(\"render\", { source = \"load\" })\n";
        let pruned = prune_panel(source).expect("pruned");
        assert!(pruned.contains(
            "function onOpen(context)\n  panel.setWantsSecondTicks(true)"
        ));
        assert!(pruned.contains(
            "function onClose()\n  panel.setWantsSecondTicks(false)"
        ));
        assert!(
            !pruned.contains("panel.setWantsSecondTicks(true)\nbridge.call")
        );
    }
}
