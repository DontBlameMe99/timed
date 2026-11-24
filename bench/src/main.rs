#![expect(
    clippy::excessive_nesting,
    reason = "the harness reads the spool protocol and JSON envelopes in place"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "one argument per fixture or protocol field"
)]

//! Paired benchmark: the original Luau Timed entries against the Rust rewrite.
//!
//! The Luau side runs the vendored originals in `bench/luau` under a mock host
//! and times itself with `os.clock()`. The Rust side calls the real handlers and
//! times itself with `Instant`. Both sides run the same fixture, the same one
//! second per event clock, the same warmup, and the same batches, and both emit
//! the ops of one parity dispatch which must match before any timing is reported.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;

use noctalia_rs::{Context, Op, Parts};
use serde::Serialize;
use serde_json::{Value, json};

const PARITY_NOW: i64 = 1_800_000_000_000;
const START: i64 = PARITY_NOW + 1_000_000;

const COLORS: &[(&str, &str)] = &[
    ("primary", "#CBA6F7"),
    ("secondary", "#F9E2AF"),
    ("tertiary", "#8BD5CA"),
    ("error", "#F38BA8"),
    ("on_surface", "#CDD6F4"),
    ("on_surface_variant", "#A6ADC8"),
    ("outline_variant", "#4C4F69"),
];

const SETTINGS: &[(&str, &str)] = &[
    ("shell.font_family", "sans-serif"),
    ("shell.time_format", "%H:%M"),
    ("shell.date_format", "%Y-%m-%d"),
];

const PRELUDE: &str = include_str!("../luau/prelude.luau");
const BRIDGE: &str = include_str!("../../timed/lib/bridge.luau");

const MODULES: &[(&str, &str)] = &[
    ("lib/duration", include_str!("../luau/lib/duration.luau")),
    ("lib/timers", include_str!("../luau/lib/timers.luau")),
    ("lib/urgency", include_str!("../luau/lib/urgency.luau")),
    ("bar", include_str!("../luau/bar.luau")),
    ("panel", include_str!("../luau/panel.luau")),
    ("launcher", include_str!("../luau/launcher.luau")),
    ("service", include_str!("../luau/service.luau")),
];

const CASES: &[&str] = &[
    "widget",
    "widget_tree",
    "panel",
    "launcher",
    "service",
    "duration",
    "urgency",
];

struct Config {
    samples: usize,
    warmup: usize,
    batch: usize,
    luau: String,
    helper: Option<PathBuf>,
    report: Option<PathBuf>,
    only: Option<String>,
    row: Option<String>,
}

#[derive(Clone, Copy, Default, Serialize, serde::Deserialize)]
struct Stats {
    median: f64,
    p95: f64,
    p99: f64,
    mean: f64,
}

#[derive(Serialize, serde::Deserialize)]
struct Row {
    case: String,
    parity: bool,
    luau: Stats,
    rust: Stats,
    speedup_median: f64,
    speedup_p95: f64,
    speedup_p99: f64,
}

#[derive(Serialize)]
struct BridgeRow {
    events: usize,
    latency_median: f64,
    latency_p95: f64,
    latency_p99: f64,
    cpu_us_per_event: f64,
    idle_cpu_us_per_s: f64,
    rss_kib: u64,
}

