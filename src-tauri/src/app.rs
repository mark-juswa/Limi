//! Application wiring: shared state, events to the UI, alerts, commands and tray.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;
use tauri_plugin_updater::UpdaterExt as _;
use tokio::sync::mpsc::UnboundedSender;

use crate::claude::{self, Bridge};
use crate::claude_live::{self, Fetch};
use crate::codex::{self, Msg};
use crate::model::{ProviderSnapshot, Status};
use crate::paths;
use crate::settings::{Edge, Settings, Theme};
use crate::tokens::{Scanner, TokenUsage};
use crate::window::{self, HitRect, LayoutInfo};

/// How often the status-line record file is re-read.
const RECORD_POLL: Duration = Duration::from_secs(3);
/// Claude's usage service rate-limits frequent checks (Claude Code then
/// serves a cached copy and waits 5 minutes), so Claude is checked at most
/// this often on schedule...
const CLAUDE_INTERVAL: Duration = Duration::from_secs(300);
/// ...and at most this often when new Claude activity asks for a refresh.
const CLAUDE_MIN_GAP: Duration = Duration::from_secs(180);
const TOKEN_SCAN: Duration = Duration::from_secs(15);

pub struct AppState {
    settings: Mutex<Settings>,
    claude: Mutex<ProviderSnapshot>,
    codex: Mutex<ProviderSnapshot>,
    codex_tx: Mutex<Option<UnboundedSender<Msg>>>,
    claude_tx: Mutex<Option<UnboundedSender<Msg>>>,
    /// Last reading from Claude Code's `get_usage`.
    claude_live: Mutex<Option<ProviderSnapshot>>,
    alerted: Mutex<HashSet<String>>,
    hit: std::sync::Arc<Mutex<Option<HitRect>>>,
    tokens: Mutex<Tokens>,
}

/// Exact token counts from local logs, per provider.
#[derive(Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    claude: Option<TokenUsage>,
    codex: Option<TokenUsage>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    claude: ProviderSnapshot,
    codex: ProviderSnapshot,
    tokens: Tokens,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatePayload {
    settings: Settings,
    usage: Usage,
    version: &'static str,
    autostart: bool,
    /// Usage is also shown in Claude Code's status line.
    claude_status_line: bool,
}

fn state(app: &AppHandle) -> &AppState {
    app.state::<AppState>().inner()
}

pub fn settings(app: &AppHandle) -> Settings {
    state(app).settings.lock().unwrap().clone()
}

fn usage(app: &AppHandle) -> Usage {
    let s = state(app);
    let claude = s.claude.lock().unwrap().clone();
    let codex = s.codex.lock().unwrap().clone();
    let tokens = s.tokens.lock().unwrap().clone();
    Usage { claude, codex, tokens }
}

pub fn current_plan(app: &AppHandle, id: &str) -> Option<String> {
    let s = state(app);
    let slot = if id == "claude" { &s.claude } else { &s.codex };
    let plan = slot.lock().unwrap().plan.clone();
    plan
}

/// Store a provider snapshot, tell the UI, update the tray, fire alerts.
pub fn publish(app: &AppHandle, snap: ProviderSnapshot) {
    {
        let s = state(app);
        let slot = if snap.id == "claude" { &s.claude } else { &s.codex };
        let mut cur = slot.lock().unwrap();
        if serde_json::to_value(&*cur).ok() == serde_json::to_value(&snap).ok() {
            return;
        }
        *cur = snap.clone();
    }
    alert(app, &snap);
    let u = usage(app);
    update_tray_tooltip(app, &u);
    let _ = app.emit("usage", u);
}

/// Keep the last known numbers on screen but flag the problem.
pub fn publish_error(app: &AppHandle, id: &str, message: String) {
    let mut snap = {
        let s = state(app);
        let slot = if id == "claude" { &s.claude } else { &s.codex };
        let snap = slot.lock().unwrap().clone();
        snap
    };
    snap.status = Status::Error;
    snap.message = Some(message);
    publish(app, snap);
}

