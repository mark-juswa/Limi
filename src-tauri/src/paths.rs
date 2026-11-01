//! File locations shared by the widget and the status-line bridge.
//!
//! The bridge runs as a separate short-lived process (launched by Claude Code),
//! so it cannot use Tauri's path resolver. Both sides use these helpers instead.

use std::io;
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\app.limi.widget` on Windows (Tauri's app-data convention,
/// separate from the install folder so updates and uninstalls handle it correctly).
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("app.limi.widget")
}

/// Carry settings over from the app's former name (Quoti), once.
pub fn migrate_legacy() {
    let new = data_dir();
    let old = new.with_file_name("app.quoti.widget");
    if !new.join("settings.json").exists() && old.join("settings.json").exists() {
        let _ = std::fs::create_dir_all(&new);
        let _ = std::fs::copy(old.join("settings.json"), new.join("settings.json"));
    }
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

/// Last good live Claude reading, kept across restarts.
pub fn claude_last_file() -> PathBuf {
    data_dir().join("claude-last.json")
}

/// Usage numbers captured from Claude Code's status line.
pub fn claude_usage_file() -> PathBuf {
    data_dir().join("claude-usage.json")
}

/// Claude Code's config folder. Honors `CLAUDE_CONFIG_DIR` like Claude Code does.
pub fn claude_config_dir() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(d) if !d.is_empty() => Some(PathBuf::from(d)),
        _ => Some(dirs::home_dir()?.join(".claude")),
    }
}

pub fn claude_settings_file() -> Option<PathBuf> {
    Some(claude_config_dir()?.join("settings.json"))
}

/// Codex's home folder. Honors `CODEX_HOME` like Codex does.
pub fn codex_home() -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME") {
        Some(d) if !d.is_empty() => Some(PathBuf::from(d)),
        _ => Some(dirs::home_dir()?.join(".codex")),
    }
}

/// Write via a temp file + rename so readers never see a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    // Another process may hold the target open for a moment; retry briefly.
    let mut last_err = None;
    for _ in 0..5 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last_err.unwrap_or_else(|| io::Error::other("rename failed")))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
