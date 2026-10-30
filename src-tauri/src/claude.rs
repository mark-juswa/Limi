//! Claude Code integration.
//!
//! Claude Code passes subscription limits (`rate_limits.five_hour` / `seven_day`)
//! to its status-line command as JSON on stdin. Limi registers itself as that
//! command ("the bridge"), stores only those numbers, and passes the payload on
//! to the user's own status line if they had one. No credentials are read.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::{ProviderSnapshot, Status, UsageWindow};
use crate::paths;
use crate::settings::Settings;

pub const SOURCE: &str = "Claude Code status line";

/// Resets closer together than this are treated as the same window.
const SAME_WINDOW_SECS: i64 = 120;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    pub used_percentage: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageRecord {
    /// Last time Claude Code ran the status line (epoch ms).
    pub last_seen_at: Option<i64>,
    /// Last time Claude Code reported limits (epoch ms).
    pub captured_at: Option<i64>,
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
}

fn parse_window(v: Option<&Value>) -> Option<LimitWindow> {
    let v = v?;
    let used = v.get("used_percentage")?.as_f64()?;
    if !used.is_finite() {
        return None;
    }
    let resets_at = v.get("resets_at").and_then(Value::as_f64).map(|s| s as i64);
    Some(LimitWindow { used_percentage: used, resets_at })
}

/// Combine a stored window with a newly reported one.
///
/// Several Claude Code sessions can run at once and an idle session may re-run
/// its status line with older numbers. Within one window usage only grows, so
/// keep the highest value; a later reset time means a newer window.
fn merge_window(old: Option<LimitWindow>, new: Option<LimitWindow>) -> Option<LimitWindow> {
    match (old, new) {
        (None, n) => n,
        (o, None) => o,
        (Some(o), Some(n)) => match (o.resets_at, n.resets_at) {
            (Some(a), Some(b)) if (a - b).abs() <= SAME_WINDOW_SECS => Some(LimitWindow {
                used_percentage: o.used_percentage.max(n.used_percentage),
                resets_at: Some(a.max(b)),
            }),
            (Some(a), Some(b)) if a > b => Some(o),
            _ => Some(n),
        },
    }
}

/// Fold one status-line payload into the stored record.
pub fn apply_payload(record: &mut UsageRecord, payload: &Value, now_ms: i64) {
    record.last_seen_at = Some(now_ms);
    let Some(limits) = payload.get("rate_limits").filter(|v| v.is_object()) else {
        return;
    };
    let five = parse_window(limits.get("five_hour"));
    let seven = parse_window(limits.get("seven_day"));
    if five.is_none() && seven.is_none() {
        return;
    }
    record.five_hour = merge_window(record.five_hour, five);
    record.seven_day = merge_window(record.seven_day, seven);
    record.captured_at = Some(now_ms);
}

pub fn read_record(path: &Path) -> Option<UsageRecord> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Called by the bridge process for every status-line update.
pub fn record_payload(payload: &Value) {
    let path = paths::claude_usage_file();
    let mut record = read_record(&path).unwrap_or_default();
    let before = record.clone();
    apply_payload(&mut record, payload, paths::now_ms());
    if record != before {
        if let Ok(bytes) = serde_json::to_vec(&record) {
            let _ = paths::write_atomic(&path, &bytes);
        }
    }
}

// ---------------------------------------------------------------- bridge setup

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bridge {
    Connected,
    /// Our command is registered but points at an old executable path.
    Stale,
    NotConnected,
}

/// Matches our status line, including ones registered under former app names.
pub fn is_our_status_line(v: &Value) -> bool {
    v.get("command")
        .and_then(Value::as_str)
        .map(|c| {
            let c = c.to_ascii_lowercase();
            c.contains("--statusline") && ["limi.exe", "/limi ", "quoti", "headroom"].iter().any(|n| c.contains(n))
        })
        .unwrap_or(false)
}