// ---------------------------------------------------------------- alerts

fn alert(app: &AppHandle, snap: &ProviderSnapshot) {
    let threshold = settings(app).alert_threshold;
    if threshold == 0 || snap.status != Status::Ok {
        return;
    }
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    for w in &snap.windows {
        if w.resets_at.is_some_and(|r| r <= now) {
            continue;
        }
        let level = if w.used_percent >= 100.0 {
            100
        } else if w.used_percent >= threshold as f64 {
            threshold
        } else {
            continue;
        };
        // One alert per level per window; reset times are bucketed to absorb jitter.
        let key = format!("{}:{}:{}:{level}", snap.id, w.id, w.resets_at.unwrap_or(0) / 600);
        if !state(app).alerted.lock().unwrap().insert(key) {
            continue;
        }
        let what = w.label.to_lowercase();
        let title = if level == 100 {
            format!("{} {what} limit reached", snap.name)
        } else {
            format!("{} is at {:.0}% of its {what} limit", snap.name, w.used_percent)
        };
        let body = match w.resets_at {
            Some(r) => format!("Resets in {}.", human_duration(r - now)),
            None => "Open Limi for details.".into(),
        };
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

fn human_duration(secs: i64) -> String {
    let secs = secs.max(60);
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} d {h} h"),
    }
}

// ---------------------------------------------------------------- claude watcher

fn current_exe() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()
}

fn status_line_connected() -> bool {
    match (paths::claude_settings_file(), current_exe()) {
        (Some(file), Some(exe)) => claude::bridge_state(&file, &exe) != Bridge::NotConnected,
        _ => false,
    }
}

/// Publish Claude's numbers: the live reading, or a newer status-line record.
fn refresh_claude(app: &AppHandle) {
    let s = settings(app);
    if let (Some(file), Some(exe)) = (paths::claude_settings_file(), current_exe()) {
        if claude::bridge_state(&file, &exe) == Bridge::Stale {
            // Limi moved (reinstalled elsewhere or renamed): point the status line at this copy.
            let mut s = state(app).settings.lock().unwrap();
            if claude::connect(&file, &exe, &mut s).is_ok() {
                let _ = s.save();
            }
        }
    }
    let live = state(app).claude_live.lock().unwrap().clone();
    let record = claude::read_record(&paths::claude_usage_file());
    publish(app, claude::merge(s.claude_enabled, live, record));
}

fn live_snapshot(fetch: Fetch, previous: Option<ProviderSnapshot>) -> ProviderSnapshot {
    let base = ProviderSnapshot::new("claude", "Claude", claude_live::SOURCE, true);
    match fetch {
        Fetch::Ok { windows, plan, fresh: true } => ProviderSnapshot {
            status: Status::Ok,
            windows,
            plan,
            captured_at: Some(paths::now_ms()),
            ..base
        },
        // Claude Code answered from its cache: keep a newer reading if we have
        // one, and otherwise show the cache with its real age.
        Fetch::Ok { windows, plan, fresh: false } => {
            let cached = claude_live::cached_at();
            match previous.filter(|p| p.status == Status::Ok && p.captured_at >= cached) {
                Some(p) => p,
                None => ProviderSnapshot {
                    status: Status::Ok,
                    windows,
                    plan,
                    captured_at: cached,
                    live: false,
                    ..base
                },
            }
        }
        Fetch::NoLimits => base.with_status(
            Status::NoLimits,
            "Claude Code is signed in without a Pro or Max plan, so there are no plan limits to show.".to_string(),
        ),
        Fetch::SignedOut => base.with_status(
            Status::SignedOut,
            "Claude Code isn't signed in. Run `claude` and use /login.".to_string(),
        ),
        // Keep the last numbers on screen and flag the problem.
        Fetch::Failed(msg) => match previous.filter(|p| !p.windows.is_empty()) {
            Some(mut p) => {
                p.status = Status::Error;
                p.message = Some(msg);
                p
            }
            None => base.with_status(Status::Error, msg),
        },
    }
}

