//! End-to-end test of the built plugin: the same spool protocol, entry
//! dispatch, state channel, and persistence the Noctalia bridge uses.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Lines};
use std::path::{Path, PathBuf};
use std::process::{self, Child, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const NOW: i64 = 1_800_000_000_000;

struct Helper {
    child: Child,
    lines: Lines<BufReader<ChildStdout>>,
    boot: String,
    run_dir: PathBuf,
    state: Value,
    sequence: usize,
    response: Value,
    host: Value,
}

#[expect(
    clippy::excessive_nesting,
    reason = "the spool read loops mirror the bridge protocol"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "one argument per request field the helper sends"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the payload is moved into the request"
)]
impl Helper {
    fn start(root: &Path) -> Self {
        let data = root.join("data");
        let spool = root.join("spool");
        fs::create_dir_all(&data).expect("data dir");
        fs::create_dir_all(&spool).expect("spool dir");
        let mut child = Command::new(env!("CARGO_BIN_EXE_timed"))
            .arg("serve")
            .arg(&data)
            .arg(&spool)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .expect("helper");
        let mut lines =
            BufReader::new(child.stdout.take().expect("stdout")).lines();
        let ready: Value = loop {
            let line = lines.next().expect("ready line").expect("line");
            let value: Value = serde_json::from_str(&line).expect("json");
            if value["control"] == json!("ready") {
                break value;
            }
        };
        Self {
            child,
            lines,
            boot: ready["boot"].as_str().expect("boot").to_owned(),
            run_dir: PathBuf::from(ready["spool"].as_str().expect("spool")),
            state: json!({}),
            sequence: 0,
            response: json!({}),
            host: host(),
        }
    }

    fn state(&mut self, state: Value, response: Value) -> &mut Self {
        self.state = state;
        self.response = response;
        self
    }

    fn now(&mut self, now: i64) -> &mut Self {
        self.host = host();
        self.host["now_ms"] = json!(now);
        self
    }

    fn call(&mut self, entry: &str, event: &str, payload: Value) -> Vec<Value> {
        self.sequence = self.sequence.saturating_add(1);
        let request = json!({
            "boot": self.boot,
            "entry": entry,
            "event": event,
            "payload": payload,
            "config": config(entry),
            "state": self.state,
            "host": self.host.clone(),
        });
        let name = format!("{:012}", self.sequence);
        let target = self.run_dir.join(format!("{name}.json"));
        let temporary = self.run_dir.join(format!("{name}.pending"));
        fs::write(&temporary, request.to_string()).expect("request");
        fs::rename(&temporary, &target).expect("rename");

        let mut ops = Vec::new();
        loop {
            let line = self.lines.next().expect("line").expect("read");
            let value: Value = serde_json::from_str(&line).expect("json");
            if let Some(control) = value.get("control").and_then(Value::as_str)
            {
                match control {
                    "done" => return ops,
                    "error" => {
                        panic!("helper error: {}", value["message"]);
                    }
                    _ => continue,
                }
            }
            if value["op"] == json!("query") {
                self.answer(&value);
                continue;
            }
            ops.push(value);
        }
    }

