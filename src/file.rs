//! Small files the plugin owns: the saved timer list and the ring image.

use std::fs;
use std::io;
use std::path::Path;

/// Replaces a file in one step, so a reader never sees a half-written file.
///
/// # Errors
/// Returns the write or rename error, and removes the temporary file.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)
        .and_then(|()| fs::rename(&temporary, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
}
