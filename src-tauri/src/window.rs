//! Docking: the widget hugs the left or right edge of a monitor's work area.
//! The rail keeps its screen position when the panel expands around it.

use serde::Serialize;
use tauri::{Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::settings::{Edge, Settings};

/// Gap between the window and the screen edge (logical px). The rail sits flush
/// against the edge; the page adds transparent padding on the other sides for its shadow.
const EDGE_GAP: f64 = 0.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInfo {
    pub edge: Edge,
    /// Where the rail's top sits inside the window, in logical px.
    pub rail_top: f64,
    /// Window height in logical px (may be less than requested on short screens).
    pub height: f64,
}

struct Area {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: f64,
}

fn area(m: &Monitor) -> Area {
    let wa = m.work_area();
    Area {
        x: wa.position.x,
        y: wa.position.y,
        w: wa.size.width as i32,
        h: wa.size.height as i32,
        scale: m.scale_factor(),
    }
}

fn target_monitor(window: &WebviewWindow, settings: &Settings) -> Option<Monitor> {
    let monitors = window.available_monitors().ok()?;
    if let Some(name) = &settings.monitor {
        if let Some(m) = monitors.iter().find(|m| m.name() == Some(name)) {
            return Some(m.clone());
        }
    }
    window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| monitors.into_iter().next())
}

/// Size and place the window. `rail_h` is the compact height (logical px).
pub fn apply(
    window: &WebviewWindow,
    settings: &Settings,
    width: f64,
    height: f64,
    rail_h: f64,
    animate: bool,
) -> tauri::Result<LayoutInfo> {
    let Some(monitor) = target_monitor(window, settings) else {
        return Ok(LayoutInfo { edge: settings.edge, rail_top: 0.0, height });
    };
    let a = area(&monitor);
    let px = |v: f64| (v * a.scale).round() as i32;
    let (w, h, rh, gap) = (px(width), px(height).min(a.h), px(rail_h), px(EDGE_GAP));

    let x = match settings.edge {
        Edge::Right => a.x + a.w - w - gap,
        Edge::Left => a.x + gap,
    };
    let rail_y = a.y + ((a.h - rh).max(0) as f64 * settings.top_ratio).round() as i32;
    let y = rail_y.min(a.y + a.h - h).max(a.y);

    let info = LayoutInfo {
        edge: settings.edge,
        rail_top: ((rail_y - y) as f64 / a.scale).max(0.0),
        height: h as f64 / a.scale,
    };

    // Glide to the edge after a drag instead of jumping.
    if animate {
        if let (Ok(from), Ok(size)) = (window.outer_position(), window.outer_size()) {
            if size.width as i32 == w && size.height as i32 == h {
                let win = window.clone();
                std::thread::spawn(move || {
                    const STEPS: i32 = 14;
                    for i in 1..=STEPS {
                        let t = i as f64 / STEPS as f64;
                        let e = 1.0 - (1.0 - t).powi(3);
                        let xi = from.x + ((x - from.x) as f64 * e).round() as i32;
                        let yi = from.y + ((y - from.y) as f64 * e).round() as i32;
                        let _ = set_bounds(&win, xi, yi, w, h);
                        std::thread::sleep(std::time::Duration::from_millis(12));
                    }
                });
                return Ok(info);
            }
        }
    }

    set_bounds(window, x, y, w, h)?;
    if !window.is_visible().unwrap_or(true) {
        window.show()?;
    }
    Ok(info)
}

/// After a drag: dock the rail to the nearest edge of the monitor it was dropped on.
/// `rail_x` is the rail's center and `rail_top`/`rail_h` its box, in logical px
/// inside the window (the window is wider than the rail).
pub fn snap(window: &WebviewWindow, settings: &mut Settings, rail_x: f64, rail_top: f64, rail_h: f64) -> tauri::Result<()> {
    let pos = window.outer_position()?;
    let scale = window.scale_factor()?;
    let cx = pos.x + (rail_x * scale).round() as i32;
    let top = pos.y + (rail_top * scale).round() as i32;
    let height = (rail_h * scale).round() as i32;
    let cy = top + height / 2;

    let monitors = window.available_monitors()?;
    let contains = |m: &&Monitor| {
        let (p, s) = (m.position(), m.size());
        cx >= p.x && cx < p.x + s.width as i32 && cy >= p.y && cy < p.y + s.height as i32
    };
    let Some(monitor) = monitors
        .iter()
        .find(contains)
        .cloned()
        .or_else(|| window.current_monitor().ok().flatten())
    else {
        return Ok(());
    };
    let a = area(&monitor);
    settings.edge = if cx < a.x + a.w / 2 { Edge::Left } else { Edge::Right };
    let free = a.h - height;
    settings.top_ratio = if free > 0 {
        ((top - a.y) as f64 / free as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    settings.monitor = monitor.name().cloned();
    Ok(())
}

/// True while the primary mouse button is held (e.g. during an OS window drag).
#[cfg(windows)]
pub fn mouse_button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    (unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } as u16 & 0x8000) != 0
}

#[cfg(not(windows))]
pub fn mouse_button_down() -> bool {
    false
}

/// Move and resize in one step so the rail doesn't jump between two calls.
#[cfg(windows)]
fn set_bounds(window: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    let hwnd = window.hwnd()?;
    let ok = unsafe {
        SetWindowPos(hwnd.0 as _, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE)
    };
    if ok == 0 {
        window.set_position(PhysicalPosition::new(x, y))?;
        window.set_size(PhysicalSize::new(w as u32, h as u32))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_bounds(window: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) -> tauri::Result<()> {
    window.set_size(PhysicalSize::new(w as u32, h as u32))?;
    window.set_position(PhysicalPosition::new(x, y))
}

/// Interactive area of the window in logical px: x, y, width, height.
pub type HitRect = [f64; 4];

/// The window is larger than the visible glass so it never has to resize while
/// animating. Outside the glass it must not block the desktop: watch the cursor
/// and let clicks fall through unless it is over the glass. Tells the page when
/// the cursor enters or leaves.
pub fn spawn_hit_test(window: WebviewWindow, rect: std::sync::Arc<std::sync::Mutex<Option<HitRect>>>) {
    use tauri::Emitter;
    std::thread::spawn(move || {
        let mut inside_last: Option<bool> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(25));
            let Some(r) = *rect.lock().unwrap() else { continue };
            let (Some((cx, cy)), Ok(pos), Ok(scale)) = (cursor(), window.outer_position(), window.scale_factor()) else {
                continue;
            };
            let x = (cx - pos.x) as f64 / scale;
            let y = (cy - pos.y) as f64 / scale;
            let inside = x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3];
            // Keep receiving events while a button is held (e.g. dragging the rail).
            let inside = inside || (inside_last == Some(true) && mouse_button_down());
            if inside_last != Some(inside) {
                let _ = window.set_ignore_cursor_events(!inside);
                let _ = window.emit("hover", inside);
                inside_last = Some(inside);
            }
        }
    });
}

#[cfg(windows)]
fn cursor() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
}

#[cfg(not(windows))]
fn cursor() -> Option<(i32, i32)> {
    None
}