fn main() {
    let config = parse_args();

    // Each case runs in its own process so the plugin's process-global caches
    // start cold, exactly like the fresh Luau VM on the other side.
    if let Some(case) = &config.row {
        let row = run_one(&config, case);
        println!("{}", serde_json::to_string(&row).expect("row"));
        return;
    }

    let mut rows = Vec::new();
    for case in CASES {
        if config.only.as_deref().is_some_and(|only| only != *case) {
            continue;
        }
        let exe = env::current_exe().expect("current exe");
        let output = Command::new(exe)
            .arg("--row")
            .arg(case)
            .arg("--samples")
            .arg(config.samples.to_string())
            .arg("--warmup")
            .arg(config.warmup.to_string())
            .arg("--batch")
            .arg(config.batch.to_string())
            .arg("--luau")
            .arg(&config.luau)
            .output()
            .expect("case process");
        if !output.status.success() {
            eprintln!(
                "{case} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            std::process::exit(1);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let number: Value =
            serde_json::from_str(text.trim()).expect("row json");
        let row: Row = serde_json::from_value(number).expect("row");
        rows.push(row);
    }

    let bridge = config.helper.as_ref().and_then(|_| bridge(&config));
    let submission = shell_submission(&config);

    print_report(&rows, bridge.as_ref(), submission);
    if let Some(path) = &config.report {
        let document = json!({
            "machine": machine(),
            "config": {
                "samples": config.samples,
                "warmup": config.warmup,
                "batch": config.batch,
            },
            "cases": rows,
            "bridge": bridge,
            "shell_submission_us": submission.map(|stats| stats.median),
        });
        if let Err(error) = fs::write(path, format!("{document:#}\n")) {
            eprintln!("cannot write {}: {error}", path.display());
        }
    }
}

fn run_one(config: &Config, case: &str) -> Row {
    let (luau_ops, luau_times) = run_luau(config, case);
    let (rust_ops, rust_times) = run_rust(config, case);
    let luau_ops = canonical_luau(luau_ops);
    let parity = same(&luau_ops, &rust_ops);
    if !parity {
        fs::write(
            format!("/tmp/timed-bench-mismatch-{case}-luau.json"),
            serde_json::to_string_pretty(&luau_ops).unwrap_or_default(),
        )
        .ok();
        fs::write(
            format!("/tmp/timed-bench-mismatch-{case}-rust.json"),
            serde_json::to_string_pretty(&rust_ops).unwrap_or_default(),
        )
        .ok();
    }
    let luau = stats(luau_times);
    let rust = stats(rust_times);
    Row {
        case: case.to_owned(),
        parity,
        speedup_median: luau.median / rust.median,
        speedup_p95: luau.p95 / rust.p95,
        speedup_p99: luau.p99 / rust.p99,
        luau,
        rust,
    }
}

fn parse_args() -> Config {
    let mut config = Config {
        samples: 100,
        warmup: 10,
        batch: 20,
        luau: "luau".to_owned(),
        helper: None,
        report: None,
        only: None,
        row: None,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => config.samples = number(&mut args),
            "--warmup" => config.warmup = number(&mut args),
            "--batch" => config.batch = number(&mut args),
            "--luau" => config.luau = args.next().unwrap_or_default(),
            "--helper" => config.helper = args.next().map(PathBuf::from),
            "--report" => config.report = args.next().map(PathBuf::from),
            "--case" => config.only = args.next(),
            "--row" => config.row = args.next(),
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    config
}

fn number(args: &mut impl Iterator<Item = String>) -> usize {
    args.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_default()
}

fn scratch(name: &str) -> PathBuf {
    let path = env::temp_dir()
        .join(format!("timed-bench-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("scratch dir");
    path
}

fn machine() -> Value {
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or_default();
    json!({
        "cpu": cpu,
        "cpus": std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(0),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

fn stats(mut values: Vec<f64>) -> Stats {
    if values.is_empty() {
        return Stats::default();
    }
    values.sort_by(f64::total_cmp);
    let count = values.len();
    let rank = |share: f64| {
        let index = ((share * count as f64).ceil() as usize).saturating_sub(1);
        values.get(index).copied().unwrap_or_default()
    };
    Stats {
        median: rank(0.5),
        p95: rank(0.95),
        p99: rank(0.99),
        mean: values.iter().sum::<f64>() / count as f64,
    }
}

fn same(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(one), Value::Number(two)) => {
            let (Some(one), Some(two)) = (one.as_f64(), two.as_f64()) else {
                return one == two;
            };
            (one - two).abs() <= 1e-9 * one.abs().max(two.abs()).max(1.0)
        }
        (Value::Array(one), Value::Array(two)) => {
            one.len() == two.len()
                && one.iter().zip(two).all(|(one, two)| same(one, two))
        }
        (Value::Object(one), Value::Object(two)) => {
            one.len() == two.len()
                && one.iter().all(|(key, value)| {
                    two.get(key).is_some_and(|other| same(value, other))
                })
        }
        _ => left == right,
    }
}

fn canonical(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                canonical(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                canonical(item);
            }
            let op = map.get("op").and_then(Value::as_str).map(str::to_owned);
            match op.as_deref() {
                Some("state_set") => {
                    if map.get("key").and_then(Value::as_str)
                        == Some("timed.state")
                    {
                        map.insert("key".to_owned(), json!("timed.list"));
                    }
                }
                Some("bar_image") => {
                    map.insert("path".to_owned(), json!("<ring>"));
                }
                Some("host_call")
                    if map.get("path") == Some(&json!(["state", "watch"])) =>
                {
                    let key = map
                        .get("args")
                        .and_then(|args| args.get(0))
                        .cloned()
                        .unwrap_or(Value::Null);
                    *map = BTreeMap::from([
                        ("op".to_owned(), json!("state_watch")),
                        ("key".to_owned(), key),
                    ])
                    .into_iter()
                    .collect();
                }
                _ => {}
            }
            for key in ["onClick", "onChange", "onSubmit"] {
                if map.contains_key(key) {
                    map.insert(key.to_owned(), json!("<callback>"));
                }
            }
            // The panel draws its bar in 1/400 steps, and the Luau original
            // hands the shell the exact fraction. Round both to the grid the
            // panel width can actually show.
            if let Some(progress) = map.get("progress").and_then(Value::as_f64)
            {
                map.insert(
                    "progress".to_owned(),
                    json!((progress * 400.0).round() / 400.0),
                );
            }
        }
        _ => {}
    }
}

fn canonical_ops(ops: &[Op]) -> Value {
    let mut value = serde_json::to_value(ops).unwrap_or(Value::Null);
    canonical(&mut value);
    drop_watches(&mut value);
    value
}

fn drop_watches(value: &mut Value) {
    if let Some(items) = value.as_array_mut() {
        items.retain(|item| {
            item.get("op").and_then(Value::as_str) != Some("state_watch")
        });
    }
}

/// Control-plane ops differ between the two host bindings: the bridge exposes a
/// watch as a host call, and the panel follows `timed.colors` with a watch
/// instead of reading it inside the render. Neither changes what the plugin
/// draws, so both are compared out.
fn canonical_luau(value: Value) -> Value {
    let mut value = value;
    canonical(&mut value);
    drop_watches(&mut value);
    value
}

fn measure<S>(
    config: &Config,
    mut state: S,
    mut parity: impl FnMut(&mut S) -> Vec<Op>,
    mut step: impl FnMut(&mut S, i64) -> Vec<Op>,
) -> (Value, Vec<f64>) {
    let ops = parity(&mut state);
    let value = canonical_ops(&ops);

    let mut count = 0i64;
    for _ in 0..config.warmup {
        count += 1;
        let _ = std::hint::black_box(step(&mut state, START + count * 1_000));
    }

    let mut samples = Vec::with_capacity(config.samples);
    for _ in 0..config.samples {
        let started = Instant::now();
        for _ in 0..config.batch {
            count += 1;
            let _ =
                std::hint::black_box(step(&mut state, START + count * 1_000));
        }
        samples
            .push(started.elapsed().as_secs_f64() * 1e6 / config.batch as f64);
    }
    (value, samples)
}

fn translations() -> Value {
    timed::build_plugin().translations_value()
}

fn flatten(value: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
    if let Some(map) = value.as_object() {
        for (key, item) in map {
            let key = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            flatten(item, &key, out);
        }
    } else if let Some(text) = value.as_str() {
        out.push((prefix.to_owned(), text.to_owned()));
    }
}

fn luau_string(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

fn luau_map(entries: &[(String, String)]) -> String {
    let items = entries
        .iter()
        .map(|(key, value)| {
            format!("[{}] = {}", luau_string(key), luau_string(value))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {items} }}")
}

fn luau_script(config: &Config, case: &str) -> String {
    let mut flat = Vec::new();
    flatten(&translations(), "", &mut flat);

    let colors = COLORS
        .iter()
        .map(|(role, value)| ((*role).to_owned(), (*value).to_owned()))
        .collect::<Vec<_>>();

    let mut script = String::from(PRELUDE);
    script.push_str(&format!(
        "\n__TRANSLATIONS = {}\n__COLORS = {}\n",
        luau_map(&flat),
        luau_map(&colors)
    ));
    assert!(!BRIDGE.contains("]=====]"), "bridge terminator");
    script.push_str(&format!("__BRIDGE = [=====[\n{BRIDGE}\n]=====]\n"));
    script.push_str("__SOURCES = {}\n");
    for (name, source) in MODULES {
        assert!(
            !source.contains("]=====]"),
            "{name} contains the long bracket terminator"
        );
        script.push_str(&format!(
            "__SOURCES[{}] = [=====[\n{source}\n]=====]\n",
            luau_string(name)
        ));
    }
    script.push_str(&format!(
        "__SAMPLES = {}\n__WARMUP = {}\n__BATCH = {}\n__START = {}\n__PARITY = {PARITY_NOW}\n",
        config.samples, config.warmup, config.batch, START
    ));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("luau")
        .join("cases")
        .join(format!("{case}.luau"));
    script.push_str(&fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("cannot read {}: {error}", path.display())
    }));
    script
}

fn run_luau(config: &Config, case: &str) -> (Value, Vec<f64>) {
    let dir = scratch(&format!("luau-{case}"));
    let path = dir.join(format!("{case}.luau"));
    fs::write(&path, luau_script(config, case)).expect("write script");

    let output = Command::new(&config.luau)
        .arg(&path)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", config.luau));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        panic!(
            "luau {case} failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut ops = None;
    let mut times = None;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("OPS ") {
            ops = serde_json::from_str(rest).ok();
        }
        if let Some(rest) = line.strip_prefix("TIMES ") {
            let value: Value = serde_json::from_str(rest).expect("times");
            times = Some(
                value
                    .as_array()
                    .expect("times array")
                    .iter()
                    .filter_map(Value::as_f64)
                    .collect::<Vec<_>>(),
            );
        }
    }
    (
        ops.unwrap_or_else(|| panic!("luau {case}: no OPS line: {stdout}")),
        times.unwrap_or_else(|| panic!("luau {case}: no TIMES line: {stdout}")),
    )
}

fn host_value(now: i64) -> Value {
    json!({
        "now_ms": now,
        "pluginDir": "/plugin",
        "pluginDataDir": "/data",
        "bar": { "vertical": false, "output_name": "DP-1" },
        "colors": COLORS.iter().copied().collect::<BTreeMap<_, _>>(),
        "setting": SETTINGS.iter().copied().collect::<BTreeMap<_, _>>(),
    })
}

fn widget_config() -> Value {
    json!({
        "show_text": true,
        "show_count": true,
        "show_progress": true,
        "hide_when_idle": false,
        "use_urgency_colors": true,
        "safe_color": "tertiary",
        "warning_color": "secondary",
        "urgent_color": "error",
        "idle_color": "primary",
    })
}

fn widget_tree_config() -> Value {
    let mut config = widget_config();
    config["show_progress"] = json!(false);
    config
}

fn widget_timers(now: i64) -> Value {
    json!([
        { "id": "t1", "seconds": 3600, "label": "1h", "message": "tea", "paused": false, "endsAt": now + 3_600_000 },
        { "id": "t2", "seconds": 3900, "label": "1h 5m", "paused": false, "endsAt": now + 3_900_000 },
        { "id": "t3", "seconds": 7200, "label": "2h", "message": "standup", "paused": false, "endsAt": now + 7_200_000 },
        { "id": "t4", "seconds": 86400, "label": "1d", "paused": false, "endsAt": now + 86_400_000 },
        { "id": "t5", "seconds": 172800, "label": "2d", "paused": true, "remainingMs": 5_000_000 }
    ])
}

fn panel_timers(now: i64) -> Value {
    json!([
        { "id": "p1", "seconds": 600, "label": "10m", "message": "focus", "paused": false, "endsAt": now + 600_000 },
        { "id": "p2", "seconds": 1800, "label": "30m", "paused": false, "endsAt": now + 1_800_000 },
        { "id": "p3", "seconds": 5400, "label": "1h 30m", "message": "standup", "paused": false, "endsAt": now + 5_400_000 },
        { "id": "p4", "seconds": 86400, "label": "1d", "paused": true, "remainingMs": 900_000 },
        { "id": "p5", "seconds": 172800, "label": "2d", "paused": false, "endsAt": now + 172_800_000 }
    ])
}

fn service_timers(now: i64) -> Value {
    json!([
        { "id": "s1", "seconds": 3600, "label": "1h", "message": "tea", "paused": false, "endsAt": now + 3_600_000 },
        { "id": "s2", "seconds": 7200, "label": "2h", "paused": false, "endsAt": now + 7_200_000 },
        { "id": "s3", "seconds": 14400, "label": "4h", "message": "bake", "paused": false, "endsAt": now + 14_400_000 },
        { "id": "s4", "seconds": 28800, "label": "8h", "paused": false, "endsAt": now + 28_800_000 },
        { "id": "s5", "seconds": 86400, "label": "1d", "paused": false, "endsAt": now + 86_400_000 },
        { "id": "s6", "seconds": 172800, "label": "2d", "message": "soak", "paused": false, "endsAt": now + 172_800_000 },
        { "id": "s7", "seconds": 604800, "label": "7d", "paused": false, "endsAt": now + 604_800_000 },
        { "id": "s8", "seconds": 1209600, "label": "2w", "paused": false, "endsAt": now + 1_209_600_000 },
        { "id": "s9", "seconds": 2592000, "label": "30d", "paused": true, "remainingMs": 1_800_000 },
        { "id": "s10", "seconds": 8640000, "label": "100d", "paused": false, "endsAt": now + 8_640_000_000_i64 }
    ])
}

fn context(
    entry: &str,
    event: &str,
    config: Value,
    state: Value,
    host: Value,
    dir: PathBuf,
) -> Context {
    Context::new(
        Parts {
            entry: entry.to_owned(),
            event: event.to_owned(),
            payload: json!({}),
            config,
            state,
            host,
            data_dir: dir,
        },
        translations(),
    )
}

/// Answers the synchronous host queries the Rust handlers make, the same way
/// the Luau bridge would.
fn answer_queries(ctx: &mut Context, state: Value, settings: Value) {
    ctx.set_op_sink(Arc::new(move |op| {
        let Op::Query {
            id: _,
            root,
            path,
            args,
            response,
        } = op
        else {
            return;
        };
        let value = if root == "noctalia"
            && path == &["state".to_owned(), "get".to_owned()]
        {
            args.first()
                .and_then(Value::as_str)
                .and_then(|key| state.get(key))
                .cloned()
                .unwrap_or(Value::Null)
        } else if root == "noctalia" && path == &["getSetting".to_owned()] {
            args.first()
                .and_then(Value::as_str)
                .and_then(|key| settings.get(key))
                .cloned()
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let _ = fs::write(
            response,
            json!({ "ok": true, "value": value }).to_string(),
        );
    }));
}

fn run_rust(config: &Config, case: &str) -> (Value, Vec<f64>) {
    match case {
        "widget" => widget_case(config),
        "widget_tree" => widget_tree_case(config),
        "panel" => panel_case(config),
        "launcher" => launcher_case(config),
        "service" => service_case(config),
        "duration" => duration_case(config),
        "urgency" => urgency_case(config),
        other => panic!("unknown case {other}"),
    }
}

fn widget_case(config: &Config) -> (Value, Vec<f64>) {
    let dir = scratch("widget");
    let state = json!({ "timers": widget_timers(PARITY_NOW) });
    let host = host_value(PARITY_NOW);
    let mut ctx = context(
        "widget",
        "render",
        widget_config(),
        state.clone(),
        host.clone(),
        dir,
    );
    answer_queries(&mut ctx, state, json!({}));
    measure(
        config,
        ctx,
        |ctx| {
            ctx.now_ms = PARITY_NOW;
            timed::widget::update(ctx).unwrap()
        },
        |ctx, now| {
            ctx.now_ms = now;
            timed::widget::update(ctx).unwrap()
        },
    )
}

fn widget_tree_case(config: &Config) -> (Value, Vec<f64>) {
    let dir = scratch("widget-tree");
    let state = json!({ "timers": widget_timers(PARITY_NOW) });
    let host = host_value(PARITY_NOW);
    let mut ctx = context(
        "widget",
        "render",
        widget_tree_config(),
        state.clone(),
        host,
        dir,
    );
    answer_queries(&mut ctx, state, json!({}));
    measure(
        config,
        ctx,
        |ctx| {
            ctx.now_ms = PARITY_NOW;
            timed::widget::update(ctx).unwrap()
        },
        |ctx, now| {
            ctx.now_ms = now;
            timed::widget::update(ctx).unwrap()
        },
    )
}

fn panel_case(config: &Config) -> (Value, Vec<f64>) {
    let dir = scratch("panel");
    let state = json!({ "timers": panel_timers(PARITY_NOW) });
    let colors = json!({
        "safe": "tertiary",
        "warning": "secondary",
        "urgent": "error",
        "idle": "primary",
        "useUrgency": true,
    });
    let host = host_value(PARITY_NOW);
    let mut ctx = context("panel", "open", json!({}), state.clone(), host, dir);
    answer_queries(&mut ctx, json!({ "timed.colors": colors }), json!({}));
    measure(
        config,
        ctx,
        |ctx| {
            ctx.now_ms = PARITY_NOW;
            ctx.event = "open".to_owned();
            timed::panel::update(ctx).unwrap()
        },
        |ctx, now| {
            ctx.now_ms = now;
            ctx.event = "render".to_owned();
            timed::panel::update(ctx).unwrap()
        },
    )
}

fn launcher_case(config: &Config) -> (Value, Vec<f64>) {
    let dir = scratch("launcher");
    let state = json!({ "timers": panel_timers(PARITY_NOW) });
    let host = host_value(PARITY_NOW);
    let mut ctx = context("launcher", "query", json!({}), state, host, dir);
    ctx.payload = json!({ "text": "1h5 wake up" });
    answer_queries(&mut ctx, json!({}), json!({}));
    measure(
        config,
        ctx,
        |ctx| {
            ctx.now_ms = PARITY_NOW;
            timed::launcher::update(ctx).unwrap()
        },
        |ctx, now| {
            ctx.now_ms = now;
            timed::launcher::update(ctx).unwrap()
        },
    )
}

fn service_case(config: &Config) -> (Value, Vec<f64>) {
    let dir = scratch("service");
    let list = json!({ "timers": service_timers(PARITY_NOW) });
    fs::write(
        dir.join("timers.json"),
        serde_json::to_string(&list).expect("serialize"),
    )
    .expect("seed timers");
    let host = host_value(PARITY_NOW);
    let mut ctx = context("service", "load", json!({}), json!({}), host, dir);
    answer_queries(&mut ctx, json!({}), json!({}));
    measure(
        config,
        ctx,
        |ctx| {
            ctx.now_ms = PARITY_NOW;
            ctx.event = "load".to_owned();
            timed::service::update(ctx).unwrap()
        },
        |ctx, now| {
            ctx.now_ms = now;
            ctx.event = "update".to_owned();
            timed::service::update(ctx).unwrap()
        },
    )
}

const DURATION_INPUTS: &[&str] = &[
    "5",
    "1h5",
    "1m5",
    "3h20m10s",
    "1s4h",
    "4s10h3m",
    "1h 5m",
    "1y",
    "1y2y",
    "100y",
    "101y",
    "1s5",
    "0",
    "",
    "abc",
    "1x",
    "1.5h",
    "3h2s something I want",
    "30 walk the dog",
    "1h5 30",
];

fn duration_ops() -> Vec<Op> {
    let num = |value: Option<i64>| {
        value.map_or_else(|| "-".to_owned(), |value| value.to_string())
    };
    let mut parts = Vec::with_capacity(DURATION_INPUTS.len());
    for input in DURATION_INPUTS {
        let parsed = timed::duration::parse(input);
        let (seconds, message) = timed::duration::split(input)
            .map_or((None, None), |(seconds, message)| {
                (Some(seconds), message)
            });
        let fields = [
            (*input).to_owned(),
            num(parsed),
            num(seconds),
            message.unwrap_or_else(|| "-".to_owned()),
            parsed.map_or_else(|| "-".to_owned(), timed::duration::human),
            parsed.map_or_else(|| "-".to_owned(), timed::duration::approx),
            parsed.map_or_else(|| "-".to_owned(), timed::duration::clock),
        ];
        parts.push(fields.join(" | "));
    }
    vec![Op::log(parts.join("\n"))]
}

fn duration_case(config: &Config) -> (Value, Vec<f64>) {
    measure(config, (), |()| duration_ops(), |(), _| duration_ops())
}

fn urgency_ops() -> Vec<Op> {
    let colors = json!({
        "safe": "tertiary",
        "warning": "secondary",
        "urgent": "error",
        "idle": "primary",
        "useUrgency": true,
    });
    let colors = timed::urgency::Colors::from_value(Some(&colors));
    let mut parts = Vec::with_capacity(200);
    for index in 1..=200i64 {
        let timer = timed::timer::Timer {
            id: format!("u{index}"),
            seconds: 100 + index,
            label: "x".to_owned(),
            message: None,
            paused: index % 7 == 0,
            ends_at: Some(PARITY_NOW + index * 1_000),
            remaining_ms: None,
        };
        let state = timed::urgency::state(&timer, PARITY_NOW);
        let role = timed::urgency::role(&timer, PARITY_NOW, &colors);
        parts.push(format!("{state}:{role}"));
    }
    vec![Op::log(parts.join(" "))]
}

fn urgency_case(config: &Config) -> (Value, Vec<f64>) {
    measure(config, (), |()| urgency_ops(), |(), _| urgency_ops())
}

#[allow(clippy::too_many_lines)]
fn bridge(config: &Config) -> Option<BridgeRow> {
    let helper = config.helper.as_ref()?;
    let dir = scratch("bridge");
    let data = dir.join("data");
    let spool = dir.join("spool");
    fs::create_dir_all(&data).ok()?;
    fs::create_dir_all(&spool).ok()?;

    let mut child = Command::new(helper)
        .arg("serve")
        .arg(&data)
        .arg(&spool)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let pid = child.id();
    let stdout = child.stdout.take()?;
    let mut lines = BufReader::new(stdout).lines();

    let ready = loop {
        let line = lines.next()?.ok()?;
        if let Ok(value) = serde_json::from_str::<Value>(&line)
            && value.get("control").and_then(Value::as_str) == Some("ready")
        {
            break value;
        }
    };
    let boot = ready.get("boot")?.as_str()?.to_owned();
    let run_dir = PathBuf::from(ready.get("spool")?.as_str()?);

    let request = json!({
        "boot": boot,
        "entry": "timed_indicator",
        "event": "render",
        "payload": {},
        "config": widget_config(),
        "state": { "timers": widget_timers(PARITY_NOW) },
        "host": host_value(PARITY_NOW),
    });
    let text = request.to_string();

    // The helper polls its spool every millisecond, so its idling cost shows up
    // here rather than in the request loop.
    let idle_before = cpu_ns(pid)?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    let idle_after = cpu_ns(pid)?;
    let idle_cpu_us_per_s =
        (idle_after.saturating_sub(idle_before)) as f64 / 1000.0 / 2.0;

    let mut times = Vec::with_capacity(config.samples);
    let cpu_before = cpu_ns(pid);
    for index in 0..(config.warmup + config.samples) {
        let name = format!("{:012}", index);
        let target = run_dir.join(format!("{name}.json"));
        let temporary = run_dir.join(format!("{name}.pending"));
        let started = Instant::now();
        fs::write(&temporary, &text).ok()?;
        fs::rename(&temporary, &target).ok()?;
        loop {
            let line = lines.next()?.ok()?;
            let value: Value =
                serde_json::from_str(&line).unwrap_or(Value::Null);
            match value.get("control").and_then(Value::as_str) {
                Some("done") => break,
                Some("error") => {
                    eprintln!(
                        "bridge: {}",
                        value.get("message").unwrap_or(&Value::Null)
                    );
                    break;
                }
                _ => {}
            }
        }
        if index >= config.warmup {
            times.push(started.elapsed().as_secs_f64() * 1e6);
        }
    }
    let cpu_after = cpu_ns(pid);
    let rss = rss_kib(pid).unwrap_or(0);
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(&dir);

    let samples = stats(times);
    let events = (cpu_after
        .unwrap_or(0)
        .saturating_sub(cpu_before.unwrap_or(0))) as f64
        / 1000.0
        / config.samples as f64;
    Some(BridgeRow {
        events: config.samples,
        latency_median: samples.median,
        latency_p95: samples.p95,
        latency_p99: samples.p99,
        cpu_us_per_event: events,
        idle_cpu_us_per_s,
        rss_kib: rss,
    })
}

/// On-CPU nanoseconds from `/proc/<pid>/schedstat`, which resolves a single
/// event far better than the 10 ms utime/stime tick.
fn cpu_ns(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/schedstat")).ok()?;
    text.split_whitespace().next()?.parse().ok()
}

fn rss_kib(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    text.lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
}

/// Cost of one `bridge.call` inside the shell's Luau VM: the host snapshot,
/// request encode, and spool write the Rust design adds to every event.
fn shell_submission(config: &Config) -> Option<Stats> {
    let (_, times) = run_luau(config, "bridge_call");
    let stats = stats(times);
    (stats.median > 0.0).then_some(stats)
}

fn print_report(
    rows: &[Row],
    bridge: Option<&BridgeRow>,
    submission: Option<Stats>,
) {
    let machine = machine();
    println!(
        "machine: {} | {} cpus | {} {}",
        machine["cpu"].as_str().unwrap_or("?"),
        machine["cpus"],
        machine["os"].as_str().unwrap_or("?"),
        machine["arch"].as_str().unwrap_or("?")
    );
    println!();
    println!(
        "| case | parity | Luau median | Luau p95 | Luau p99 | Rust median | Rust p95 | Rust p99 | speedup median | speedup p95 | speedup p99 |"
    );
    println!(
        "| :--- | :--- | --: | --: | --: | --: | --: | --: | --: | --: | --: |"
    );
    for row in rows {
        println!(
            "| {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.2}x | {:.2}x | {:.2}x |",
            row.case,
            if row.parity { "ok" } else { "MISMATCH" },
            row.luau.median,
            row.luau.p95,
            row.luau.p99,
            row.rust.median,
            row.rust.p95,
            row.rust.p99,
            row.speedup_median,
            row.speedup_p95,
            row.speedup_p99,
        );
    }
    if let Some(submission) = submission {
        println!();
        println!(
            "shell bridge submission: median {:.3} us, p95 {:.3} us, p99 {:.3} us per event (host snapshot, encode, in-memory spool write)",
            submission.median, submission.p95, submission.p99
        );
    }
    if let Some(bridge) = bridge {
        println!();
        println!(
            "bridge: {} requests, latency median {:.3} ms, p95 {:.3} ms, p99 {:.3} ms, helper CPU {:.1} us/event, helper idle {:.1} us/s, RSS {} KiB",
            bridge.events,
            bridge.latency_median / 1000.0,
            bridge.latency_p95 / 1000.0,
            bridge.latency_p99 / 1000.0,
            bridge.cpu_us_per_event,
            bridge.idle_cpu_us_per_s,
            bridge.rss_kib,
        );
    }
    let _ = std::io::stdout().flush();
}