/// Check Claude Code's plan usage on the poll interval and on demand.
fn spawn_claude_live(app: AppHandle) -> UnboundedSender<Msg> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
    tauri::async_runtime::spawn(async move {
        loop {
            let s = settings(&app);
            let mut gap = CLAUDE_MIN_GAP;
            if s.claude_enabled {
                let snap = match claude_live::resolve(s.claude_path.as_deref()) {
                    Some(exe) => {
                        let previous = state(&app).claude_live.lock().unwrap().clone();
                        let fetch = claude_live::fetch(&exe).await;
                        if matches!(fetch, Fetch::Ok { fresh: false, .. }) {
                            // Refused: give the service its full cool-down.
                            gap = CLAUDE_INTERVAL;
                        }
                        live_snapshot(fetch, previous)
                    }
                    None => ProviderSnapshot::new("claude", "Claude", claude_live::SOURCE, true).with_status(
                        Status::NotInstalled,
                        "Claude Code wasn't found. Limi reads your plan usage through it. Install Claude Code, or set its path in Settings.".to_string(),
                    ),
                };
                *state(&app).claude_live.lock().unwrap() = Some(snap);
            }
            refresh_claude(&app);
            tokio::time::sleep(gap).await;
            // Requests that arrived during the gap are covered by the next check.
            let mut pending = false;
            while rx.try_recv().is_ok() {
                pending = true;
            }
            if pending {
                continue;
            }
            let wait = Duration::from_secs(s.poll_seconds).max(CLAUDE_INTERVAL).saturating_sub(gap);
            tokio::select! {
                m = rx.recv() => if m.is_none() { return },
                _ = tokio::time::sleep(wait) => {}
            }
        }
    });
    tx
}

/// Pick up status-line records written by Claude Code replies.
fn spawn_record_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(RECORD_POLL).await;
            refresh_claude(&app);
        }
    });
}

fn send(slot: &Mutex<Option<UnboundedSender<Msg>>>, msg: Msg) {
    if let Some(tx) = slot.lock().unwrap().as_ref() {
        let _ = tx.send(msg);
    }
}

// ---------------------------------------------------------------- token counts

/// Start of the provider's current window, from its reported reset time;
/// a rolling window when the provider hasn't reported one.
fn window_start(snap: &ProviderSnapshot, which: &str, fallback_min: i64, now_ms: i64) -> i64 {
    if let Some(w) = snap.windows.iter().find(|w| w.id == which) {
        if let (Some(reset), Some(mins)) = (w.resets_at, w.window_minutes) {
            if reset * 1000 > now_ms {
                return (reset - mins as i64 * 60) * 1000;
            }
        }
    }
    now_ms - fallback_min * 60_000
}

