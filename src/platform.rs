//! The one place that knows what the host operating system can answer.
//!
//! Everything above this module — the snapshot, the summary, the receipt — is
//! OS-agnostic. On Windows [`Host`] dispatches to the registry, the privacy
//! audit log and the activity timeline. Anywhere else it returns empty values
//! **with a stated reason**, so `stoolie doctor` can say "this source does not
//! exist here" instead of an empty list that looks like a clean machine.
//!
//! Portability is therefore a fact you can check: `cargo check
//! --no-default-features` compiles the whole core on any target, and only this
//! file changes behaviour.

use crate::eventlog::{self, ReadOptions, Reading};
use crate::model::{CapabilityState, Session};
use crate::registry;
use crate::timeline::{self, Timeline};

/// What the current machine can tell us, and what it cannot.
pub struct Host {
    supported: bool,
    os: String,
    reasons: Vec<String>,
}

impl Host {
    pub fn detect() -> Host {
        let supported = cfg!(windows);
        let mut reasons = Vec::new();
        if !supported {
            reasons.push(
                "capability access records are a Windows artefact; nothing to read on this host"
                    .to_string(),
            );
        }
        Host {
            supported,
            os: os_string(),
            reasons,
        }
    }

    /// Whether this host has the artefacts the product is built on.
    pub fn supported(&self) -> bool {
        self.supported
    }

    pub fn os(&self) -> &str {
        &self.os
    }

    /// Why a source is empty, when it is empty because of the host.
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }

    pub fn sessions(&self) -> Vec<Session> {
        if self.supported {
            registry::read_sessions()
        } else {
            Vec::new()
        }
    }

    pub fn states(&self) -> Vec<CapabilityState> {
        if self.supported {
            registry::read_states()
        } else {
            Vec::new()
        }
    }

    pub fn capability_count(&self) -> usize {
        if self.supported {
            registry::capability_count()
        } else {
            0
        }
    }

    pub fn read_events(&self, options: &ReadOptions) -> Reading {
        if self.supported {
            eventlog::read(options)
        } else {
            Reading {
                events: Vec::new(),
                warning: Some("privacy auditing is a Windows event channel".to_string()),
            }
        }
    }

    pub fn timeline(&self) -> Timeline {
        if self.supported {
            timeline::load()
        } else {
            Timeline::default()
        }
    }

    /// Sockets owned by this process, as counted by the host's own tooling.
    /// `None` when the host has no such tool, which is reported rather than
    /// guessed.
    pub fn outbound_connection_count(&self) -> Option<usize> {
        crate::platform::count_outbound_connections()
    }
}

/// `Windows 10 Pro 22H2 (build 19045)`, or the platform name elsewhere.
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
        std::env::consts::OS.to_string()
    }
}

#[cfg(windows)]
fn count_outbound_connections() -> Option<usize> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("netstat")
        .arg("-ano")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let pid = std::process::id().to_string();
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .filter(|line| line.trim_end().ends_with(&pid))
            .filter(|line| line.contains("ESTABLISHED") || line.contains("SYN_SENT"))
            .count(),
    )
}

#[cfg(not(windows))]
fn count_outbound_connections() -> Option<usize> {
    // No netstat audit outside Windows: the tool has no sockets to count, but
    // "no tool" and "zero sockets" are different claims.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_reports_whether_it_can_answer() {
        let host = Host::detect();
        assert_eq!(host.supported(), cfg!(windows));
        assert!(!host.os().is_empty());
        if host.supported() {
            assert!(host.reasons().is_empty());
        } else {
            assert!(
                !host.reasons().is_empty(),
                "an unsupported host must say why"
            );
        }
    }

    #[test]
    fn an_unsupported_host_returns_empty_but_explained_readings() {
        let host = Host {
            supported: false,
            os: "test".to_string(),
            reasons: vec!["not windows".to_string()],
        };
        assert!(host.sessions().is_empty());
        assert!(host.states().is_empty());
        assert_eq!(host.capability_count(), 0);
        let reading = host.read_events(&ReadOptions::default());
        assert!(reading.events.is_empty());
        assert!(reading.warning.is_some(), "silence is not an empty log");
        assert!(!host.timeline().available());
    }
}
