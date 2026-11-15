//! Builds the snapshot every renderer consumes, and prints the terminal summary.

use crate::model::{CapabilityState, ConsentEvent, Session};
use crate::{eventlog, registry, store, util, SENSOR_CAPABILITIES};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TimelineSummary {
    pub available: bool,
    pub activities: usize,
    pub apps: usize,
    pub first: Option<i64>,
    pub last: Option<i64>,
    pub per_day: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub version: String,
    pub generated: i64,
    pub days: u32,
    pub os: String,
    pub sessions: Vec<Session>,
    pub states: Vec<CapabilityState>,
    pub events: Vec<ConsentEvent>,
    pub timeline: TimelineSummary,
    pub archive_sessions: i64,
    pub archive_events: i64,
    pub archive_path: String,
    pub capabilities_seen: usize,
    pub event_log_available: bool,
    /// Things the run could not read. Printed, never swallowed.
    pub warnings: Vec<String>,
}

impl Snapshot {
    pub fn build(days: u32) -> Result<Snapshot> {
        let now = util::now_ms();
        let since = now - (days as i64) * 86_400_000;

        let live = registry::read_sessions();
        let events_all = eventlog::read_events(400);

        let mut store = store::Store::open_default().ok();
        let archive_error = match store.as_mut() {
            Some(store) => store.sync(&live, &events_all).err().map(|error| {
                // A failed import must not fail the run: the live registry is
                // still the truth for this moment. Say so, then carry on.
                format!("archive import failed: {error}")
            }),
            None => Some("archive unavailable: could not open the database".to_string()),
        };

        let timeline = crate::timeline::load();
        let mut sessions: Vec<Session> = live;
        if let Some(store) = store.as_ref() {
            if let Ok(archived) = store.sessions_since(since) {
                for session in archived {
                    if !sessions
                        .iter()
                        .any(|known| known.identity() == session.identity())
                    {
                        sessions.push(session);
                    }
                }
            }
        }

        sessions.retain(|session| session.start >= since);
        sessions.retain(|session| is_sensor(&session.capability));
        for session in sessions.iter_mut() {
            session.context = timeline.context_at(session.start);
        }
        sessions.sort_by_key(|session| std::cmp::Reverse(session.start));

        let events: Vec<ConsentEvent> = events_all
            .iter()
            .filter(|event| is_sensor(&event.capability))
            .cloned()
            .collect();

        let states = registry::read_states();
        let archive_sessions = store
            .as_ref()
            .map(|s| s.session_count().unwrap_or(0))
            .unwrap_or(0);
        let event_log_available = !events_all.is_empty();
        let mut warnings: Vec<String> = Vec::new();
        if let Some(message) = archive_error {
            warnings.push(message);
        }
        if !event_log_available {
            warnings.push("privacy-auditing log returned no events".to_string());
        }
        if !timeline.available() {
            warnings.push("activity timeline unavailable on this machine".to_string());
        }
        let archive_events = store
            .as_ref()
            .map(|s| s.event_count().unwrap_or(0))
            .unwrap_or(0);

        Ok(Snapshot {
            version: crate::VERSION.to_string(),
            generated: now,
            days,
            os: os_string(),
            sessions,
            states,
            events,
            timeline: TimelineSummary {
                available: timeline.available(),
                activities: timeline.activities.len(),
                apps: timeline.apps(),
                first: timeline.first(),
                last: timeline.last(),
                per_day: timeline.per_day(),
            },
            archive_sessions,
            archive_events,
            archive_path: store
                .as_ref()
                .map(|s| s.path().display().to_string())
                .unwrap_or_default(),
            capabilities_seen: registry::capability_count(),
            event_log_available,
            warnings,
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn to_json_compact(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Total microphone time in the window, in milliseconds.
    pub fn microphone_seconds(&self) -> i64 {
        self.sessions
            .iter()
            .filter(|session| session.capability == "microphone")
            .filter_map(|session| session.duration())
            .sum()
    }

    pub fn live(&self, capability: &str) -> bool {
        self.sessions
            .iter()
            .any(|session| session.capability == capability && session.live())
    }

    pub fn state(&self, capability: &str) -> Option<&str> {
        self.states
            .iter()
            .find(|state| state.name == capability)
            .map(|state| state.value.as_str())
    }

    pub fn sessions_for(&self, capability: &str) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|session| session.capability == capability)
            .collect()
    }

    /// Human summary printed by `stoolie` with no subcommand.
    pub fn summary(&self, colour: bool) -> String {
        let paint = Palette::new(colour);
        let mut out = String::new();
        out.push_str(&format!(
            "\n{}\n\n",
            paint.bold(&format!(
                "  SENSOR RECEIPT · {} · {} RECORDED MOMENTS",
                util::plural_days(self.days).to_uppercase(),
                thousands(self.timeline.activities)
            ))
        ));

        out.push_str(&format!("  {}\n", paint.cyan("MICROPHONE")));
        let microphone = self.sessions_for("microphone");
        if microphone.is_empty() {
            out.push_str(&format!(
                "    {}\n",
                paint.dim(&format!(
                    "nothing recorded in the {}",
                    util::plural_days(self.days)
                ))
            ));
        } else {
            for session in microphone.iter().take(6) {
                render_session(&paint, &mut out, session);
            }
        }
        out.push('\n');

        let camera = self.sessions_for("webcam");
        if camera.is_empty() {
            out.push_str(&format!(
                "  CAMERA        {} · unused for {}\n\n",
                paint.dim(&format!("access is {}", allowed_word(self.state("webcam")))),
                util::plural_days(self.days)
            ));
        } else {
            out.push_str(&format!("  {}\n", paint.cyan("CAMERA")));
            for session in camera.iter().take(4) {
                render_session(&paint, &mut out, session);
            }
            out.push('\n');
        }

        let location = self.sessions_for("location");
        if !location.is_empty() {
            out.push_str(&format!("  {}\n", paint.cyan("LOCATION")));
            for session in location.iter().take(4) {
                render_session(&paint, &mut out, session);
            }
            out.push('\n');
        }

        out.push_str(&format!(
            "  {}\n",
            paint.cyan(&format!(
                "PRIVACY LEDGER  {} changes since {}",
                self.events.len(),
                self.events
                    .first()
                    .map(|event| util::month(event.ts))
                    .unwrap_or_else(|| "the log".to_string())
            ))
        ));
        if self.events.is_empty() {
            out.push_str(&format!(
                "    {}\n",
                paint.dim(if self.event_log_available {
                    "no sensor-setting changes in the log"
                } else {
                    "privacy-auditing log unavailable on this machine"
                })
            ));
        }
        for event in self.events.iter().rev().take(4) {
            out.push_str(&format!(
                "    {}  {:<11} {} → {:<10} {} {}\n",
                util::stamp(event.ts),
                event.capability,
                event.old.clone().unwrap_or_else(|| "unset".into()),
                event.new.clone().unwrap_or_else(|| "unset".into()),
                event
                    .app
                    .as_deref()
                    .map(util::packaged_short_name)
                    .unwrap_or_else(|| "all apps".into()),
                paint.dim(&format!(
                    "· {}",
                    event.caller.clone().unwrap_or_else(|| "system".to_string())
                ))
            ));
        }

        out.push_str(&format!(
            "\n  RIGHT NOW     microphone: {}   camera: {}\n",
            status_word(&paint, self.live("microphone")),
            status_word(&paint, self.live("webcam"))
        ));
        out.push_str(&format!(
            "  ARCHIVE       {} sessions · {} events · {}\n",
            thousands(self.archive_sessions.max(0) as usize),
            thousands(self.archive_events.max(0) as usize),
            paint.dim(&self.archive_path)
        ));
        for warning in &self.warnings {
            out.push_str(&format!("  {} {}\n", paint.red("!"), paint.dim(warning)));
        }
        out.push_str(&format!(
            "\n  → {}   {}\n\n",
            paint.bold("stoolie receipt"),
            paint.dim("one HTML file · every app · 0 bytes sent")
        ));
        out
    }
}

/// One session line: app, day, window, duration, and the archive tag only when
/// the row is not something the OS still remembers.
fn render_session(paint: &Palette, out: &mut String, session: &Session) {
    let window = match session.stop {
        Some(stop) => format!(
            "{} → {}   ({})",
            util::time(session.start),
            util::time(stop),
            util::duration(session.duration().unwrap_or(0))
        ),
        None => format!(
            "{} → {}",
            util::time(session.start),
            paint.red("in use now")
        ),
    };
    let tag = match session.source {
        crate::model::Source::Archive => format!("  {}", paint.dim("(archive)")),
        crate::model::Source::Registry => String::new(),
    };
    out.push_str(&format!(
        "    {:<22} {}  {:<14}{}\n",
        paint.bold(&session.app),
        util::day(session.start),
        window,
        tag
    ));
    if session.context.len() > 1 {
        out.push_str(&format!(
            "      {}\n",
            paint.dim(&format!(
                "context: {} apps had records that second",
                session.context.len()
            ))
        ));
    }
}

fn allowed_word(state: Option<&str>) -> &'static str {
    match state.map(|value| value.to_ascii_lowercase()).as_deref() {
        Some("allow") => "allowed for every app",
        Some("deny") => "blocked",
        Some("prompt") => "ask every time",
        _ => "unknown",
    }
}

fn status_word(paint: &Palette, live: bool) -> String {
    if live {
        paint.red("in use")
    } else {
        paint.green("idle")
    }
}

fn is_sensor(capability: &str) -> bool {
    SENSOR_CAPABILITIES.contains(&capability)
}

pub fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// OS string for headers: `Windows 10 Pro 22H2 (build 19045)`.
pub fn os_string() -> String {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        {
            let name: String = key.get_value("ProductName").unwrap_or_default();
            let version: String = key.get_value("DisplayVersion").unwrap_or_default();
            let build: String = key.get_value("CurrentBuild").unwrap_or_default();
            if !name.is_empty() {
                return format!("{name} {version} (build {build})");
            }
        }
        "Windows".to_string()
    }
    #[cfg(not(windows))]
    {
        "non-Windows host".to_string()
    }
}