    fn answer(&self, query: &Value) {
        let path = query["response"].as_str().expect("response path");
        fs::write(
            path,
            json!({ "ok": true, "value": self.response }).to_string(),
        )
        .expect("answer");
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn config(entry: &str) -> Value {
    if entry == "timed_indicator" {
        return json!({
            "show_text": true,
            "show_count": true,
            "show_progress": true,
            "hide_when_idle": false,
            "use_urgency_colors": true,
            "safe_color": "tertiary",
            "warning_color": "secondary",
            "urgent_color": "error",
            "idle_color": "primary",
        });
    }
    json!({})
}

fn host() -> Value {
    json!({
        "now_ms": NOW,
        "pluginDir": "/plugin",
        "pluginDataDir": "/data",
        "bar": { "vertical": false, "output_name": "DP-1" },
        "colors": {
            "primary": "#CBA6F7",
            "secondary": "#F9E2AF",
            "tertiary": "#8BD5CA",
            "error": "#F38BA8",
            "on_surface": "#CDD6F4",
            "on_surface_variant": "#A6ADC8",
            "outline_variant": "#4C4F69"
        },
        "setting": {
            "shell.font_family": "sans-serif",
            "shell.time_format": "%H:%M",
            "shell.date_format": "%Y-%m-%d"
        }
    })
}

fn timers(ops: &[Value]) -> Option<Value> {
    ops.iter()
        .find(|op| {
            op["op"] == json!("state_set") && op["key"] == json!("timed.state")
        })
        .and_then(|op| op.get("value"))
        .and_then(|value| value.get("timers"))
        .cloned()
}

fn fixture() -> Value {
    json!({
        "timers": [
            {
                "id": "one",
                "seconds": 600,
                "label": "10m",
                "message": "focus",
                "paused": false,
                "endsAt": NOW + 600_000
            },
            {
                "id": "two",
                "seconds": 1800,
                "label": "30m",
                "paused": true,
                "remainingMs": 900_000
            }
        ]
    })
}

fn scratch(name: &str) -> PathBuf {
    let path =
        env::temp_dir().join(format!("timed-flow-{}-{name}", process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("scratch");
    path
}

#[test]
fn service_publishes_persists_and_notifies() {
    let root = scratch("service");
    let mut helper = Helper::start(&root);

    let ops = helper.call("service", "load", json!({ "source": "load" }));
    assert_eq!(timers(&ops), Some(json!([])));
    assert!(
        ops.iter().any(|op| op["op"] == json!("set_interval")
            && op["ms"] == json!(86_400_000))
    );
    assert!(ops.iter().any(|op| op["op"] == json!("host_call")));

    let command = json!({
        "action": "start",
        "seconds": 300,
        "message": "tea",
        "n": NOW
    });
    let ops = helper
        .state(
            json!({ "timed.command": command, "timed.lastCommand": NOW }),
            json!(null),
        )
        .call(
            "service",
            "host_callback",
            json!({ "id": "command", "args": [command] }),
        );
    let published = timers(&ops).expect("published");
    assert_eq!(published[0]["label"], json!("5m"));
    assert_eq!(published[0]["message"], json!("tea"));
    assert_eq!(published[0]["seconds"], json!(300));
    assert!(ops.iter().any(|op| op["op"] == json!("log")
        && op["message"] == json!("timed: started 1800000000000 for 5m")));
    assert!(
        ops.iter().any(|op| op["op"] == json!("set_interval")
            && op["ms"] == json!(300_050))
    );

    let saved: Value = serde_json::from_str(
        &fs::read_to_string(root.join("data/timers.json")).expect("saved"),
    )
    .expect("json");
    assert_eq!(saved["timers"][0]["id"], json!("1800000000000"));

    // A shell restart reloads the saved timer and does not start it again.
    drop(helper);
    let mut helper = Helper::start(&root);
    let ops = helper
        .state(
            json!({ "timed.command": command, "timed.lastCommand": NOW }),
            json!(null),
        )
        .call("service", "load", json!({ "source": "load" }));
    assert_eq!(timers(&ops).expect("reloaded")[0]["id"], published[0]["id"]);
    assert!(!ops.iter().any(|op| op["op"] == json!("log")));

    // The timer expires: the tick notifies with the stored message.
    let expired = json!({
        "timers": [{
            "id": "old",
            "seconds": 300,
            "label": "5m",
            "message": "tea",
            "paused": false,
            "endsAt": NOW - 1_000
        }]
    });
    fs::write(root.join("data/timers.json"), expired.to_string())
        .expect("write");
    drop(helper);
    let mut helper = Helper::start(&root);
    let ops = helper.state(json!({}), json!(null)).call(
        "service",
        "load",
        json!({ "source": "load" }),
    );
    assert!(ops.iter().any(|op| op["op"] == json!("notify")
        && op["title"] == json!("Timed")
        && op["body"] == json!("tea")));
    assert!(ops.iter().any(|op| op["op"] == json!("log")
        && op["message"] == json!("timed: finished old")));
    assert_eq!(timers(&ops), Some(json!([])));

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ipc_controls_the_timer_list() {
    let root = scratch("ipc");
    let mut helper = Helper::start(&root);
    helper.state(json!({}), json!(null)).call(
        "service",
        "load",
        json!({ "source": "load" }),
    );

    let ops = helper.state(json!({}), json!(null)).call(
        "service",
        "ipc",
        json!({ "event": "start", "payload": "1h5 focus" }),
    );
    let published = timers(&ops).expect("published");
    assert_eq!(published[0]["label"], json!("1h 5m"));
    assert_eq!(published[0]["message"], json!("focus"));

    let ops = helper.state(json!({}), json!(null)).call(
        "service",
        "ipc",
        json!({ "event": "list", "payload": "" }),
    );
    let messages = ops
        .iter()
        .filter_map(|op| op["message"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(messages[0], "timed: 1 timer(s)");
    assert!(
        messages[1].ends_with("running 1:05:00 left"),
        "{messages:?}"
    );

    let id = published[0]["id"].as_str().expect("id").to_owned();
    let ops = helper.state(json!({}), json!(null)).call(
        "service",
        "ipc",
        json!({ "event": "pause", "payload": id }),
    );
    let published = timers(&ops).expect("paused");
    assert_eq!(published[0]["paused"], json!(true));
    assert!(published[0].get("endsAt").is_none());

    let ops = helper.state(json!({}), json!(null)).call(
        "service",
        "ipc",
        json!({ "event": "stop-all", "payload": "" }),
    );
    assert_eq!(timers(&ops), Some(json!([])));

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ipc_expires_finished_timers_first() {
    let root = scratch("ipc-expire");
    let mut helper = Helper::start(&root);
    helper
        .state(json!({}), json!(null))
        .call("service", "load", json!({}));

    let ops = helper.state(json!({}), json!(null)).call(
        "service",
        "ipc",
        json!({ "event": "start", "payload": "1s tea" }),
    );
    let id = timers(&ops).expect("published")[0]["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let ops = helper.now(NOW + 2_000).state(json!({}), json!(null)).call(
        "service",
        "ipc",
        json!({ "event": "list", "payload": "" }),
    );
    assert!(
        ops.iter().any(|op| op["op"] == json!("notify")
            && op["body"] == json!("tea")),
        "{ops:?}"
    );
    assert!(ops.iter().any(|op| op["op"] == json!("log")
        && op["message"] == json!(format!("timed: finished {id}"))));
    let messages = ops
        .iter()
        .filter_map(|op| op["message"].as_str())
        .collect::<Vec<_>>();
    assert!(messages.contains(&"timed: 0 timer(s)"), "{messages:?}");
    assert_eq!(timers(&ops), Some(json!([])));

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn widget_renders_the_published_list() {
    let root = scratch("ui-widget");
    let mut helper = Helper::start(&root);

    let ops = helper.state(fixture(), json!(null)).call(
        "timed_indicator",
        "render",
        json!({ "source": "load" }),
    );
    assert!(
        ops.iter().any(|op| op["op"] == json!("bar_visible")
            && op["visible"] == json!(true))
    );
    assert!(
        ops.iter()
            .any(|op| op["op"] == json!("bar_text")
                && op["text"] == json!("10m"))
    );
    assert!(ops.iter().any(|op| op["op"] == json!("bar_image")));
    assert!(ops.iter().any(|op| op["op"] == json!("bar_tooltip")
        && op["value"][1]["value"] == json!("Paused - 15m")));
    assert!(
        ops.iter()
            .any(|op| op["op"] == json!("set_interval")
                && op["ms"] == json!(1000))
    );
    let image = ops
        .iter()
        .find(|op| op["op"] == json!("bar_image"))
        .and_then(|op| op["path"].as_str())
        .expect("ring path")
        .to_owned();
    assert!(Path::new(&image).exists(), "{image}");

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn panel_renders_the_published_list() {
    let root = scratch("ui-panel");
    let mut helper = Helper::start(&root);

    let ops = helper
        .state(
            fixture(),
            json!({
                "safe": "tertiary",
                "warning": "secondary",
                "urgent": "error",
                "idle": "primary",
                "useUrgency": true
            }),
        )
        .call("panel", "open", json!({ "context": null }));
    let tree = ops
        .iter()
        .find(|op| op["op"] == json!("panel_render"))
        .expect("panel render");
    let text = tree["tree"].to_string();
    assert!(text.contains("focus"), "{text}");
    assert!(text.contains("Duration - e.g. 5m or 1h5"), "{text}");
    assert!(text.contains("Ends at"), "{text}");

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn panel_reports_an_invalid_duration() {
    let root = scratch("ui-panel-error");
    let mut helper = Helper::start(&root);
    let colors = json!({
        "safe": "tertiary",
        "warning": "secondary",
        "urgent": "error",
        "idle": "primary",
        "useUrgency": true
    });
    helper.state(fixture(), colors.clone()).call(
        "panel",
        "open",
        json!({ "context": null }),
    );

    let ops = helper.state(fixture(), colors).call(
        "panel",
        "action",
        json!({ "name": "submit", "args": ["nope"] }),
    );
    let tree = ops
        .iter()
        .find(|op| op["op"] == json!("panel_render"))
        .expect("panel render");
    let text = tree["tree"].to_string();
    assert!(text.contains("Invalid duration"), "{text}");

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn launcher_queries_and_starts_timers() {
    let root = scratch("ui-launcher");
    let mut helper = Helper::start(&root);

    let ops = helper.state(fixture(), json!(null)).call(
        "launcher",
        "query",
        json!({ "text": "1h5 wake up" }),
    );
    let results = ops
        .iter()
        .find(|op| op["op"] == json!("launcher_results"))
        .expect("results");
    assert_eq!(results["text"], json!("1h5 wake up"));
    assert_eq!(results["rows"][0]["id"], json!("start:3900:wake up"));

    let ops = helper.state(fixture(), json!(null)).call(
        "launcher",
        "activate",
        json!({ "id": "start:3900:wake up" }),
    );
    let command = ops
        .iter()
        .find(|op| {
            op["op"] == json!("state_set")
                && op["key"] == json!("timed.command")
        })
        .expect("command");
    assert_eq!(command["value"]["action"], json!("start"));
    assert_eq!(command["value"]["seconds"], json!(3900));
    assert_eq!(command["value"]["message"], json!("wake up"));

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn clean_exit_removes_the_ring() {
    let root = scratch("exit");
    let mut helper = Helper::start(&root);
    let list = json!({ "timers": [{
        "id": "one",
        "seconds": 600,
        "label": "10m",
        "paused": false,
        "endsAt": NOW + 600_000
    }] });
    let ops = helper.state(list.clone(), json!(null)).call(
        "timed_indicator",
        "render",
        json!({ "source": "load" }),
    );
    let image = ops
        .iter()
        .find(|op| op["op"] == json!("bar_image"))
        .and_then(|op| op["path"].as_str())
        .expect("ring path")
        .to_owned();
    assert!(Path::new(&image).exists());

    helper.state(list, json!(null)).call(
        "timed_indicator",
        "exit",
        json!({ "signal": 15, "reason": "quit" }),
    );
    assert!(!Path::new(&image).exists());

    drop(helper);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn state_channel_keys_are_stable() {
    let root = scratch("keys");
    let mut helper = Helper::start(&root);
    let ops = helper.state(json!({}), json!(null)).call(
        "service",
        "load",
        json!({ "source": "load" }),
    );
    let watch = ops
        .iter()
        .find(|op| op["op"] == json!("host_call"))
        .expect("watch");
    assert_eq!(watch["path"], json!(["state", "watch"]));
    assert_eq!(watch["args"][0], json!("timed.command"));
    assert_eq!(
        ops.iter()
            .find(|op| op["op"] == json!("state_set"))
            .expect("state")["key"],
        json!("timed.state")
    );
    drop(helper);
    let _ = fs::remove_dir_all(root);
}
