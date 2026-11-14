//! Reader for `Microsoft-Windows-Privacy-Auditing/Operational`.
//!
//! This log records *setting changes*, not usage: who flipped a capability from
//! Allow to Deny, when, and which process did it. It is the only part of the
//! story that survives the registry's one-slot history, and it goes back months
//! on a machine that has never been reinstalled.
//!
//! The log is read through `wevtutil`, which ships with Windows and needs no
//! elevation for this channel. Parsing is deliberately forgiving: an unexpected
//! shape drops the record, never the whole run.

use crate::model::ConsentEvent;
use chrono::DateTime;

pub const CHANNEL: &str = "Microsoft-Windows-Privacy-Auditing/Operational";

/// Read up to `limit` events, newest last. Returns an empty vector when the
/// channel is unavailable; callers report that rather than failing.
pub fn read_events(limit: usize) -> Vec<ConsentEvent> {
    match read_raw(limit) {
        Ok(xml) => {
            let mut events = parse(&xml);
            events.sort_by_key(|event| event.ts);
            events
        }
        Err(_) => Vec::new(),
    }
}

#[cfg(windows)]
fn read_raw(limit: usize) -> std::io::Result<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("wevtutil")
        .args(["qe", CHANNEL, &format!("/c:{limit}"), "/f:xml"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(not(windows))]
fn read_raw(_limit: usize) -> std::io::Result<String> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "privacy auditing is a Windows event channel",
    ))
}

/// Whether the channel can be read at all on this machine.
pub fn available() -> bool {
    read_raw(1).is_ok()
}

fn parse(xml: &str) -> Vec<ConsentEvent> {
    let mut events = Vec::new();
    for chunk in xml.split("</Event>") {
        let Some(body) = chunk.rfind("<Event ").map(|index| &chunk[index..]) else {
            continue;
        };
        let Some(raw_time) = attr(body, "SystemTime='") else {
            continue;
        };
        let Some(ts) = parse_utc(&raw_time) else {
            continue;
        };
        let event_id = tag_number(body, "<EventID>").unwrap_or(0);
        let mut fields = Vec::new();
        let mut rest = body;
        while let Some(index) = rest.find("<Data Name='") {
            rest = &rest[index + "<Data Name='".len()..];
            let Some(end_name) = rest.find('\'') else {
                break;
            };
            let name = &rest[..end_name];
            let after = &rest[end_name + 1..];
            if let Some(value) = after.strip_prefix("/>") {
                fields.push((name.to_string(), String::new()));
                rest = value;
                continue;
            }
            let Some(value) = after.strip_prefix('>') else {
                break;
            };
            let Some(end_value) = value.find("</Data>") else {
                break;
            };
            fields.push((name.to_string(), unescape(&value[..end_value])));
            rest = &value[end_value + "</Data>".len()..];
        }
        let get = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        events.push(ConsentEvent {
            ts,
            event_id,
            capability: get("SettingName").unwrap_or_else(|| "unknown".to_string()),
            app: get("AppPackageFamilyName").filter(|value| value != "NULL"),
            old: get("OldConsentValue"),
            new: get("NewConsentValue"),
            caller: get("CallerProcessName").filter(|value| !value.is_empty()),
        });
    }
    events
}

fn attr(body: &str, needle: &str) -> Option<String> {
    let start = body.find(needle)? + needle.len();
    let rest = &body[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn tag_number(body: &str, open: &str) -> Option<u32> {
    let start = body.find(open)? + open.len();
    let rest = &body[start..];
    let end = rest.find('<')?;
    rest[..end].trim().parse().ok()
}

/// Event timestamps look like `2026-11-07T19:31:32.3292630Z`.
pub fn parse_utc(raw: &str) -> Option<i64> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.timestamp_millis());
    }
    // Some builds emit more than nine fractional digits; trim and retry.
    let (whole, rest) = raw.split_once('.')?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    let trimmed: String = digits.chars().take(6).collect();
    let rebuilt = format!("{whole}.{trimmed}Z");
    DateTime::parse_from_rfc3339(&rebuilt)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}

fn unescape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let replacement = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            other => other
                .strip_prefix('#')
                .and_then(|digits| {
                    if let Some(hex) = digits.strip_prefix('x').or(digits.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        digits.parse::<u32>().ok()
                    }
                })
                .and_then(char::from_u32),
        };
        match replacement {
            Some(ch) => out.push(ch),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-Privacy-Auditing'/><EventID>1004</EventID><TimeCreated SystemTime='2026-11-07T19:31:32.3292630Z'/></System><EventData><Data Name='CallerProcessName'>SystemSettings.exe</Data><Data Name='OldConsentValue'>Prompt</Data><Data Name='NewConsentValue'>Allow</Data><Data Name='AppPackageFamilyName'>Microsoft.Windows.Photos_8wekyb3d8bbwe</Data><Data Name='SettingName'>microphone</Data></EventData></Event>"#;

    #[test]
    fn parses_a_consent_change() {
        let events = parse(SAMPLE);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.event_id, 1004);
        assert_eq!(event.capability, "microphone");
        assert_eq!(
            event.app.as_deref(),
            Some("Microsoft.Windows.Photos_8wekyb3d8bbwe")
        );
        assert_eq!(event.old.as_deref(), Some("Prompt"));
        assert_eq!(event.new.as_deref(), Some("Allow"));
        assert_eq!(event.caller.as_deref(), Some("SystemSettings.exe"));
        assert_eq!(event.ts, 1_794_079_892_329);
    }

    #[test]
    fn drops_null_app_names() {
        let xml = SAMPLE.replace("Microsoft.Windows.Photos_8wekyb3d8bbwe", "NULL");
        assert_eq!(parse(&xml)[0].app, None);
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(unescape("a&amp;b&lt;c&gt;d&quot;e&apos;f"), "a&b<c>d\"e'f");
        assert_eq!(unescape("x&#65;y"), "xAy");
    }

    #[test]
    fn handles_odd_fractional_digits() {
        assert_eq!(
            parse_utc("2026-11-07T19:31:32.3292630123Z"),
            parse_utc("2026-11-07T19:31:32.329263Z")
        );
    }
}
