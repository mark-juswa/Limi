//! Live Claude plan usage through Claude Code itself.
//!
//! Claude Code's SDK interface has a `get_usage` control request meant for
//! usage meters: it returns the same plan limits Claude shows in its own usage
//! screen. Limi starts Claude Code headless, sends `initialize` + `get_usage`,
//! and exits. No prompt is sent, so nothing counts against the limits, and
//! Claude Code handles its own sign-in: Limi never touches credentials.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::model::UsageWindow;
use crate::tokens::parse_ts_ms;

pub const SOURCE: &str = "Claude Code";
const TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, PartialEq)]
pub enum Fetch {
    /// `fresh` is false when Claude Code answered from its cached copy because
    /// the usage service refused the request (it rate-limits frequent checks).
    Ok { windows: Vec<UsageWindow>, plan: Option<String>, fresh: bool },
    /// The usage service is rate-limiting this account and Claude Code had no
    /// recent copy to fall back on. Temporary; retry later.
    Busy,
    /// Signed in without a Pro/Max plan (e.g. API key): no plan limits exist.
    NoLimits,
    SignedOut,
    Failed(String),
}

/// Find Claude Code's executable (native install or npm global install).
pub fn resolve(custom: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = custom.map(str::trim).filter(|c| !c.is_empty()) {
        let p = PathBuf::from(c);
        return p.is_file().then_some(p);
    }
    let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let direct = dir.join(exe);
            if direct.is_file() {
                return Some(direct);
            }
            // npm global install: claude.cmd wraps the native binary.
            if cfg!(windows) && dir.join("claude.cmd").is_file() {
                let bin = dir
                    .join("node_modules")
                    .join("@anthropic-ai")
                    .join("claude-code")
                    .join("bin")
                    .join(exe);
                if bin.is_file() {
                    return Some(bin);
                }
            }
        }
    }
    let local = dirs::home_dir()?.join(".local").join("bin").join(exe);
    local.is_file().then_some(local)
}