fn spawn_token_scanner(app: AppHandle) {
    std::thread::spawn(move || {
        let mut scanner = Scanner::new();
        let mut seen: [Option<i64>; 2] = [None, None];
        let mut first = true;
        loop {
            let now = paths::now_ms();
            scanner.scan(now);
            let count = |snap: ProviderSnapshot, enabled: bool| -> Option<TokenUsage> {
                enabled.then(|| {
                    scanner.usage(
                        snap.id,
                        window_start(&snap, "session", 300, now),
                        window_start(&snap, "weekly", 10_080, now),
                    )
                })
            };
            let s = settings(&app);
            let claude = state(&app).claude.lock().unwrap().clone();
            let codex = state(&app).codex.lock().unwrap().clone();
            let next = Tokens {
                claude: count(claude, s.claude_enabled),
                codex: count(codex, s.codex_enabled),
            };
            // New activity in the logs: ask the provider for fresh numbers now
            // instead of waiting for the next scheduled check.
            let latest = [
                next.claude.as_ref().and_then(|t| t.last_activity_at),
                next.codex.as_ref().and_then(|t| t.last_activity_at),
            ];
            if !first {
                if latest[0] > seen[0] {
                    send(&state(&app).claude_tx, Msg::Refresh);
                }
                if latest[1] > seen[1] {
                    send(&state(&app).codex_tx, Msg::Refresh);
                }
            }
            seen = latest;
            first = false;
            let changed = {
                let mut cur = state(&app).tokens.lock().unwrap();
                let changed = *cur != next;
                *cur = next;
                changed
            };
            if changed {
                let _ = app.emit("usage", usage(&app));
            }
            std::thread::sleep(TOKEN_SCAN);
        }
    });
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_state(app: AppHandle) -> StatePayload {
    StatePayload {
        settings: settings(&app),
        usage: usage(&app),
        version: env!("CARGO_PKG_VERSION"),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        claude_status_line: status_line_connected(),
    }
}

#[tauri::command]
fn refresh(app: AppHandle) {
    send(&state(&app).codex_tx, Msg::Refresh);
    send(&state(&app).claude_tx, Msg::Refresh);
}

#[tauri::command]
fn set_layout(
    window: WebviewWindow,
    app: AppHandle,
    width: f64,
    height: f64,
    rail_height: f64,
    animate: Option<bool>,
) -> Result<LayoutInfo, String> {
    window::apply(&window, &settings(&app), width, height, rail_height, animate.unwrap_or(false))
        .map_err(|e| e.to_string())
}

/// Dock after a drag. Returns `None` while the mouse button is still held.
#[tauri::command]
fn snap_to_edge(window: WebviewWindow, app: AppHandle, rail_x: f64, rail_top: f64, rail_height: f64) -> Result<Option<Settings>, String> {
    if window::mouse_button_down() {
        return Ok(None);
    }
    let mut s = state(&app).settings.lock().unwrap();
    window::snap(&window, &mut s, rail_x, rail_top, rail_height).map_err(|e| e.to_string())?;
    let _ = s.save();
    Ok(Some(s.clone()))
}

/// The part of the window that should take clicks (logical px).
#[tauri::command]
fn set_hit_rect(app: AppHandle, x: f64, y: f64, width: f64, height: f64) {
    *state(&app).hit.lock().unwrap() = Some([x, y, width, height]);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    claude_enabled: Option<bool>,
    codex_enabled: Option<bool>,
    codex_path: Option<String>,
    claude_path: Option<String>,
    poll_seconds: Option<u64>,
    alert_threshold: Option<u8>,
    always_on_top: Option<bool>,
    edge: Option<Edge>,
    theme: Option<Theme>,
}

#[tauri::command]
fn update_settings(app: AppHandle, patch: SettingsPatch) -> Result<Settings, String> {
    let (updated, codex_changed) = {
        let mut s = state(&app).settings.lock().unwrap();
        let before = (s.codex_enabled, s.codex_path.clone(), s.poll_seconds);
        if let Some(v) = patch.claude_enabled { s.claude_enabled = v; }
        if let Some(v) = patch.codex_enabled { s.codex_enabled = v; }
        if let Some(v) = patch.codex_path {
            s.codex_path = Some(v.trim().to_owned()).filter(|p| !p.is_empty());
        }
        if let Some(v) = patch.claude_path {
            s.claude_path = Some(v.trim().to_owned()).filter(|p| !p.is_empty());
        }
        if let Some(v) = patch.poll_seconds { s.poll_seconds = v; }
        if let Some(v) = patch.alert_threshold {
            s.alert_threshold = v;
            state(&app).alerted.lock().unwrap().clear();
        }
        if let Some(v) = patch.always_on_top { s.always_on_top = v; }
        if let Some(v) = patch.edge { s.edge = v; }
        if let Some(v) = patch.theme { s.theme = v; }
        *s = s.clone().sanitized();
        s.save().map_err(|e| e.to_string())?;
        let after = (s.codex_enabled, s.codex_path.clone(), s.poll_seconds);
        (s.clone(), before != after)
    };
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(updated.always_on_top);
    }
    if codex_changed {
        send(&state(&app).codex_tx, Msg::Reconfigure);
    }
    send(&state(&app).claude_tx, Msg::Refresh);
    refresh_claude(&app);
    Ok(updated)
}

