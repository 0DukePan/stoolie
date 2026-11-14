//! Small, dependency-light helpers: FILETIME conversion, human formatting and
//! HTML escaping.
//!
//! Timestamps are carried around as Unix seconds (UTC). Only the presentation
//! layer converts to local time, so the archive stays portable.

use chrono::{DateTime, Local, TimeZone};

/// Seconds between the Windows FILETIME epoch (1601-01-01) and the Unix epoch.
pub const FILETIME_EPOCH_OFFSET_SECS: i64 = 11_644_473_600;

/// Convert a registry `LastUsedTimeStart`/`LastUsedTimeStop` value to Unix
/// **milliseconds**. `0` is meaningful in this API — it means "not stopped yet"
/// — so it maps to `None` rather than to the 1601 epoch.
///
/// Milliseconds matter here: the registry keeps 100 ns precision, and "14.1 s"
/// is a better sentence than "14 s".
pub fn filetime_to_unix_ms(value: u64) -> Option<i64> {
    if value == 0 {
        return None;
    }
    let milliseconds = (value / 10_000) as i64 - FILETIME_EPOCH_OFFSET_SECS * 1_000;
    (milliseconds > 0).then_some(milliseconds)
}

/// Unix milliseconds, UTC, for "now".
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Reverse of [`filetime_to_unix_ms`], used by tests.
pub fn unix_ms_to_filetime(milliseconds: i64) -> u64 {
    ((milliseconds + FILETIME_EPOCH_OFFSET_SECS * 1_000) as u64) * 10_000
}

/// Local wall-clock time for a Unix millisecond timestamp.
pub fn local(ts_ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ts_ms)
        .single()
        .unwrap_or_else(|| Local.timestamp_millis_opt(0).unwrap())
}

/// `Thu 12 Nov`
pub fn day(ts: i64) -> String {
    local(ts).format("%a %d %b").to_string()
}

/// `02:09:36`
pub fn time(ts: i64) -> String {
    local(ts).format("%H:%M:%S").to_string()
}

/// `12 Nov 02:09`
pub fn stamp(ts: i64) -> String {
    local(ts).format("%d %b %H:%M").to_string()
}

/// `Jun 2026`
pub fn month(ts: i64) -> String {
    local(ts).format("%b %Y").to_string()
}

/// `2026-11-14T00:56:13+01:00`
pub fn rfc3339(ts: i64) -> String {
    local(ts).to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// `14.1s`, `2m 03s`, `1h 04m` from milliseconds.
pub fn duration(milliseconds: i64) -> String {
    let seconds = (milliseconds.max(0) as f64) / 1_000.0;
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else if seconds < 3_600.0 {
        let whole = seconds as i64;
        format!("{}m {:02}s", whole / 60, whole % 60)
    } else {
        let whole = seconds as i64;
        format!("{}h {:02}m", whole / 3_600, (whole % 3_600) / 60)
    }
}

/// `13 days`, `1 day`
pub fn plural_days(days: u32) -> String {
    if days == 1 {
        "1 day".to_string()
    } else {
        format!("{days} days")
    }
}

/// Escape text for the receipt. The receipt is generated from local data that
/// can contain hostile strings (a path chosen by anyone who can create a file),
/// so every interpolation goes through this.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// File name of a `NonPackaged` executable path, falling back to the full path.
pub fn app_short_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

/// `Microsoft.Windows.Photos_8wekyb3d8bbwe` -> `Microsoft.Windows.Photos`
pub fn packaged_short_name(package: &str) -> String {
    match package.split_once('_') {
        Some((name, _publisher)) => name.to_string(),
        None => package.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_round_trips_with_sub_second_precision() {
        let ts = 1_763_000_000_141;
        let ft = unix_ms_to_filetime(ts);
        assert_eq!(filetime_to_unix_ms(ft), Some(ts));
    }

    #[test]
    fn filetime_zero_means_not_stopped() {
        assert_eq!(filetime_to_unix_ms(0), None);
    }

    #[test]
    fn duration_formats() {
        assert_eq!(duration(14_100), "14.1s");
        assert_eq!(duration(123_000), "2m 03s");
        assert_eq!(duration(3_845_000), "1h 04m");
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(
            escape_html("<img src=x onerror='a&b'>"),
            "&lt;img src=x onerror=&#39;a&amp;b&#39;&gt;"
        );
    }

    #[test]
    fn app_names_are_shortened() {
        assert_eq!(
            app_short_name(r"C:\Apps\Discord\Discord.exe"),
            "Discord.exe"
        );
        assert_eq!(
            packaged_short_name("Microsoft.Windows.Photos_8wekyb3d8bbwe"),
            "Microsoft.Windows.Photos"
        );
    }
}
