//! Reader for `CapabilityAccessManager\ConsentStore`.
//!
//! Windows keeps one timestamp pair per application and capability: the start
//! and stop of the *most recent* use. That is the whole history the OS offers,
//! and it is overwritten on the next use — which is why stoolie keeps its own
//! archive.
//!
//! Classic Win32 programs live under `…\<capability>\NonPackaged\`, with the
//! executable path as the key name and `#` where `\` would be. Packaged apps
//! live directly under the capability key, named by package family name.

use crate::model::{CapabilityState, Session, Source};
use crate::util::{app_short_name, filetime_to_unix_ms, packaged_short_name};

const CONSENT_STORE: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

#[cfg(windows)]
mod platform {
    use super::*;
    use std::collections::BTreeMap;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    /// Every capability key under the current user (and, when readable, the
    /// machine hive). Sorted for stable output.
    fn capability_keys() -> Vec<(String, RegKey)> {
        let mut found: BTreeMap<String, RegKey> = BTreeMap::new();
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(CONSENT_STORE, KEY_READ)
            else {
                continue;
            };
            for name in root.enum_keys().flatten() {
                if found.contains_key(&name) {
                    continue;
                }
                if let Ok(key) = root.open_subkey_with_flags(&name, KEY_READ) {
                    found.insert(name, key);
                }
            }
        }
        found.into_iter().collect()
    }

    fn read_times(key: &RegKey) -> (Option<i64>, Option<i64>) {
        let start = key
            .get_value::<u64, _>("LastUsedTimeStart")
            .ok()
            .and_then(filetime_to_unix_ms);
        let stop = key
            .get_value::<u64, _>("LastUsedTimeStop")
            .ok()
            .and_then(filetime_to_unix_ms);
        (start, stop)
    }

    fn push_session(
        out: &mut BTreeMap<(String, String, i64), Session>,
        capability: &str,
        app: String,
        app_path: Option<String>,
        start: i64,
        stop: Option<i64>,
    ) {
        let session = Session {
            capability: capability.to_string(),
            app,
            app_path,
            start,
            stop,
            source: Source::Registry,
            context: Vec::new(),
        };
        out.entry(session.identity()).or_insert(session);
    }

    pub fn read_sessions() -> Vec<Session> {
        let mut out: BTreeMap<(String, String, i64), Session> = BTreeMap::new();
        for (capability, cap_key) in capability_keys() {
            for child in cap_key.enum_keys().flatten() {
                let Ok(key) = cap_key.open_subkey_with_flags(&child, KEY_READ) else {
                    continue;
                };
                if child == "NonPackaged" {
                    for entry in key.enum_keys().flatten() {
                        let Ok(app_key) = key.open_subkey_with_flags(&entry, KEY_READ) else {
                            continue;
                        };
                        let (Some(start), stop) = read_times(&app_key) else {
                            continue;
                        };
                        let path = entry.replace('#', r"\");
                        push_session(
                            &mut out,
                            &capability,
                            app_short_name(&path),
                            Some(path),
                            start,
                            stop,
                        );
                    }
                } else {
                    let (Some(start), stop) = read_times(&key) else {
                        continue;
                    };
                    push_session(
                        &mut out,
                        &capability,
                        packaged_short_name(&child),
                        None,
                        start,
                        stop,
                    );
                }
            }
        }
        let mut sessions: Vec<Session> = out.into_values().collect();
        sessions.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| a.app.cmp(&b.app)));
        sessions
    }

    pub fn read_states() -> Vec<CapabilityState> {
        let mut states = Vec::new();
        for (capability, key) in capability_keys() {
            if let Ok(value) = key.get_value::<String, _>("Value") {
                states.push(CapabilityState {
                    name: capability,
                    value,
                });
            }
        }
        states
    }

    pub fn capability_count() -> usize {
        capability_keys().len()
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn read_sessions() -> Vec<Session> {
        Vec::new()
    }

    pub fn read_states() -> Vec<CapabilityState> {
        Vec::new()
    }

    pub fn capability_count() -> usize {
        0
    }
}

pub use platform::{capability_count, read_sessions, read_states};

/// Decode a `NonPackaged` key name (`C:#Program Files#Zoom#Zoom.exe`).
pub fn decode_app_path(key_name: &str) -> String {
    key_name.replace('#', r"\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_packaged_keys_decode_to_windows_paths() {
        assert_eq!(
            decode_app_path(r"C:#Users#a#AppData#Local#Discord#app-1.0.9267#Discord.exe"),
            r"C:\Users\a\AppData\Local\Discord\app-1.0.9267\Discord.exe"
        );
    }

    #[test]
    fn reading_never_panics_on_this_machine() {
        // A smoke test rather than a fixture test: the reader must survive
        // whatever shape this machine's registry is in.
        let sessions = read_sessions();
        let states = read_states();
        assert!(sessions.len() < 100_000);
        assert!(states.len() < 1_000);
        for session in sessions {
            assert!(!session.capability.is_empty());
            assert!(!session.app.is_empty());
            if let Some(stop) = session.stop {
                assert!(stop >= session.start);
            }
        }
    }
}