/// Shell command Claude Code runs. Claude Code may use Git Bash or PowerShell,
/// so avoid liming where possible: forward slashes and no spaces work in both.
pub fn bridge_command(exe: &Path) -> String {
    #[allow(unused_mut)]
    let mut p = exe.to_string_lossy().into_owned();
    #[cfg(windows)]
    if p.contains(' ') {
        if let Some(short) = short_path(exe) {
            p = short;
        }
    }
    let p = p.replace('\\', "/");
    if p.contains(' ') {
        format!("\"{p}\" --statusline")
    } else {
        format!("{p} --statusline")
    }
}

#[cfg(windows)]
fn short_path(path: &Path) -> Option<String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let len = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if len == 0 || len as usize >= buf.len() {
        return None;
    }
    buf.truncate(len as usize);
    Some(std::ffi::OsString::from_wide(&buf).to_string_lossy().into_owned())
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(Map::new()),
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(m)) => Ok(m),
            _ => Err(format!(
                "{} is not valid JSON, so Limi left it untouched. Fix the file and try again.",
                path.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("Could not read {}: {e}", path.display())),
    }
}

fn write_json_object(path: &Path, root: Map<String, Value>) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    text.push('\n');
    paths::write_atomic(path, text.as_bytes())
        .map_err(|e| format!("Could not write {}: {e}", path.display()))
}

pub fn bridge_state(claude_settings: &Path, exe: &Path) -> Bridge {
    let Ok(root) = read_json_object(claude_settings) else {
        return Bridge::NotConnected;
    };
    match root.get("statusLine") {
        Some(v) if is_our_status_line(v) => {
            let expected = bridge_command(exe);
            if v.get("command").and_then(Value::as_str) == Some(expected.as_str()) {
                Bridge::Connected
            } else {
                Bridge::Stale
            }
        }
        _ => Bridge::NotConnected,
    }
}

/// Register the bridge in Claude Code's settings, keeping the user's own
/// status line (if any) so it keeps showing and can be restored.
pub fn connect(claude_settings: &Path, exe: &Path, settings: &mut Settings) -> Result<(), String> {
    let mut root = read_json_object(claude_settings)?;
    let current = root.get("statusLine").cloned();

    let mut line = Map::new();
    line.insert("type".into(), "command".into());
    line.insert("command".into(), bridge_command(exe).into());
    if let Some(Value::Object(cur)) = &current {
        if !is_our_status_line(&Value::Object(cur.clone())) {
            settings.claude_previous_status_line = Some(Value::Object(cur.clone()));
        }
        // Keep layout options such as padding or refreshInterval.
        for (k, v) in cur {
            if k != "type" && k != "command" {
                line.insert(k.clone(), v.clone());
            }
        }
    }

    if claude_settings.exists() {
        let backup = claude_settings.with_extension("json.limi-backup");
        let _ = std::fs::copy(claude_settings, backup);
    }
    root.insert("statusLine".into(), Value::Object(line));
    write_json_object(claude_settings, root)
}

/// Remove the bridge and put back the user's previous status line.
pub fn disconnect(claude_settings: &Path, settings: &mut Settings) -> Result<(), String> {
    let mut root = read_json_object(claude_settings)?;
    let ours = root.get("statusLine").map(is_our_status_line).unwrap_or(false);
    if ours {
        match settings.claude_previous_status_line.take() {
            Some(prev) => {
                root.insert("statusLine".into(), prev);
            }
            None => {
                root.shift_remove("statusLine");
            }
        }
        write_json_object(claude_settings, root)?;
    }
    settings.claude_previous_status_line = None;
    Ok(())
}

/// Uninstall cleanup. Never fails the uninstaller.
pub fn disconnect_installed() -> i32 {
    if let Some(file) = paths::claude_settings_file() {
        let mut settings = Settings::load();
        if disconnect(&file, &mut settings).is_ok() {
            let _ = settings.save();
        }
    }
    0
}

// ---------------------------------------------------------------- snapshot

