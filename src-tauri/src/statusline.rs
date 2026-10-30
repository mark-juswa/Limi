//! `limi --statusline`: run by Claude Code on each status-line update.
//!
//! Reads the JSON payload from stdin, records the plan limits, then prints a
//! status line: the user's own (if they had one) or a short usage summary.
//! Must stay fast and must never fail loudly inside Claude Code.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::claude;
use crate::settings::Settings;

const MAX_INPUT: u64 = 4 * 1024 * 1024;
const CHAIN_TIMEOUT: Duration = Duration::from_secs(4);

pub fn run() -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().take(MAX_INPUT).read_to_string(&mut input);
    let payload: Option<Value> = serde_json::from_str(&input).ok();
    if let Some(p) = &payload {
        claude::record_payload(p);
    }

    let out = match Settings::load().previous_status_line_command() {
        Some(cmd) => run_chained(&cmd, &input).unwrap_or_default(),
        None => summary(payload.as_ref()),
    };
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
    0
}

fn summary(payload: Option<&Value>) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let limits = payload.and_then(|p| p.get("rate_limits"));
    let part = |key: &str, label: &str| -> Option<String> {
        let w = limits?.get(key)?;
        let pct = w.get("used_percentage")?.as_f64()?;
        let mut s = format!("{label} {pct:.0}%");
        if let Some(reset) = w.get("resets_at").and_then(Value::as_f64) {
            let left = reset as i64 - now;
            if left > 0 {
                s.push_str(&format!(" ({})", short_duration(left)));
            }
        }
        Some(s)
    };
    let parts: Vec<String> = [part("five_hour", "5h"), part("seven_day", "7d")]
        .into_iter()
        .flatten()
        .collect();
    if parts.is_empty() {
        "Limi".into()
    } else {
        parts.join(" · ")
    }
}

fn short_duration(secs: i64) -> String {
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

/// Run the user's previous status-line command with the same input.
fn run_chained(cmd: &str, input: &str) -> Option<String> {
    let mut command = shell(cmd);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let data = input.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(data.as_bytes());
        });
    }
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < CHAIN_TIMEOUT => std::thread::sleep(Duration::from_millis(15)),
            _ => {
                let _ = child.kill();
                break;
            }
        }
    }
    reader.join().ok()
}

#[cfg(windows)]
fn shell(cmd: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Claude Code runs commands with Git Bash when available, else PowerShell.
    let mut c = match git_bash() {
        Some(bash) => {
            let mut c = Command::new(bash);
            c.arg("-c").arg(cmd);
            c
        }
        None => {
            let mut c = Command::new("powershell.exe");
            c.args(["-NoProfile", "-NonInteractive", "-Command", cmd]);
            c
        }
    };
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(windows)]
fn git_bash() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(p) = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let git = dir.join("git.exe");
        if git.is_file() {
            // <Git>\cmd\git.exe -> <Git>\bin\bash.exe
            if let Some(root) = dir.parent() {
                let bash = root.join("bin").join("bash.exe");
                if bash.is_file() {
                    return Some(bash);
                }
            }
        }
    }
    let default = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
    default.is_file().then_some(default)
}

#[cfg(not(windows))]
fn shell(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_formats_both_windows() {
        let far = 4_000_000_000i64;
        let p = json!({ "rate_limits": {
            "five_hour": { "used_percentage": 42.4, "resets_at": far },
            "seven_day": { "used_percentage": 18.0 }
        }});
        let s = summary(Some(&p));
        assert!(s.starts_with("5h 42% ("), "{s}");
        assert!(s.ends_with(" · 7d 18%"), "{s}");
    }

    #[test]
    fn summary_without_limits() {
        assert_eq!(summary(Some(&json!({}))), "Limi");
        assert_eq!(summary(None), "Limi");
    }
}
