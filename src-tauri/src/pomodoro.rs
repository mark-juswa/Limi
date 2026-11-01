//! Pomodoro focus timer: focus rounds with short breaks, and a long break
//! after every fourth round.
//!
//! The timer lives here, not in the UI, so it keeps exact time and still
//! notifies when a round ends while the widget is hidden.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt as _;

use crate::paths::now_ms;
use crate::settings::Settings;

/// Focus rounds before a long break.
pub const ROUNDS: u32 = 4;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Focus,
    Break,
    LongBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Durations {
    pub focus_ms: i64,
    pub break_ms: i64,
    pub long_break_ms: i64,
}

impl Durations {
    pub fn from_settings(s: &Settings) -> Self {
        let ms = |min: u32| i64::from(min) * 60_000;
        Self {
            focus_ms: ms(s.focus_minutes),
            break_ms: ms(s.break_minutes),
            long_break_ms: ms(s.long_break_minutes),
        }
    }

    fn of(&self, phase: Phase) -> i64 {
        match phase {
            Phase::Focus => self.focus_ms,
            Phase::Break => self.break_ms,
            Phase::LongBreak => self.long_break_ms,
        }
    }
}

/// What the UI draws. While running it counts down to `ends_at` itself.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub phase: Phase,
    pub running: bool,
    /// Epoch ms when the current phase ends (while running).
    pub ends_at: Option<i64>,
    /// Time left while paused.
    pub remaining_ms: i64,
    pub total_ms: i64,
    /// Focus rounds finished in this cycle (0..ROUNDS).
    pub done: u32,
    pub rounds: u32,
}

impl Timer {
    pub fn new(d: &Durations) -> Self {
        Self {
            phase: Phase::Focus,
            running: false,
            ends_at: None,
            remaining_ms: d.focus_ms,
            total_ms: d.focus_ms,
            done: 0,
            rounds: ROUNDS,
        }
    }

    fn start_phase(&mut self, phase: Phase, d: &Durations) {
        self.phase = phase;
        self.total_ms = d.of(phase);
        self.remaining_ms = self.total_ms;
        self.running = false;
        self.ends_at = None;
    }

    /// Start or pause.
    pub fn toggle(&mut self, now: i64) {
        if self.running {
            self.remaining_ms = (self.ends_at.unwrap_or(now) - now).max(0);
            self.ends_at = None;
            self.running = false;
        } else {
            self.ends_at = Some(now + self.remaining_ms);
            self.running = true;
        }
    }

    /// Restart the current phase. On a phase that's already untouched, start
    /// the whole cycle over.
    pub fn reset(&mut self, d: &Durations) {
        let untouched = !self.running && self.remaining_ms == self.total_ms;
        if untouched {
            *self = Self::new(d);
        } else {
            self.start_phase(self.phase, d);
        }
    }

    /// Move to the next phase. A finished focus round counts toward the long
    /// break; a skipped one doesn't.
    pub fn advance(&mut self, d: &Durations, finished: bool) {
        let next = match self.phase {
            Phase::Focus => {
                if finished {
                    self.done += 1;
                }
                if self.done >= ROUNDS {
                    Phase::LongBreak
                } else {
                    Phase::Break
                }
            }
            Phase::LongBreak => {
                self.done = 0;
                Phase::Focus
            }
            Phase::Break => Phase::Focus,
        };
        self.start_phase(next, d);
    }

    /// End the phase if its time is up. Returns the phase that just ended.
    pub fn tick(&mut self, now: i64, d: &Durations) -> Option<Phase> {
        match self.ends_at {
            Some(end) if self.running && now >= end => {
                let ended = self.phase;
                self.advance(d, true);
                Some(ended)
            }
            _ => None,
        }
    }

    /// Apply new lengths to a phase that hasn't started yet.
    pub fn set_durations(&mut self, d: &Durations) {
        if !self.running && self.remaining_ms == self.total_ms {
            self.total_ms = d.of(self.phase);
            self.remaining_ms = self.total_ms;
        }
    }
}

// ---------------------------------------------------------------- app wiring

pub struct State(pub Mutex<Timer>);

pub fn init(settings: &Settings) -> State {
    State(Mutex::new(Timer::new(&Durations::from_settings(settings))))
}

fn durations(app: &AppHandle) -> Durations {
    Durations::from_settings(&crate::app::settings(app))
}

