//! Test contexts: a plugin request without the bridge.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

use noctalia_rs::{Context, Parts};
use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn data_dir() -> PathBuf {
    let index = NEXT.fetch_add(1, Ordering::Relaxed);
    let path =
        env::temp_dir().join(format!("timed-test-{}-{index}", process::id()));
    let _ = fs::create_dir_all(&path);
    path
}

#[expect(
    clippy::too_many_arguments,
    reason = "one argument per request part the handler reads"
)]
pub fn context(
    event: &str,
    config: Value,
    state: Value,
    host: Value,
) -> Context {
    Context::new(
        Parts {
            entry: "test".to_owned(),
            event: event.to_owned(),
            payload: json!({}),
            config,
            state,
            host,
            data_dir: data_dir(),
        },
        crate::build_plugin().translations_value(),
    )
}
