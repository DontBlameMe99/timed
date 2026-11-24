#![expect(
    clippy::too_many_lines,
    reason = "one small benchmark driver covers every real handler"
)]
#![expect(
    clippy::expect_used,
    reason = "the benchmark stops when setup or serialization fails"
)]
#![expect(
    clippy::absolute_paths,
    reason = "fully qualified standard calls keep the driver compact"
)]
#![expect(
    clippy::excessive_nesting,
    reason = "the driver mirrors nested host payloads"
)]
#![expect(clippy::print_stdout, reason = "the benchmark emits one JSON report")]

//! Stable-Rust performance harness for the real plugin handlers.
//!
//! Run one case per process so process-global caches start cold:
//! `cargo bench --bench perf -- --case widget_tick`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::time::Instant;

use noctalia_rs::{Context, Op, Parts};
use serde_json::{Value, json};

const NOW: i64 = 1_800_000_000_000;
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
const CASES: &[&str] = &[
    "widget_tick",
    "widget_redraw",
    "panel_tick",
    "panel_redraw",
    "launcher_list",
    "launcher_start",
    "service_tick",
    "service_commit",
];

#[derive(Debug)]
struct Config {
    case: String,
    samples: usize,
    warmup: usize,
    batch: usize,
    trace_steps: usize,
    snapshot: Option<PathBuf>,
    record: Option<PathBuf>,
}

#[derive(Debug, serde::Serialize)]
struct Stats {
    median: f64,
    p95: f64,
    p99: f64,
}

#[derive(Debug, serde::Serialize)]
struct Report {
    case: String,
    samples: usize,
    batch: usize,
    stats: Stats,
    trace_hash: String,
    trace: Value,
}

