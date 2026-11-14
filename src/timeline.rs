//! Reader for the Windows activity timeline (`ActivitiesCache.db`).
//!
//! This is used for one thing only: answering "what else had activity records
//! at that second?" when a sensor session happened. Timeline records overlap by
//! design, so this is corroboration, never attribution — the summary says "3
//! apps had records that second", not "you were in X".
//!
//! The database is copied before it is opened: Windows keeps it open, and a
//! read-only open of a live SQLite file can fail on a locked page.

use crate::model::Activity;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Timeline {
    pub activities: Vec<Activity>,
    pub database: Option<PathBuf>,
}

impl Timeline {
    pub fn available(&self) -> bool {
        self.database.is_some()
    }

    /// Applications with a timeline record overlapping `ts_ms` (one second of
    /// slack on either side). Deduplicated, sorted.
    pub fn context_at(&self, ts_ms: i64) -> Vec<String> {
        let mut apps: Vec<String> = self
            .activities
            .iter()
            .filter(|activity| {
                activity.start * 1_000 - 1_000 <= ts_ms && ts_ms <= activity.end * 1_000 + 1_000
            })
            .map(|activity| activity.app.clone())
            .collect();
        apps.sort();
        apps.dedup();
        apps
    }

    pub fn first(&self) -> Option<i64> {
        self.activities.iter().map(|activity| activity.start).min()
    }

    pub fn last(&self) -> Option<i64> {
        self.activities.iter().map(|activity| activity.start).max()
    }

    pub fn apps(&self) -> usize {
        let mut names: Vec<&str> = self
            .activities
            .iter()
            .map(|activity| activity.app.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        names.len()
    }

    /// Records per local day, oldest first — the receipt's activity strip.
    pub fn per_day(&self) -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for activity in &self.activities {
            let day = crate::util::local(activity.start * 1_000)
                .format("%Y-%m-%d")
                .to_string();
            match counts.last_mut() {
                Some((last_day, count)) if *last_day == day => *count += 1,
                _ => counts.push((day, 1)),
            }
        }
        counts
    }
}

/// Load the timeline for the current user. Missing or unreadable databases are
/// not an error: the product must still work without this source.
pub fn load() -> Timeline {
    let Some(database) = locate() else {
        return Timeline::default();
    };
    let Some(copy) = copy_to_temp(&database) else {
        return Timeline::default();
    };
    let activities = read_activities(&copy).unwrap_or_default();
    let _ = std::fs::remove_file(&copy);
    Timeline {
        activities,
        database: Some(database),
    }
}

/// `%LOCALAPPDATA%\ConnectedDevicesPlatform\<id>\ActivitiesCache.db`, when it exists.
pub fn locate() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let base = Path::new(&base).join("ConnectedDevicesPlatform");
    let entries = std::fs::read_dir(base).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join("ActivitiesCache.db");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn copy_to_temp(source: &Path) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let target = std::env::temp_dir().join(format!(
        "stoolie-activities-{}-{stamp}.db",
        std::process::id()
    ));
    std::fs::copy(source, &target).ok()?;
    Some(target)
}

fn read_activities(path: &Path) -> rusqlite::Result<Vec<Activity>> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement =
        connection.prepare("SELECT StartTime, EndTime, AppId FROM Activity WHERE StartTime > 0")?;
    let rows = statement.query_map([], |row| {
        let start: i64 = row.get(0)?;
        let end: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(start);
        let app_id: Option<String> = row.get(2)?;
        Ok((start, end, app_id))
    })?;
    let mut activities = Vec::new();
    for row in rows.flatten() {
        let (start, end, app_id) = row;
        let Some(app) = app_id.as_deref().and_then(app_from_payload) else {
            continue;
        };
        if start <= 1_000_000_000 {
            continue;
        }
        activities.push(Activity {
            start,
            end: end.max(start),
            app,
        });
    }
    activities.sort_by_key(|activity| activity.start);
    Ok(activities)
}

/// `AppId` is a JSON array; the first element carries `"application":"…"`.
/// A hand-rolled scan keeps serde out of a hot path and tolerates the odd
/// truncated payload Windows sometimes leaves behind.
pub fn app_from_payload(payload: &str) -> Option<String> {
    let marker = "\"application\":\"";
    let start = payload.find(marker)? + marker.len();
    let rest = &payload[start..];
    let end = rest.find('"')?;
    let mut app = rest[..end].to_string();
    if let Some((_, tail)) = app.split_once('!') {
        app = tail.to_string();
    }
    // Packaged identifiers keep the publisher hash; trim it for display.
    if let Some((name, hash)) = app.split_once('_') {
        if hash.len() >= 8 && hash.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            app = name.to_string();
        }
    }
    Some(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_application_name() {
        let payload = r#"[{"application":"com.squirrel.Discord.Discord","platform":"windows_win32"},{"application":"Discord"}]"#;
        assert_eq!(
            app_from_payload(payload).as_deref(),
            Some("com.squirrel.Discord.Discord")
        );
    }

    #[test]
    fn trims_packaged_publisher_hashes() {
        let payload = r#"[{"application":"Microsoft.ScreenSketch_8wekyb3d8bbwe!App"}]"#;
        assert_eq!(app_from_payload(payload).as_deref(), Some("App"));
    }

    #[test]
    fn context_is_deduplicated_and_bounded() {
        let timeline = Timeline {
            activities: vec![
                Activity {
                    start: 100,
                    end: 200,
                    app: "Brave".into(),
                },
                Activity {
                    start: 150,
                    end: 250,
                    app: "Brave".into(),
                },
                Activity {
                    start: 151,
                    end: 260,
                    app: "Discord".into(),
                },
                Activity {
                    start: 900,
                    end: 950,
                    app: "Later".into(),
                },
            ],
            database: None,
        };
        assert_eq!(
            timeline.context_at(180_000),
            vec!["Brave".to_string(), "Discord".to_string()]
        );
        assert!(timeline.context_at(500_000).is_empty());
    }

    #[test]
    fn per_day_is_chronological() {
        let base = 1_763_000_000;
        let timeline = Timeline {
            activities: vec![
                Activity {
                    start: base,
                    end: base,
                    app: "A".into(),
                },
                Activity {
                    start: base + 60,
                    end: base + 60,
                    app: "B".into(),
                },
                Activity {
                    start: base + 86_400,
                    end: base + 86_400,
                    app: "C".into(),
                },
            ],
            database: None,
        };
        let days = timeline.per_day();
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].1, 2);
    }
}