/// Run `f` on the shared timer and return a copy of the result.
fn with_timer<R>(app: &AppHandle, f: impl FnOnce(&mut Timer) -> R) -> (R, Timer) {
    let state = app.state::<State>();
    let mut t = state.0.lock().unwrap();
    let r = f(&mut t);
    (r, t.clone())
}

pub fn current(app: &AppHandle) -> Timer {
    with_timer(app, |_| ()).1
}

fn changed(app: &AppHandle, t: &Timer) {
    let _ = app.emit("pomodoro", t);
}

#[tauri::command]
pub fn pomodoro(app: AppHandle, action: String) -> Result<Timer, String> {
    let d = durations(&app);
    if !matches!(action.as_str(), "toggle" | "reset" | "skip") {
        return Err(format!("Unknown timer action: {action}"));
    }
    let (_, t) = with_timer(&app, |t| match action.as_str() {
        "toggle" => t.toggle(now_ms()),
        "reset" => t.reset(&d),
        _ => t.advance(&d, false),
    });
    changed(&app, &t);
    Ok(t)
}

/// Settings changed: new lengths, or the timer was turned off.
pub fn configure(app: &AppHandle, s: &Settings) {
    let d = Durations::from_settings(s);
    let (_, t) = with_timer(app, |t| {
        if s.pomodoro_enabled {
            t.set_durations(&d);
        } else {
            *t = Timer::new(&d);
        }
    });
    changed(app, &t);
}

/// Watch for the end of each phase and announce it.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        let d = durations(&app);
        let (ended, t) = with_timer(&app, |t| t.tick(now_ms(), &d));
        let Some(ended) = ended else { continue };
        changed(&app, &t);
        let mins = t.total_ms / 60_000;
        let (title, body) = match ended {
            Phase::Focus if t.phase == Phase::LongBreak => {
                ("Focus round done".to_string(), format!("All {ROUNDS} rounds done. Take a {mins}-minute long break."))
            }
            Phase::Focus => ("Focus round done".to_string(), format!("Time for a {mins}-minute break.")),
            _ => ("Break's over".to_string(), format!("Ready for {mins} minutes of focus?")),
        };
        let _ = app.notification().builder().title(title).body(body).show();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Durations = Durations { focus_ms: 25 * 60_000, break_ms: 5 * 60_000, long_break_ms: 15 * 60_000 };

    #[test]
    fn pause_keeps_time_left() {
        let mut t = Timer::new(&D);
        t.toggle(0);
        t.toggle(60_000);
        assert!(!t.running);
        assert_eq!(t.remaining_ms, 24 * 60_000);
        t.toggle(100_000);
        assert_eq!(t.ends_at, Some(100_000 + 24 * 60_000));
    }

    #[test]
    fn long_break_after_four_rounds() {
        let mut t = Timer::new(&D);
        for round in 1..=ROUNDS {
            t.toggle(0);
            assert_eq!(t.tick(D.focus_ms, &D), Some(Phase::Focus));
            assert_eq!(t.done, round);
            let expect = if round == ROUNDS { Phase::LongBreak } else { Phase::Break };
            assert_eq!(t.phase, expect);
            assert!(!t.running, "next phase waits for Start");
            t.toggle(0);
            t.tick(t.total_ms, &D);
            assert_eq!(t.phase, Phase::Focus);
        }
        assert_eq!(t.done, 0);
    }

    #[test]
    fn skip_doesnt_count_and_tick_waits() {
        let mut t = Timer::new(&D);
        t.toggle(0);
        assert_eq!(t.tick(D.focus_ms - 1, &D), None);
        t.advance(&D, false);
        assert_eq!((t.phase, t.done), (Phase::Break, 0));
    }

    #[test]
    fn reset_restarts_phase_then_cycle() {
        let mut t = Timer::new(&D);
        t.advance(&D, true);
        t.toggle(0);
        t.reset(&D);
        assert_eq!((t.phase, t.remaining_ms, t.done), (Phase::Break, D.break_ms, 1));
        t.reset(&D);
        assert_eq!(t, Timer::new(&D));
    }

    #[test]
    fn new_lengths_apply_only_before_start() {
        let mut t = Timer::new(&D);
        let longer = Durations { focus_ms: 50 * 60_000, ..D };
        t.set_durations(&longer);
        assert_eq!(t.remaining_ms, 50 * 60_000);
        t.toggle(0);
        t.set_durations(&D);
        assert_eq!(t.total_ms, 50 * 60_000);
    }
}
