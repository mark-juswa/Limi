//! Data sent to the widget UI. Every number here comes from a provider;
//! nothing is estimated or invented.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// Stable id, e.g. "session" or "weekly".
    pub id: String,
    pub label: String,
    /// 0..=100 as reported by the provider.
    pub used_percent: f64,
    /// Unix epoch seconds when this window resets.
    pub resets_at: Option<i64>,
    pub window_minutes: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Numbers available.
    Ok,
    /// Connected, but the provider has not reported numbers yet.
    Waiting,
    /// CLI not found on this machine.
    NotInstalled,
    SignedOut,
    /// Account has no plan limits (e.g. API key billing).
    NoLimits,
    Error,
    Disabled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub id: &'static str,
    pub name: &'static str,
    pub status: Status,
    pub message: Option<String>,
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
    /// When the provider produced these numbers (epoch ms).
    pub captured_at: Option<i64>,
    /// Where the numbers come from, shown in the UI.
    pub source: &'static str,
    /// True when Limi actively polls the provider; false when it waits for updates.
    pub live: bool,
}

impl ProviderSnapshot {
    pub fn new(id: &'static str, name: &'static str, source: &'static str, live: bool) -> Self {
        Self {
            id,
            name,
            status: Status::Waiting,
            message: None,
            plan: None,
            windows: Vec::new(),
            captured_at: None,
            source,
            live,
        }
    }

    pub fn with_status(mut self, status: Status, message: impl Into<Option<String>>) -> Self {
        self.status = status;
        self.message = message.into();
        self
    }
}

/// Human label for a limit window length.
pub fn window_label(minutes: Option<u32>) -> (String, String) {
    match minutes {
        Some(300) => ("session".into(), "5-hour session".into()),
        Some(10080) => ("weekly".into(), "Weekly".into()),
        Some(1440) => ("daily".into(), "Daily".into()),
        Some(m) if m % 1440 == 0 => (format!("w{m}"), format!("{}-day", m / 1440)),
        Some(m) if m % 60 == 0 => (format!("w{m}"), format!("{}-hour", m / 60)),
        Some(m) => (format!("w{m}"), format!("{m}-minute")),
        None => ("window".into(), "Usage window".into()),
    }
}