#[tauri::command]
fn claude_connect(app: AppHandle) -> Result<(), String> {
    let file = paths::claude_settings_file().ok_or("Couldn't find your home folder.")?;
    let exe = current_exe().ok_or("Couldn't locate Limi.")?;
    {
        let mut s = state(&app).settings.lock().unwrap();
        claude::connect(&file, &exe, &mut s)?;
        s.save().map_err(|e| e.to_string())?;
    }
    refresh_claude(&app);
    Ok(())
}

#[tauri::command]
fn claude_disconnect(app: AppHandle) -> Result<(), String> {
    let file = paths::claude_settings_file().ok_or("Couldn't find your home folder.")?;
    {
        let mut s = state(&app).settings.lock().unwrap();
        claude::disconnect(&file, &mut s)?;
        s.save().map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(paths::claude_usage_file());
    refresh_claude(&app);
    Ok(())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| e.to_string())?;
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| u.version))
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = update {
        update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

#[tauri::command]
fn hide_widget(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------- tray

fn update_tray_tooltip(app: &AppHandle, u: &Usage) {
    let line = |p: &ProviderSnapshot| -> Option<String> {
        if p.status != Status::Ok {
            return None;
        }
        let parts: Vec<String> = p.windows.iter().map(|w| format!("{:.0}%", w.used_percent)).collect();
        Some(format!("{} {}", p.name, parts.join(" / ")))
    };
    let lines: Vec<String> = [line(&u.claude), line(&u.codex)].into_iter().flatten().collect();
    let text = if lines.is_empty() { "Limi".to_string() } else { format!("Limi\n{}", lines.join("\n")) };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(text));
    }
}

fn toggle_widget(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
        }
    }
}

fn show_widget(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / hide widget", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let refresh_item = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Limi", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &refresh_item, &settings, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Limi")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => toggle_widget(app),
            "refresh" => refresh(app.clone()),
            "settings" => {
                show_widget(app);
                let _ = app.emit("open-settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_widget(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

// ---------------------------------------------------------------- run

pub fn run() {
    paths::migrate_legacy();
    let first_run = !paths::settings_file().exists();
    let settings = Settings::load();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_widget(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            claude: Mutex::new(claude::merge(settings.claude_enabled, None, None)),
            claude_live: Mutex::new(None),
            claude_tx: Mutex::new(None),
            codex: Mutex::new(ProviderSnapshot::new("codex", "Codex", codex::SOURCE, true)),
            settings: Mutex::new(settings.clone()),
            codex_tx: Mutex::new(None),
            alerted: Mutex::new(HashSet::new()),
            hit: Default::default(),
            tokens: Mutex::new(Tokens::default()),
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            refresh,
            set_layout,
            snap_to_edge,
            set_hit_rect,
            update_settings,
            claude_connect,
            claude_disconnect,
            set_autostart,
            check_update,
            install_update,
            hide_widget,
            quit
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            if first_run {
                let _ = settings.save();
                if !cfg!(debug_assertions) {
                    let _ = handle.autolaunch().enable();
                }
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_always_on_top(settings.always_on_top);
                window::spawn_hit_test(w, state(&handle).hit.clone());
            }
            build_tray(&handle)?;
            *state(&handle).codex_tx.lock().unwrap() = Some(codex::spawn(handle.clone()));
            *state(&handle).claude_tx.lock().unwrap() = Some(spawn_claude_live(handle.clone()));
            spawn_record_watcher(handle.clone());
            spawn_token_scanner(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Limi");
}
