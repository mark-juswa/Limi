//! Codex integration via the official `codex app-server` JSON-RPC interface.
//!
//! Codex handles its own sign-in; Limi only asks it for
//! `account/rateLimits/read` and listens for `account/rateLimits/updated`.
//! No Codex credentials are read by Limi.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::model::{window_label, ProviderSnapshot, Status, UsageWindow};
use crate::paths::now_ms;

pub const SOURCE: &str = "Codex app-server";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const NOT_INSTALLED_RECHECK: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    /// Fetch now.
    Refresh,
    /// Settings changed: restart with the new configuration.
    Reconfigure,
}

fn base() -> ProviderSnapshot {
    ProviderSnapshot::new("codex", "Codex", SOURCE, true)
}

// ---------------------------------------------------------------- parsing

fn plan_name(raw: &str) -> String {
    match raw {
        "plus" => "Plus".into(),
        "pro" => "Pro".into(),
        "team" => "Team".into(),
        "business" => "Business".into(),
        "enterprise" => "Enterprise".into(),
        "edu" => "Edu".into(),
        "free" => "Free".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

/// Parse an `account/rateLimits/read` result or `account/rateLimits/updated` params.
pub fn parse_rate_limits(v: &Value) -> Option<(Vec<UsageWindow>, Option<String>)> {
    let rl = v.get("rateLimits").filter(|r| r.is_object())?;
    let mut windows = Vec::new();
    for key in ["primary", "secondary"] {
        let Some(w) = rl.get(key).filter(|w| w.is_object()) else {
            continue;
        };
        let Some(used) = w.get("usedPercent").and_then(Value::as_f64).filter(|u| u.is_finite()) else {
            continue;
        };
        let minutes = w.get("windowDurationMins").and_then(Value::as_u64).map(|m| m as u32);
        let (id, label) = window_label(minutes);
        windows.push(UsageWindow {
            id,
            label,
            used_percent: used,
            resets_at: w.get("resetsAt").and_then(Value::as_f64).map(|s| s as i64),
            window_minutes: minutes,
        });
    }
    let plan = rl
        .get("planType")
        .or_else(|| v.get("planType"))
        .and_then(Value::as_str)
        .map(plan_name);
    Some((windows, plan))
}

fn ok_snapshot(windows: Vec<UsageWindow>, plan: Option<String>) -> ProviderSnapshot {
    if windows.is_empty() {
        return ProviderSnapshot { plan, ..base() }.with_status(
            Status::NoLimits,
            "Codex reports no usage limits for this account.".to_string(),
        );
    }
    ProviderSnapshot {
        status: Status::Ok,
        plan,
        windows,
        captured_at: Some(now_ms()),
        ..base()
    }
}

fn api_key_snapshot() -> ProviderSnapshot {
    base().with_status(
        Status::NoLimits,
        "Codex is signed in with an API key. API usage is billed per token and has no plan limits to show.".to_string(),
    )
}

fn signed_out_snapshot() -> ProviderSnapshot {
    base().with_status(
        Status::SignedOut,
        "Codex isn't signed in. Run `codex login` with your ChatGPT account.".to_string(),
    )
}

// ---------------------------------------------------------------- locating codex

pub fn resolve(custom: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = custom.map(str::trim).filter(|c| !c.is_empty()) {
        let p = PathBuf::from(c);
        return p.is_file().then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        {
            let exe = dir.join("codex.exe");
            if exe.is_file() {
                return Some(exe);
            }
            // npm global install: codex.cmd wraps a node script that launches a
            // native binary. Run the binary directly so no console or node
            // process is involved.
            if dir.join("codex.cmd").is_file() {
                let pkg = dir.join("node_modules").join("@openai").join("codex");
                for (platform_pkg, triple) in [
                    ("codex-win32-x64", "x86_64-pc-windows-msvc"),
                    ("codex-win32-arm64", "aarch64-pc-windows-msvc"),
                ] {
                    let candidates = [
                        pkg.join("node_modules").join("@openai").join(platform_pkg)
                            .join("vendor").join(triple).join("bin").join("codex.exe"),
                        pkg.join("vendor").join(triple).join("bin").join("codex.exe"),
                        pkg.join("vendor").join(triple).join("codex").join("codex.exe"),
                    ];
                    if let Some(found) = candidates.into_iter().find(|c| c.is_file()) {
                        return Some(found);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            let exe = dir.join("codex");
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

// ---------------------------------------------------------------- service

pub fn spawn(app: AppHandle) -> mpsc::UnboundedSender<Msg> {
    let (tx, rx) = mpsc::unbounded_channel();
    tauri::async_runtime::spawn(service(app, rx));
    tx
}

enum End {
    Reconfigure,
    Failed(String),
}

async fn service(app: AppHandle, mut rx: mpsc::UnboundedReceiver<Msg>) {
    let mut backoff = Duration::from_secs(5);
    loop {
        let settings = crate::app::settings(&app);
        if !settings.codex_enabled {
            crate::app::publish(&app, base().with_status(Status::Disabled, None));
            if rx.recv().await.is_none() {
                return;
            }
            continue;
        }
        let Some(exe) = resolve(settings.codex_path.as_deref()) else {
            crate::app::publish(
                &app,
                base().with_status(
                    Status::NotInstalled,
                    "Codex CLI wasn't found. Install it (npm i -g @openai/codex) or set its path in Settings.".to_string(),
                ),
            );
            tokio::select! {
                m = rx.recv() => if m.is_none() { return },
                _ = tokio::time::sleep(NOT_INSTALLED_RECHECK) => {}
            }
            continue;
        };

        match session(&app, exe, Duration::from_secs(settings.poll_seconds), &mut rx, &mut backoff).await {
            End::Reconfigure => backoff = Duration::from_secs(5),
            End::Failed(msg) => {
                crate::app::publish_error(&app, "codex", msg);
                tokio::select! {
                    m = rx.recv() => if m.is_none() { return },
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(Duration::from_secs(300));
            }
        }
    }
}

async fn session(
    app: &AppHandle,
    exe: PathBuf,
    poll: Duration,
    rx: &mut mpsc::UnboundedReceiver<Msg>,
    backoff: &mut Duration,
) -> End {
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return End::Failed(format!("Couldn't start Codex: {e}")),
    };
    #[cfg(windows)]
    let _job = crate::proc::kill_tree_on_close(&child);

    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return End::Failed("Couldn't talk to Codex.".into());
    };
    let mut lines = BufReader::new(stdout).lines();

    async fn send(stdin: &mut tokio::process::ChildStdin, v: Value) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(&v)?;
        line.push(b'\n');
        stdin.write_all(&line).await?;
        stdin.flush().await
    }

    let version = env!("CARGO_PKG_VERSION");
    if send(&mut stdin, json!({
        "id": 1,
        "method": "initialize",
        "params": { "clientInfo": { "name": "limi", "title": "Limi", "version": version } }
    }))
    .await
    .is_err()
    {
        return End::Failed("Codex closed the connection.".into());
    }

    let mut initialized = false;
    let mut next_id: u64 = 2;
    let mut pending: Option<(u64, tokio::time::Instant)> = None;
    let mut auth_mode: Option<String> = None;
    let mut ticker = tokio::time::interval(poll);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let init_deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;

    loop {
        let deadline = match (initialized, pending) {
            (false, _) => init_deadline,
            (true, Some((_, at))) => at,
            (true, None) => tokio::time::Instant::now() + Duration::from_secs(86400),
        };
        let mut request_now = false;

        tokio::select! {
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    _ => return End::Failed("Codex stopped unexpectedly. Retrying…".into()),
                };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                let id = msg.get("id").and_then(Value::as_u64);
                let method = msg.get("method").and_then(Value::as_str);

                if !initialized && id == Some(1) {
                    if msg.get("error").is_some() {
                        return End::Failed("Codex refused the connection. Try updating Codex.".into());
                    }
                    initialized = true;
                    if send(&mut stdin, json!({ "method": "initialized" })).await.is_err() {
                        return End::Failed("Codex closed the connection.".into());
                    }
                    request_now = true;
                } else if id.is_some() && id == pending.map(|p| p.0) {
                    pending = None;
                    *backoff = Duration::from_secs(5);
                    if let Some(result) = msg.get("result") {
                        match parse_rate_limits(result) {
                            Some((w, plan)) => crate::app::publish(app, ok_snapshot(w, plan)),
                            None => crate::app::publish_error(app, "codex", "Codex returned an unexpected response.".into()),
                        }
                    } else {
                        let text = msg.pointer("/error/message").and_then(Value::as_str).unwrap_or("").to_lowercase();
                        if auth_mode.as_deref() == Some("apikey") {
                            crate::app::publish(app, api_key_snapshot());
                        } else if text.contains("auth") || text.contains("login") || text.contains("sign") || text.contains("token") {
                            crate::app::publish(app, signed_out_snapshot());
                        } else {
                            crate::app::publish_error(app, "codex", "Codex couldn't load usage right now.".into());
                        }
                    }
                } else if method == Some("account/rateLimits/updated") {
                    if let Some((w, plan)) = msg.get("params").and_then(parse_rate_limits) {
                        let plan = plan.or_else(|| crate::app::current_plan(app, "codex"));
                        crate::app::publish(app, ok_snapshot(w, plan));
                    }
                } else if method == Some("account/updated") {
                    let mode = msg.pointer("/params/authMode").and_then(Value::as_str).map(str::to_owned);
                    match mode.as_deref() {
                        Some("apikey") => crate::app::publish(app, api_key_snapshot()),
                        None => crate::app::publish(app, signed_out_snapshot()),
                        Some(_) => {}
                    }
                    auth_mode = mode;
                }
            }
            _ = ticker.tick(), if initialized => request_now = true,
            m = rx.recv() => match m {
                Some(Msg::Refresh) => request_now = true,
                Some(Msg::Reconfigure) | None => return End::Reconfigure,
            },
            _ = tokio::time::sleep_until(deadline) => {
                return End::Failed("Codex isn't responding. Retrying…".into());
            }
        }

        if request_now && initialized && pending.is_none() && auth_mode.as_deref() != Some("apikey") {
            let id = next_id;
            next_id += 1;
            if send(&mut stdin, json!({ "id": id, "method": "account/rateLimits/read" })).await.is_err() {
                return End::Failed("Codex closed the connection.".into());
            }
            pending = Some((id, tokio::time::Instant::now() + REQUEST_TIMEOUT));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_app_server_response() {
        // Captured from codex-cli 0.166.0 (account ids removed).
        let v: Value = serde_json::from_str(r#"{"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":69,"windowDurationMins":300,"resetsAt":1793371890},"secondary":{"usedPercent":64,"windowDurationMins":10080,"resetsAt":1793792928},"credits":{"hasCredits":false,"unlimited":false,"balance":"0"},"planType":"plus","rateLimitReachedType":null}}"#).unwrap();
        let (w, plan) = parse_rate_limits(&v).unwrap();
        assert_eq!(plan.as_deref(), Some("Plus"));
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].id, "session");
        assert_eq!(w[0].label, "5-hour session");
        assert_eq!(w[0].used_percent, 69.0);
        assert_eq!(w[0].resets_at, Some(1793371890));
        assert_eq!(w[1].id, "weekly");
        assert_eq!(w[1].used_percent, 64.0);
    }

    #[test]
    fn missing_windows_are_skipped_not_invented() {
        let v = json!({ "rateLimits": { "primary": null, "secondary": { "usedPercent": 5, "windowDurationMins": 10080 } } });
        let (w, _) = parse_rate_limits(&v).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].id, "weekly");
        assert_eq!(w[0].resets_at, None);
        assert_eq!(ok_snapshot(vec![], None).status, Status::NoLimits);
    }

    #[test]
    fn rejects_unrelated_payload() {
        assert!(parse_rate_limits(&json!({ "foo": 1 })).is_none());
    }
}
