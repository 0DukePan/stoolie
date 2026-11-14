//! Domain types shared by the readers, the archive and the renderers.

use serde::Serialize;

/// Where a session came from. The receipt prints this so a row from our own
/// archive is never confused with a row the operating system still had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Read live from `CapabilityAccessManager\ConsentStore` in this run.
    Registry,
    /// Read back from stoolie's own append-only archive.
    Archive,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Registry => "registry",
            Source::Archive => "archive",
        }
    }
}

/// One period during which an application held a capability.
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub capability: String,
    /// Display name: the executable file name, or the package name without its
    /// publisher hash.
    pub app: String,
    /// Full path for classic Win32 programs; `None` for packaged apps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_path: Option<String>,
    /// Unix milliseconds, UTC.
    pub start: i64,
    /// Unix milliseconds, UTC. `None` means the capability is held right now.
    pub stop: Option<i64>,
    pub source: Source,
    /// Other applications that had activity records at that second. This is
    /// corroboration, not attribution: timeline records overlap.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
}

impl Session {
    /// How long the capability was held, in milliseconds.
    pub fn duration(&self) -> Option<i64> {
        self.stop.map(|stop| (stop - self.start).max(0))
    }

    pub fn live(&self) -> bool {
        self.stop.is_none()
    }

    /// Stable identity used to deduplicate the registry against the archive.
    pub fn identity(&self) -> (String, String, i64) {
        (self.capability.clone(), self.app.clone(), self.start)
    }
}

/// A change to a privacy setting, from the Windows privacy-auditing log.
#[derive(Debug, Clone, Serialize)]
pub struct ConsentEvent {
    /// Unix milliseconds, UTC.
    pub ts: i64,
    pub event_id: u32,
    pub capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
}

/// The per-capability global switch (`Allow`, `Deny`, `Prompt`).
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityState {
    pub name: String,
    pub value: String,
}

/// One record from the Windows activity timeline.
#[derive(Debug, Clone, Serialize)]
pub struct Activity {
    pub start: i64,
    pub end: i64,
    pub app: String,
}
