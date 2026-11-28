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

const ENCODE_REQUEST: &str = r#"local function encodeRequest(request)
  local head = noctalia.json.encode({
    boot = request.boot,
    entry = request.entry,
    event = request.event,
    payload = request.payload or {},
  })
  local host = noctalia.json.encode(M.host)
  if type(head) == "string" and type(host) == "string"
    and head:sub(1, 1) == "{" and head:sub(-1, -1) == "}"
    and host:sub(1, 1) == "{" and host:sub(-1, -1) == "}" then
    return string.sub(head, 1, -2)
      .. ',"config":' .. CONFIG_JSON
      .. ',"state":' .. STATE_JSON
      .. ',"host":' .. host .. "}"
  end
  return noctalia.json.encode(encodeBytes(request))
end"#;

const SNAPSHOT: &str = r#"local function snapshot(event)
  local fixed = {
    setting = {},
    colors = {},
  }
  if M.entry == "timed_indicator" then
    fixed.bar = {
      vertical = invoke(barWidget, "isVertical"),
      output_name = invoke(barWidget, "outputName"),
    }
  end
  for _, path in ipairs(SETTING_PATHS) do
    fixed.setting[path] = invoke(noctalia, "getSetting", path)
  end
  for _, role in ipairs(COLOR_ROLES) do
    fixed.colors[role] = invoke(noctalia, "getColor", role)
  end
  local live = {
    now_ms = invoke(noctalia, "nowMs") or 0,
    timed_state_version = STATE_VERSION,
  }
  if event == "load" or event == "activate" or event == "action"
    or event == "right_click" then
    live.timed_last_command = noctalia.state.get("timed.lastCommand")
  end
  if M.entry == "service" and event == "load" then
    live.timed_command = noctalia.state.get("timed.command")
  end
  if M.entry == "panel" and event == "open" then
    live.timed_colors = noctalia.state.get("timed.colors")
  end
  return live, fixed
end"#;

const WRITE_REQUEST: &str = r#"local function writeRequest(request)
  sequence += 1
  local target = REQUEST_DIR .. "/" .. string.format("%012d", sequence) .. ".json"
  local live, fixed = snapshot(request.event)
  for key, value in pairs(live) do
    fixed[key] = value
  end
  M.host = fixed
  request.host = M.host
  local ok, err = noctalia.writeFile(target, encodeRequest(request))
  if not ok then
    noctalia.removeFile(target)
    noctalia.log("noctalia bridge: spool write failed: " .. tostring(err))
  end
end"#;

const ENTRY_SNAPSHOT: &str = r#"  if entry == "timed_indicator" then
    SETTING_PATHS = { "shell.font_family" }
  elseif entry == "panel" or entry == "launcher" then
    SETTING_PATHS = { "shell.time_format", "shell.date_format" }
    COLOR_ROLES = {}
  else
    SETTING_PATHS = {}
    COLOR_ROLES = {}
  end"#;
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