struct Harness {
    ctx: Context,
    dir: PathBuf,
    step: i64,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Harness {
    fn new(case: &str, count: usize) -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!("perf-{case}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("benchmark scratch directory");
        let timers = timers(count);
        let state = json!({ "timers": timers });
        let host = host(NOW);
        let settings =
            json!(SETTINGS.iter().copied().collect::<BTreeMap<_, _>>());
        let query_state = json!({ "timed.colors": {
            "safe": "tertiary",
            "warning": "secondary",
            "urgent": "error",
            "idle": "primary",
            "useUrgency": true,
        }});
        let (entry, event, config, payload) = match case {
            "widget_tick" | "widget_redraw" => {
                ("widget", "render", widget_config(), json!({}))
            }
            "panel_tick" | "panel_redraw" => {
                ("panel", "open", json!({}), json!({}))
            }
            "launcher_list" => {
                ("launcher", "query", json!({}), json!({ "text": "stop" }))
            }
            "launcher_start" => (
                "launcher",
                "query",
                json!({}),
                json!({ "text": "1h5 wake up" }),
            ),
            "service_tick" | "service_commit" => {
                ("service", "load", json!({}), json!({}))
            }
            _ => process::exit(2),
        };
        let mut ctx = Context::new(
            Parts {
                entry: entry.to_owned(),
                event: event.to_owned(),
                payload,
                config,
                state,
                host,
                data_dir: dir.clone(),
            },
            timed::build_plugin().translations_value(),
        );
        let answers = query_state;
        let answer_settings = settings;
        ctx.set_op_sink(Arc::new(move |op| {
            let Op::Query {
                root,
                path,
                args,
                response,
                ..
            } = op
            else {
                return;
            };
            let value = if root == "noctalia"
                && path.iter().map(String::as_str).eq(["state", "get"])
            {
                args.first()
                    .and_then(Value::as_str)
                    .and_then(|key| answers.get(key))
                    .cloned()
                    .unwrap_or(Value::Null)
            } else if root == "noctalia"
                && path.iter().map(String::as_str).eq(["getSetting"])
            {
                args.first()
                    .and_then(Value::as_str)
                    .and_then(|key| answer_settings.get(key))
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
        let mut harness = Self { ctx, dir, step: 0 };
        match case {
            "widget_tick" | "widget_redraw" | "panel_tick" | "panel_redraw"
            | "service_tick" => {
                let _ = harness.dispatch(case);
            }
            _ => {}
        }
        harness
    }

    fn dispatch(&mut self, case: &str) -> Vec<Op> {
        self.step = self.step.saturating_add(1);
        self.ctx.now_ms = NOW.saturating_add(self.step.saturating_mul(1_000));
        if matches!(case, "widget_redraw" | "panel_redraw")
            && let Some(host) = self.ctx.host.as_object_mut()
        {
            host.insert("timed_state_version".to_owned(), json!(self.step));
        }
        if self.step > 1 {
            match case {
                "panel_tick" | "panel_redraw" => {
                    "render".clone_into(&mut self.ctx.event);
                }
                "service_tick" => "update".clone_into(&mut self.ctx.event),
                _ => {}
            }
        }
        match case {
            "widget_redraw" => {
                if let Some(timer) = self
                    .ctx
                    .state
                    .get_mut("timers")
                    .and_then(Value::as_array_mut)
                    .and_then(|timers| timers.first_mut())
                    && let Some(object) = timer.as_object_mut()
                {
                    object.insert(
                        "label".to_owned(),
                        json!(format!("tick {}", self.step)),
                    );
                }
            }
            "panel_redraw" => {
                if let Some(timer) = self
                    .ctx
                    .state
                    .get_mut("timers")
                    .and_then(Value::as_array_mut)
                    .and_then(|timers| timers.first_mut())
                    && let Some(object) = timer.as_object_mut()
                {
                    object.insert(
                        "label".to_owned(),
                        json!(format!("tick {}", self.step)),
                    );
                }
            }
            "service_commit" => {
                "host_callback".clone_into(&mut self.ctx.event);
                self.ctx.payload = json!({
                    "id": "command",
                    "args": [{
                        "action": "start",
                        "seconds": 3_600_i64.saturating_add(self.step),
                        "id": format!("bench-{}", self.step),
                        "n": self.step,
                    }],
                });
            }
            _ => {}
        }
        match case {
            "widget_tick" | "widget_redraw" => {
                timed::widget::update(&self.ctx).expect("widget")
            }
            "panel_tick" | "panel_redraw" => {
                timed::panel::update(&self.ctx).expect("panel")
            }
            "launcher_list" | "launcher_start" => {
                timed::launcher::update(&self.ctx).expect("launcher")
            }
            "service_tick" | "service_commit" => {
                timed::service::update(&self.ctx).expect("service")
            }
            _ => process::exit(2),
        }
    }
}

fn timers(count: usize) -> Value {
    let values = (0..count)
        .map(|index| {
            let seconds = 3_600_i64.saturating_add(
                i64::try_from(index).unwrap_or(i64::MAX).saturating_mul(37),
            );
            let paused = index.checked_rem(11) == Some(0);
            if paused {
                json!({
                    "id": format!("t{index}"),
                    "seconds": seconds,
                    "label": format!("timer {index}"),
                    "paused": true,
                    "remainingMs": seconds.saturating_mul(1_000),
                })
            } else {
                json!({
                    "id": format!("t{index}"),
                    "seconds": seconds,
                    "label": format!("timer {index}"),
                    "message": if index.checked_rem(3) == Some(0) { Some(format!("message {index}")) } else { None },
                    "paused": false,
                    "endsAt": NOW.saturating_add(seconds.saturating_mul(1_000)),
                })
            }
        })
        .collect::<Vec<_>>();
    Value::Array(values)
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

fn host(now: i64) -> Value {
    json!({
        "now_ms": now,
        "pluginDir": "/plugin",
        "pluginDataDir": "/data",
        "bar": { "vertical": false, "output_name": "DP-1" },
        "colors": COLORS.iter().copied().collect::<BTreeMap<_, _>>(),
        "setting": SETTINGS.iter().copied().collect::<BTreeMap<_, _>>(),
        "timed_state_version": 0,
    })
}

fn canonical(value: &mut Value, dir: &PathBuf) {
    match value {
        Value::Array(items) => {
            for item in items {
                canonical(item, dir);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                canonical(item, dir);
            }
            for item in map.values_mut() {
                if let Some(text) = item.as_str() {
                    if text.contains(".noctalia-rs/query/")
                        || text.starts_with("noctalia_rs_")
                    {
                        *item = json!("<query>");
                    } else {
                        let prefix = dir.to_string_lossy();
                        if let Some(rest) = text.strip_prefix(prefix.as_ref()) {
                            *item = json!(format!("<data>{rest}"));
                        }
                    }
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn trace(ops: &[Op], dir: &PathBuf) -> Value {
    let mut value = serde_json::to_value(ops).expect("ops");
    canonical(&mut value, dir);
    value
}

fn stats(mut samples: Vec<f64>) -> Stats {
    samples.sort_by(f64::total_cmp);
    let rank = |numerator: usize| {
        let index = samples
            .len()
            .saturating_mul(numerator)
            .saturating_add(99)
            .saturating_div(100)
            .saturating_sub(1);
        samples.get(index).copied().unwrap_or(0.0)
    };
    Stats {
        median: rank(50),
        p95: rank(95),
        p99: rank(99),
    }
}

fn run(config: &Config) -> Report {
    let count = if config.case.starts_with("service") {
        100
    } else {
        50
    };
    let mut harness = Harness::new(&config.case, count);
    if config.case == "service_commit" {
        fs::write(
            harness.dir.join("timers.json"),
            serde_json::to_string(&json!({ "timers": timers(count) }))
                .expect("seed"),
        )
        .expect("seed timers");
        let _ = harness.dispatch(&config.case);
    }
    for _ in 0..config.warmup {
        let _ = std::hint::black_box(harness.dispatch(&config.case));
    }
    let mut captured = Vec::with_capacity(config.trace_steps);
    for _ in 0..config.trace_steps {
        captured.push(trace(&harness.dispatch(&config.case), &harness.dir));
    }
    let trace_value = Value::Array(captured);
    let mut hasher = DefaultHasher::new();
    trace_value.hash(&mut hasher);
    let trace_hash = format!("{:016x}", hasher.finish());

    let mut samples = Vec::with_capacity(config.samples);
    for _ in 0..config.samples {
        let started = Instant::now();
        for _ in 0..config.batch {
            let _ = std::hint::black_box(harness.dispatch(&config.case));
        }
        let elapsed = started.elapsed().as_secs_f64();
        let per_op = elapsed * 1_000_000.0
            / f64::from(u32::try_from(config.batch).unwrap_or(u32::MAX));
        samples.push(per_op);
    }
    if let Some(path) = &config.record {
        fs::write(path, serde_json::to_string(&samples).expect("samples"))
            .expect("record");
    }
    if let Some(path) = &config.snapshot {
        fs::write(
            path,
            serde_json::to_string_pretty(&trace_value).expect("trace"),
        )
        .expect("snapshot");
    }
    Report {
        case: config.case.clone(),
        samples: config.samples,
        batch: config.batch,
        stats: stats(samples),
        trace_hash,
        trace: trace_value,
    }
}

fn number(args: &mut impl Iterator<Item = String>) -> usize {
    args.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| process::exit(2))
}

fn parse() -> Config {
    let mut config = Config {
        case: "widget_tick".to_owned(),
        samples: 200,
        warmup: 50,
        batch: 100,
        trace_steps: 4,
        snapshot: None,
        record: None,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => {
                config.case = args.next().unwrap_or_else(|| process::exit(2));
            }
            "--samples" => config.samples = number(&mut args),
            "--warmup" => config.warmup = number(&mut args),
            "--batch" => config.batch = number(&mut args),
            "--trace-steps" => config.trace_steps = number(&mut args),
            "--snapshot" => config.snapshot = args.next().map(PathBuf::from),
            "--record" => config.record = args.next().map(PathBuf::from),
            _ => process::exit(2),
        }
    }
    if !CASES.contains(&config.case.as_str()) {
        eprintln!("cases: {}", CASES.join(", "));
        process::exit(2);
    }
    config
}

fn main() {
    let config = parse();
    let report = run(&config);
    println!("{}", serde_json::to_string(&report).expect("report"));
}
