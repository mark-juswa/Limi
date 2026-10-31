use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
}

/// Where the widget lives: the side rail, or centered on the top/bottom edge.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    #[default]
    Side,
    Top,
    Bottom,
}

/// Top/bottom look: attached to the edge (notch) or floating (Dynamic Island).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BarStyle {
    Notch,
    #[default]
    Island,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the Windows light/dark setting.
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: Theme,
    pub placement: Placement,
    pub bar_style: BarStyle,
    pub claude_enabled: bool,
    pub codex_enabled: bool,
    /// Optional explicit path to the codex executable.
    pub codex_path: Option<String>,
    /// Optional explicit path to the Claude Code executable.
    pub claude_path: Option<String>,
    /// Seconds between Codex checks.
    pub poll_seconds: u64,
    /// Notify when a limit passes this percentage. 0 turns alerts off.
    pub alert_threshold: u8,
    pub always_on_top: bool,
    pub edge: Edge,
    /// Vertical position of the rail as a fraction of the usable screen height.
    pub top_ratio: f64,
    pub monitor: Option<String>,
    /// The user's own Claude Code status line, kept so the bridge can run it
    /// and so disconnecting restores it exactly.
    pub claude_previous_status_line: Option<Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            placement: Placement::Side,
            bar_style: BarStyle::Island,
            claude_enabled: true,
            codex_enabled: true,
            codex_path: None,
            claude_path: None,
            poll_seconds: 60,
            alert_threshold: 85,
            always_on_top: true,
            edge: Edge::Right,
            top_ratio: 0.3,
            monitor: None,
            claude_previous_status_line: None,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read(paths::settings_file())
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .map(Settings::sanitized)
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        paths::write_atomic(&paths::settings_file(), &bytes)
    }

    /// Clamp values that may have been hand-edited.
    pub fn sanitized(mut self) -> Self {
        self.poll_seconds = self.poll_seconds.clamp(30, 900);
        self.alert_threshold = self.alert_threshold.min(100);
        if !self.top_ratio.is_finite() {
            self.top_ratio = 0.3;
        }
        self.top_ratio = self.top_ratio.clamp(0.0, 1.0);
        self
    }

    pub fn previous_status_line_command(&self) -> Option<String> {
        self.claude_previous_status_line
            .as_ref()?
            .get("command")?
            .as_str()
            .map(str::to_owned)
    }
}