fn plan_name(raw: &str) -> String {
    match raw {
        "pro" => "Pro".into(),
        "max" => "Max".into(),
        "team" => "Team".into(),
        "enterprise" => "Enterprise".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

fn resets(v: &Value) -> Option<i64> {
    v.get("resets_at").and_then(Value::as_str).and_then(parse_ts_ms).map(|ms| ms / 1000)
}

/// "weekly_opus" -> "Weekly · Opus"
fn scoped_label(kind: &str) -> String {
    let mut parts = kind.split('_').map(|p| {
        let mut c = p.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    });
    let first = parts.next().unwrap_or_default();
    let rest: Vec<String> = parts.collect();
    if rest.is_empty() {
        first
    } else {
        format!("{first} · {}", rest.join(" "))
    }
}

/// Parse the `get_usage` response body.
pub fn parse(resp: &Value) -> Fetch {
    let plan = resp.get("subscription_type").and_then(Value::as_str).map(plan_name);
    if resp.get("rate_limits_available").and_then(Value::as_bool) == Some(false) {
        return Fetch::NoLimits;
    }
    let Some(rl) = resp.get("rate_limits").filter(|v| v.is_object()) else {
        // Limits exist for this plan but none came back: the service refused
        // the check (HTTP 429) and Claude Code's cached copy was too old.
        return Fetch::Busy;
    };

    // A fresh answer carries the `limits` list; Claude Code drops it when it
    // falls back to its cached copy.
    let fresh = rl.get("limits").is_some_and(Value::is_array);
    let mut windows = Vec::new();
    if let Some(limits) = rl.get("limits").and_then(Value::as_array) {
        for l in limits {
            let (Some(kind), Some(pct)) = (l.get("kind").and_then(Value::as_str), l.get("percent").and_then(Value::as_f64)) else {
                continue;
            };
            let group = l.get("group").and_then(Value::as_str).unwrap_or("");
            let (id, label, minutes) = match (kind, group) {
                ("session", _) => ("session".to_string(), "5-hour session".to_string(), Some(300)),
                ("weekly_all", _) => ("weekly".to_string(), "Weekly".to_string(), Some(10_080)),
                (k, "weekly") => (k.to_string(), scoped_label(k), Some(10_080)),
                (k, "session") => (k.to_string(), scoped_label(k), Some(300)),
                (k, _) => (k.to_string(), scoped_label(k), None),
            };
            windows.push(UsageWindow { id, label, used_percent: pct, resets_at: resets(l), window_minutes: minutes });
        }
    }
    // Older shape: only the named windows.
    if windows.is_empty() {
        for (key, id, label, minutes) in [
            ("five_hour", "session", "5-hour session", 300),
            ("seven_day", "weekly", "Weekly", 10_080),
        ] {
            if let Some(w) = rl.get(key).filter(|w| w.is_object()) {
                if let Some(pct) = w.get("utilization").and_then(Value::as_f64) {
                    windows.push(UsageWindow {
                        id: id.into(),
                        label: label.into(),
                        used_percent: pct,
                        resets_at: resets(w),
                        window_minutes: Some(minutes),
                    });
                }
            }
        }
    }
    if windows.is_empty() {
        return Fetch::NoLimits;
    }
    Fetch::Ok { windows, plan, fresh }
}

/// When Claude Code last fetched the usage it caches in `.claude.json`
/// (epoch ms). Only that timestamp is read.
pub fn cached_at() -> Option<i64> {
    let file = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join(".claude.json"),
        _ => dirs::home_dir()?.join(".claude.json"),
    };
    let v: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    v.pointer("/cachedUsageUtilization/fetchedAtMs")?.as_i64()
}

/// Ask Claude Code for current plan usage.
pub async fn fetch(exe: &Path) -> Fetch {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // Keep the run minimal: no user hooks, MCP servers, commands or saved session.
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--strict-mcp-config",
        "--no-session-persistence",
        "--disable-slash-commands",
    ])
    .current_dir(std::env::temp_dir())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Fetch::Failed(format!("Couldn't start Claude Code: {e}")),
    };
    #[cfg(windows)]
    let _job = crate::proc::kill_tree_on_close(&child);

    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Fetch::Failed("Couldn't talk to Claude Code.".into());
    };
    let requests = [
        json!({ "type": "control_request", "request_id": "q-init", "request": { "subtype": "initialize" } }),
        json!({ "type": "control_request", "request_id": "q-usage", "request": { "subtype": "get_usage", "skip_behaviors": true } }),
    ];
    for r in requests {
        let mut line = r.to_string().into_bytes();
        line.push(b'\n');
        if stdin.write_all(&line).await.is_err() {
            return Fetch::Failed("Claude Code closed the connection.".into());
        }
    }
    let _ = stdin.flush().await;

    let mut lines = BufReader::new(stdout).lines();
    let read = async {
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            if msg.get("type").and_then(Value::as_str) != Some("control_response") {
                continue;
            }
            let resp = &msg["response"];
            if resp.get("request_id").and_then(Value::as_str) != Some("q-usage") {
                continue;
            }
            if resp.get("subtype").and_then(Value::as_str) == Some("success") {
                return parse(&resp["response"]);
            }
            let err = resp.get("error").and_then(Value::as_str).unwrap_or("").to_lowercase();
            if err.contains("login") || err.contains("auth") || err.contains("sign") || err.contains("credential") {
                return Fetch::SignedOut;
            }
            return Fetch::Failed("Claude Code couldn't load usage right now.".into());
        }
        Fetch::Failed("Claude Code stopped before reporting usage.".into())
    };
    let result = tokio::time::timeout(TIMEOUT, read)
        .await
        .unwrap_or_else(|_| Fetch::Failed("Claude Code isn't responding.".into()));
    drop(stdin);
    let _ = child.kill().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_get_usage_response() {
        // Captured from Claude Code 2.1.150 (Pro plan), trimmed.
        let v: Value = serde_json::from_str(r#"{"subscription_type":"pro","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":70,"resets_at":"2026-10-30T15:29:59.621830+00:00"},"seven_day":{"utilization":23,"resets_at":"2026-11-05T17:59:59.621849+00:00"},"seven_day_opus":null,"limits":[{"kind":"session","group":"session","percent":70,"severity":"normal","resets_at":"2026-10-30T15:29:59.621830+00:00","scope":null,"is_active":true},{"kind":"weekly_all","group":"weekly","percent":23,"severity":"normal","resets_at":"2026-11-05T17:59:59.621849+00:00","scope":null,"is_active":false}]},"behaviors":null}"#).unwrap();
        let Fetch::Ok { windows, plan, fresh } = parse(&v) else { panic!("expected numbers") };
        assert!(fresh);
        assert_eq!(plan.as_deref(), Some("Pro"));
        assert_eq!(windows.len(), 2);
        assert_eq!((windows[0].id.as_str(), windows[0].used_percent), ("session", 70.0));
        assert_eq!(windows[0].resets_at, Some(1_793_374_199));
        assert_eq!((windows[1].id.as_str(), windows[1].used_percent), ("weekly", 23.0));
        assert_eq!(windows[1].window_minutes, Some(10_080));
    }

    #[test]
    fn model_scoped_weekly_limits_are_shown() {
        let v = json!({ "rate_limits_available": true, "rate_limits": { "limits": [
            { "kind": "weekly_opus", "group": "weekly", "percent": 41, "resets_at": "2026-11-05T17:59:59Z" }
        ]}});
        let Fetch::Ok { windows, .. } = parse(&v) else { panic!() };
        assert_eq!(windows[0].label, "Weekly · Opus");
    }

    #[test]
    fn falls_back_to_named_windows_and_handles_no_plan() {
        // Cached answer (Claude Code was rate-limited): no `limits` list.
        let v = json!({ "rate_limits": { "five_hour": { "utilization": 12, "resets_at": null } } });
        let Fetch::Ok { windows, fresh, .. } = parse(&v) else { panic!() };
        assert!(!fresh);
        assert_eq!((windows[0].used_percent, windows[0].resets_at), (12.0, None));
        assert_eq!(parse(&json!({ "rate_limits_available": false })), Fetch::NoLimits);
        // Captured while rate-limited: plan has limits, but none returned.
        assert_eq!(parse(&json!({ "subscription_type": "pro", "rate_limits_available": true, "rate_limits": null })), Fetch::Busy);
    }
}
