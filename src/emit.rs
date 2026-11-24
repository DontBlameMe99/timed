//! `emit`, with the retired color roles removed from the generated bridge.
//!
//! noctalia-rs snapshots `surface_variant` and `outline` in its bridge.
//! Both roles were retired in plugin API 43, and the plugin validator scans
//! every `.luau` file for them, so an untouched bridge fails validation. The
//! plugin never reads those two colors. Drop this module once noctalia-rs
//! stops listing them.

use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use crate::build_plugin;

const RETIRED: [&str; 2] = ["surface_variant", "outline"];

pub fn run() -> Option<ExitCode> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("emit") {
        return None;
    }
    let dir = args.next()?;
    let helper = args.next()?;

    Some(match emit(Path::new(&dir), &helper) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("noctalia: {error}");
            ExitCode::FAILURE
        }
    })
}

fn emit(dir: &Path, helper: &str) -> io::Result<()> {
    build_plugin().emit(dir, helper)?;

    let path = dir.join("lib/bridge.luau");
    let source = fs::read_to_string(&path)?;
    let mut fixed = source.clone();
    for role in RETIRED {
        fixed = fixed.replace(&format!("  \"{role}\",\n"), "");
        if fixed.contains(&format!("\"{role}\"")) {
            return Err(io::Error::other(format!(
                "the bridge still lists {role}"
            )));
        }
    }
    if fixed != source {
        fs::write(&path, fixed)?;
    }

    Ok(())
}
