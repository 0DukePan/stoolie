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
//!
//! Imports are incremental. `ReadOptions::since` is pushed into the XPath query
//! so a machine with a large log does not pay to read events the archive already
//! holds: the first run reads the history, later runs read the delta.

use crate::model::ConsentEvent;
use chrono::DateTime;

pub const CHANNEL: &str = "Microsoft-Windows-Privacy-Auditing/Operational";

/// What to read, and how much to trust the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOptions {
    /// Unix ms. Events at or after this instant are requested.
    pub since_ms: Option<i64>,
    /// Hard ceiling on events requested, newest first.
    pub limit: usize,
    /// When the cursor is older than the oldest retained record, some events
    /// may be missing; the caller prints that instead of pretending otherwise.
    pub cursor_trusted: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        ReadOptions {
            since_ms: None,
            limit: DEFAULT_LIMIT,
            cursor_trusted: true,
        }
    }
}

/// Events requested when there is no cursor yet. The channel held 206 records on
/// the development machine after five months, so this covers a full history
/// without asking for more than we render.
pub const DEFAULT_LIMIT: usize = 5_000;

#[derive(Debug, Clone, Default)]
pub struct Reading {
    pub events: Vec<ConsentEvent>,
    /// Why the list may be short: channel unavailable, query rejected, or a
    /// cursor older than the retained log.
    pub warning: Option<String>,
}

/// Read events, newest last.
pub fn read(options: &ReadOptions) -> Reading {
    match read_raw(options) {
        Ok(xml) => {
            let events = dedup(parse(&xml));
            let mut warning = None;
            if !options.cursor_trusted {
                warning = Some(
                    "the archive cursor is older than the retained log; some setting changes may be missing"
                        .to_string(),
                );
            }
            if events.len() >= options.limit {
                warning = Some(format!(
                    "read the newest {} events; older ones are in the log but were not requested",
                    options.limit
                ));
            }
            Reading { events, warning }
        }
        Err(error) => Reading {
            events: Vec::new(),
            warning: Some(error.to_string()),
        },
    }
}

/// Convenience for callers that want a plain first import.
pub fn read_events(limit: usize) -> Vec<ConsentEvent> {
    read(&ReadOptions {
        limit,
        ..ReadOptions::default()
    })
    .events
}

/// The XPath filter. `timediff(@SystemTime)` is the documented way to ask for
/// "events newer than N", and it is applied by the event subsystem rather than
/// by us, so the cost scales with the answer, not with the log.
pub fn query_for(options: &ReadOptions) -> Option<String> {
    let since = options.since_ms?;
    let now = crate::util::now_ms();
    let age = (now - since).max(0);
    Some(format!(
        "*[System[TimeCreated[timediff(@SystemTime) <= {age}]]]"
    ))
}

#[cfg(windows)]
fn read_raw(options: &ReadOptions) -> std::io::Result<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut args = vec![
        "qe".to_string(),
        CHANNEL.to_string(),
        format!("/c:{}", options.limit),
        "/f:xml".to_string(),
    ];
    if let Some(query) = query_for(options) {
        args.push(format!("/q:{query}"));
    }
    let output = Command::new("wevtutil")
        .args(&args)
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
fn read_raw(_options: &ReadOptions) -> std::io::Result<String> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "privacy auditing is a Windows event channel",
    ))
}

/// Whether the channel can be read at all on this machine.
pub fn available() -> bool {
    read_raw(&ReadOptions {
        limit: 1,
        ..ReadOptions::default()
    })
    .is_ok()
}

/// Sort by time and drop records the log repeated (a re-imported channel, or a
/// query that straddled a rotation).
pub fn dedup(mut events: Vec<ConsentEvent>) -> Vec<ConsentEvent> {
    events.sort_by_key(|event| event.ts);
    events.dedup_by(|a, b| a.ts == b.ts && a.capability == b.capability && a.app == b.app);
    events
}

/// Parse a captured `wevtutil /f:xml` document.
///
/// Public so the fixture in `tests/fixtures/` can be parsed by an integration
/// test exactly as the binary parses it.
pub fn parse(xml: &str) -> Vec<ConsentEvent> {
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
        let fields = data_fields(body);
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

/// Every `<Data Name='…'>value</Data>` in the record, in document order.
fn data_fields(body: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut rest = body;
    while let Some(index) = rest.find("<Data Name='") {
        rest = &rest[index + "<Data Name='".len()..];
        let Some(end_name) = rest.find('\'') else {
            break;
        };
        let name = &rest[..end_name];
        let after = &rest[end_name + 1..];
        if let Some(tail) = after.strip_prefix("/>") {
            fields.push((name.to_string(), String::new()));
            rest = tail;
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
    fields
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

    #[test]
    fn no_cursor_means_no_query_filter() {
        assert_eq!(query_for(&ReadOptions::default()), None);
    }

    #[test]
    fn a_cursor_becomes_a_timediff_filter() {
        let options = ReadOptions {
            since_ms: Some(crate::util::now_ms() - 3_600_000),
            ..ReadOptions::default()
        };
        let query = query_for(&options).expect("a cursor must produce a filter");
        assert!(query.starts_with("*[System[TimeCreated[timediff(@SystemTime) <= "));
        let age: String = query
            .chars()
            .skip_while(|ch| !ch.is_ascii_digit())
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        let age: i64 = age.parse().unwrap();
        assert!(
            (3_500_000..=3_700_000).contains(&age),
            "filter age {age}ms is not about an hour"
        );
    }

    #[test]
    fn a_clamped_cursor_is_rounded_up_not_negative() {
        let options = ReadOptions {
            since_ms: Some(crate::util::now_ms() + 60_000),
            ..ReadOptions::default()
        };
        let query = query_for(&options).unwrap();
        assert!(query.contains("<= 0]]]"), "future cursors clamp to zero");
    }

    /// Every fragment in the corpus must parse without panicking, and either
    /// yield a well-formed event or be dropped. Truncated, hostile and
    /// ill-ordered input is the norm in an event log.
    #[test]
    fn survives_a_corpus_of_broken_fragments() {
        let corpus = [
            "",
            "<Event ",
            "<Event><System>",
            "<Event><System><TimeCreated SystemTime='not-a-date'/></System></Event>",
            "<Event><System><TimeCreated SystemTime='2026-11-07T19:31:32Z'/></System><EventData><Data Name='SettingName'>microphone</Data></EventData></Event>",
            "<Event><System><TimeCreated SystemTime='2026-11-07T19:31:32Z'/><EventID>abc</EventID></System></Event>",
            "<Event><Data Name='SettingName'/></Event>",
            "<Event><Data Name='SettingName'>unterminated</Event>",
            "<Event><Data Name='&bogus;'>x</Data></Event>",
            "&;&#xEXTRA;<Event></Event>",
            SAMPLE,
            SAMPLE,
        ];
        for fragment in corpus {
            let events = parse(fragment);
            for event in &events {
                assert!(!event.capability.is_empty());
                assert!(event.ts > 0);
            }
        }
        // A duplicate record parses twice; deduplication is the reader's job.
        assert_eq!(parse(SAMPLE).len(), 1);
    }

    #[test]
    fn duplicate_records_are_deduplicated_on_read() {
        let events = dedup(parse(&format!("{SAMPLE}{SAMPLE}")));
        assert_eq!(events.len(), 1);
    }
}
