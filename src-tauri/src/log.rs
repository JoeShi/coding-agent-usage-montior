//! Minimal append-only diagnostics log for the installed app.
//!
//! GUI apps launched from Finder/launchd have their stdout/stderr discarded,
//! so `eprintln!` is useless post-release. This writes one timestamped line
//! per event to `~/Library/Logs/agent-plan-monitor.log` with a simple size
//! cap. All I/O errors are swallowed: logging must never break the feature
//! it observes. Never log secret material (AK/SK, session tokens, API keys).

use std::io::Write;
use std::path::PathBuf;

const MAX_LOG_BYTES: u64 = 1_000_000;

fn log_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join("Library/Logs/agent-plan-monitor.log"))
}

pub fn log(msg: &str) {
    let Some(path) = log_path() else { return };
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    // Rotate by truncation once past the cap — simple and bounded.
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let _ = std::fs::write(&path, format!("{ts} log rotated (was {} bytes)\n", meta.len()));
        }
    }
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let _ = writeln!(f, "{ts} {msg}");
}