/// Minimal ANSI palette that disappears when colour is off.
pub struct Palette {
    on: bool,
}

impl Palette {
    pub fn new(on: bool) -> Palette {
        Palette {
            on: on && std::env::var_os("NO_COLOR").is_none(),
        }
    }

    fn wrap(&self, code: &str, text: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, text: &str) -> String {
        self.wrap("1", text)
    }

    pub fn dim(&self, text: &str) -> String {
        self.wrap("2", text)
    }

    pub fn cyan(&self, text: &str) -> String {
        self.wrap("36", text)
    }

    pub fn red(&self, text: &str) -> String {
        self.wrap("31", text)
    }

    pub fn green(&self, text: &str) -> String {
        self.wrap("32", text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separates_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_246), "1,246");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn palette_is_plain_without_colour() {
        let paint = Palette::new(false);
        assert_eq!(paint.red("x"), "x");
    }

    #[test]
    fn empty_snapshot_renders_honest_copy() {
        let snapshot = Snapshot {
            version: "0.1.0".into(),
            generated: 1_763_000_000_000,
            days: 13,
            os: "Windows 10 Pro".into(),
            sessions: Vec::new(),
            states: Vec::new(),
            events: Vec::new(),
            timeline: TimelineSummary {
                available: false,
                activities: 0,
                apps: 0,
                first: None,
                last: None,
                per_day: Vec::new(),
            },
            archive_sessions: 0,
            archive_events: 0,
            archive_path: String::new(),
            capabilities_seen: 0,
            event_log_available: false,
            warnings: Vec::new(),
        };
        let text = snapshot.summary(false);
        assert!(text.contains("nothing recorded"));
        assert!(text.contains("stoolie receipt"));
        assert!(!text.contains("\x1b["));
    }
}