#[expect(
    clippy::too_many_lines,
    reason = "one structural pass prunes the generated bridge"
)]
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
    fixed = fixed
        .replace(
            "noctalia.json.encode(encodeBytes(response))",
            "noctalia.json.encode(response)",
        )
        .replace("local fixed_host = nil\nlocal fixed_sent = false\n", "")
        .replace("    -- A fresh helper has no cached fixed snapshot.\n", "")
        .replace(
            "    -- The helper may have rejected the request that carried the fixed part,\n    -- so it has to travel again with the next one.\n",
            "",
        )
        .replace("    fixed_sent = false\n", "");
    replace_function(&mut fixed, "local function sameValue", "\nend\n", "")?;
    replace_function(&mut fixed, "local function mergeHost", "\nend\n", "")?;
    replace_function(
        &mut fixed,
        "local function snapshot()",
        "\nend\n",
        SNAPSHOT,
    )?;
    replace_function(
        &mut fixed,
        "local function writeRequest(request)",
        "\nend\n",
        &format!("{ENCODE_REQUEST}\n\n{WRITE_REQUEST}"),
    )?;
    fixed = fixed.replace(
        "  M.entry = entry\n",
        &format!("  M.entry = entry\n{ENTRY_SNAPSHOT}\n"),
    );
    fixed = fixed.replace(
        "  local host, fixed = snapshot()\n  M.host = mergeHost(fixed, host)\n  startStream()",
        "  startStream()",
    );
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
    Ok(fixed)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the source, function markers, and replacement text"
)]
fn replace_function(
    source: &mut String,
    start_marker: &str,
    end_marker: &str,
    replacement: &str,
) -> io::Result<()> {
    let Some(start) = source.find(start_marker) else {
        return Err(io::Error::other(format!("cannot find {start_marker}")));
    };
    let Some(offset) = source[start..].find(end_marker) else {
        return Err(io::Error::other(format!(
            "cannot find the end of {start_marker}"
        )));
    };
    let end = start.saturating_add(offset).saturating_add(4);
    source.replace_range(start..end, replacement);
    Ok(())
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

    fn bridge_source() -> String {
        r#"local COLOR_ROLES = {
  "surface_variant",
  "outline",
  "primary",
  "shadow",
}
local SETTING_PATHS = {
  "shell.time_format",
  "shell.date_format",
  "shell.lang",
  "bar",
}
M.state = {}
local fixed_host = nil
local fixed_sent = false
local function sameValue(left, right)
  return left == right
end
local function mergeHost(fixed, live)
  return fixed
end
local function snapshot()
  local host = {
    now_ms = invoke(noctalia, "nowMs") or 0,
    dark = invoke(noctalia, "isDarkMode"),
    pluginDir = invoke(noctalia, "pluginDir"),
    setting = {},
    bar = {},
    colors = {},
  }
  return host, host
end
local function writeRequest(request)
  local host, fixed = snapshot()
  M.host = mergeHost(fixed, host)
  request.host = host
  request.host_fixed = fixed
  local ok, err = noctalia.writeFile(target, noctalia.json.encode(encodeBytes(request)))
  if not ok then
    noctalia.log(err)
  end
end
function M.setup(entry, configKeys)
  M.state = noctalia.state.get(STATE_KEY) or {}
  M.entry = entry
  local host, fixed = snapshot()
  M.host = mergeHost(fixed, host)
  startStream()
end
function M.watch_state(rerender)
  noctalia.state.watch(STATE_KEY, function(value)
    M.state = value or {}
    rerender()
  end)
end
noctalia.json.encode(encodeBytes(response))
"#
        .to_owned()
    }

    #[test]
    fn prune_bridge_keeps_the_settings_the_plugin_reads() {
        let pruned = prune_bridge(&bridge_source()).expect("pruned");
        assert!(pruned.contains(
            "local SETTING_PATHS = {\n  \"shell.time_format\",\n  \"shell.font_family\",\n  \"shell.date_format\",\n}"
        ));
        assert!(!pruned.contains("shell.lang"));
        assert!(pruned.contains(ENTRY_SNAPSHOT));
    }

    #[test]
    fn prune_bridge_snapshots_only_what_each_entry_reads() {
        let pruned = prune_bridge(&bridge_source()).expect("pruned");
        assert!(pruned.contains(USED_COLORS));
        assert!(!pruned.contains("shadow"));
        assert!(!pruned.contains("isDarkMode"));
        assert!(!pruned.contains("pluginDir = invoke"));
        assert!(pruned.contains(SNAPSHOT));
        assert_eq!(pruned.matches("snapshot(request.event)").count(), 1);
        assert!(!pruned.contains("local host, fixed = snapshot()"));
        assert!(pruned.contains("M.entry = entry\n"));
        assert!(pruned.contains("  startStream()"));
    }

    #[test]
    fn prune_bridge_encodes_json_values_without_copying_them() {
        let pruned = prune_bridge(&bridge_source()).expect("pruned");
        assert!(pruned.contains(ENCODE_REQUEST));
        assert!(pruned.contains(WRITE_REQUEST));
        assert_eq!(pruned.matches("encodeBytes(request)").count(), 1);
        assert!(pruned.contains("noctalia.json.encode(response)"));
        assert!(!pruned.contains("host_fixed"));
        assert!(!pruned.contains("sameValue"));
        assert!(!pruned.contains("mergeHost"));
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
        assert!(pruned.contains("live.timed_last_command"));
        assert!(pruned.contains("live.timed_command"));
        assert!(pruned.contains("live.timed_colors"));
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