/// Numbers from the status-line record, if it has any.
fn record_snapshot(record: &UsageRecord) -> Option<ProviderSnapshot> {
    let mut windows = Vec::new();
    if let Some(w) = record.five_hour {
        windows.push(UsageWindow {
            id: "session".into(),
            label: "5-hour session".into(),
            used_percent: w.used_percentage,
            resets_at: w.resets_at,
            window_minutes: Some(300),
        });
    }
    if let Some(w) = record.seven_day {
        windows.push(UsageWindow {
            id: "weekly".into(),
            label: "Weekly".into(),
            used_percent: w.used_percentage,
            resets_at: w.resets_at,
            window_minutes: Some(10080),
        });
    }
    if windows.is_empty() {
        return None;
    }
    Some(ProviderSnapshot {
        status: Status::Ok,
        windows,
        captured_at: record.captured_at,
        ..ProviderSnapshot::new("claude", "Claude", SOURCE, false)
    })
}

/// Combine the live reading (Claude Code `get_usage`) with the optional
/// status-line record, using whichever reading is newer.
pub fn merge(enabled: bool, live: Option<ProviderSnapshot>, record: Option<UsageRecord>) -> ProviderSnapshot {
    let base = ProviderSnapshot::new("claude", "Claude", crate::claude_live::SOURCE, true);
    if !enabled {
        return base.with_status(Status::Disabled, None);
    }
    let from_record = record.as_ref().and_then(record_snapshot);
    match (live, from_record) {
        (Some(mut l), Some(r)) if l.status == Status::Ok => {
            if r.captured_at > l.captured_at {
                // A Claude Code reply reported newer numbers than the last check.
                for w in &mut l.windows {
                    if let Some(rw) = r.windows.iter().find(|rw| rw.id == w.id) {
                        w.used_percent = rw.used_percent;
                        w.resets_at = rw.resets_at;
                    }
                }
                l.captured_at = r.captured_at;
            }
            l
        }
        (Some(l), _) if l.status == Status::Ok => l,
        // Live check unavailable: fall back to the status-line numbers.
        (_, Some(r)) => r,
        (Some(l), None) => l,
        (None, None) => base.with_status(Status::Waiting, "Checking Claude Code…".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(five: f64, five_reset: i64, seven: f64, seven_reset: i64) -> Value {
        json!({
            "model": { "display_name": "Opus" },
            "rate_limits": {
                "five_hour": { "used_percentage": five, "resets_at": five_reset },
                "seven_day": { "used_percentage": seven, "resets_at": seven_reset }
            }
        })
    }

    #[test]
    fn records_limits_from_status_line_payload() {
        let mut r = UsageRecord::default();
        apply_payload(&mut r, &payload(42.0, 1000, 18.0, 9000), 5);
        assert_eq!(r.five_hour, Some(LimitWindow { used_percentage: 42.0, resets_at: Some(1000) }));
        assert_eq!(r.seven_day, Some(LimitWindow { used_percentage: 18.0, resets_at: Some(9000) }));
        assert_eq!(r.captured_at, Some(5));
    }

    #[test]
    fn payload_without_limits_only_marks_seen() {
        let mut r = UsageRecord::default();
        apply_payload(&mut r, &json!({ "model": {} }), 7);
        assert_eq!(r.last_seen_at, Some(7));
        assert_eq!(r.captured_at, None);
        assert!(r.five_hour.is_none());
    }

    #[test]
    fn older_session_cannot_lower_usage_in_same_window() {
        let mut r = UsageRecord::default();
        apply_payload(&mut r, &payload(60.0, 1000, 30.0, 9000), 1);
        apply_payload(&mut r, &payload(40.0, 1030, 20.0, 9000), 2);
        assert_eq!(r.five_hour.unwrap().used_percentage, 60.0);
        assert_eq!(r.seven_day.unwrap().used_percentage, 30.0);
    }

    #[test]
    fn newer_window_replaces_old_one() {
        let mut r = UsageRecord::default();
        apply_payload(&mut r, &payload(95.0, 1000, 30.0, 9000), 1);
        apply_payload(&mut r, &payload(3.0, 19000, 31.0, 9000), 2);
        assert_eq!(r.five_hour.unwrap().used_percentage, 3.0);
        assert_eq!(r.five_hour.unwrap().resets_at, Some(19000));
        // ...and a stale payload from the old window is ignored.
        apply_payload(&mut r, &payload(97.0, 1000, 31.0, 9000), 3);
        assert_eq!(r.five_hour.unwrap().used_percentage, 3.0);
    }

    #[test]
    fn bridge_command_uses_forward_slashes() {
        let cmd = bridge_command(Path::new("C:\\Apps\\Limi\\limi.exe"));
        assert_eq!(cmd, "C:/Apps/Limi/limi.exe --statusline");
    }

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("limi-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn connect_and_disconnect_restore_user_status_line() {
        let file = temp_file("restore");
        let original = json!({
            "model": "sonnet",
            "statusLine": { "type": "command", "command": "~/.claude/mine.sh", "padding": 1 },
            "hooks": {}
        });
        std::fs::write(&file, serde_json::to_string_pretty(&original).unwrap()).unwrap();
        let exe = Path::new("C:/Apps/Limi/limi.exe");
        let mut s = Settings::default();

        connect(&file, exe, &mut s).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert!(is_our_status_line(&after["statusLine"]));
        assert_eq!(after["statusLine"]["padding"], 1);
        assert_eq!(after["model"], "sonnet");
        assert_eq!(s.previous_status_line_command().as_deref(), Some("~/.claude/mine.sh"));
        assert_eq!(bridge_state(&file, exe), Bridge::Connected);
        assert_eq!(bridge_state(&file, Path::new("C:/Other/limi.exe")), Bridge::Stale);

        // Reconnecting must not overwrite the saved user command with our own.
        connect(&file, exe, &mut s).unwrap();
        assert_eq!(s.previous_status_line_command().as_deref(), Some("~/.claude/mine.sh"));

        disconnect(&file, &mut s).unwrap();
        let restored: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(restored, original);
        assert!(s.claude_previous_status_line.is_none());
    }

    #[test]
    fn bridge_from_former_name_is_ours_and_gets_repointed() {
        let file = temp_file("legacy");
        std::fs::write(&file, r#"{"statusLine":{"type":"command","command":"C:/Users/x/AppData/Local/Headroom/headroom.exe --statusline"}}"#).unwrap();
        assert_eq!(bridge_state(&file, Path::new("C:/Apps/Limi/limi.exe")), Bridge::Stale);
    }

    #[test]
    fn disconnect_removes_key_when_user_had_none() {
        let file = temp_file("remove");
        std::fs::write(&file, r#"{"model":"opus"}"#).unwrap();
        let exe = Path::new("C:/Apps/limi.exe");
        let mut s = Settings::default();
        connect(&file, exe, &mut s).unwrap();
        disconnect(&file, &mut s).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(v, json!({ "model": "opus" }));
    }

    #[test]
    fn invalid_settings_file_is_never_overwritten() {
        let file = temp_file("invalid");
        std::fs::write(&file, "{ not json").unwrap();
        let mut s = Settings::default();
        assert!(connect(&file, Path::new("C:/h/limi.exe"), &mut s).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
    }

    #[test]
    fn merge_prefers_newer_reading() {
        let live = ProviderSnapshot {
            status: Status::Ok,
            windows: vec![UsageWindow { id: "session".into(), label: "5-hour session".into(), used_percent: 70.0, resets_at: Some(1000), window_minutes: Some(300) }],
            captured_at: Some(10),
            ..ProviderSnapshot::new("claude", "Claude", "Claude Code", true)
        };
        let mut r = UsageRecord::default();
        apply_payload(&mut r, &payload(42.0, 1000, 18.0, 9000), 5);
        // Older status-line numbers never override the live reading.
        assert_eq!(merge(true, Some(live.clone()), Some(r.clone())).windows[0].used_percent, 70.0);
        // A newer reply does.
        apply_payload(&mut r, &payload(74.0, 1000, 19.0, 9000), 20);
        let m = merge(true, Some(live), Some(r.clone()));
        assert_eq!((m.windows[0].used_percent, m.captured_at), (74.0, Some(20)));
        // Without a live reading the record is used; with neither, we wait.
        assert_eq!(merge(true, None, Some(r)).status, Status::Ok);
        assert_eq!(merge(true, None, None).status, Status::Waiting);
        assert_eq!(merge(false, None, None).status, Status::Disabled);
    }
}
