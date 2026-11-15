//! The archive: stoolie's own append-only record of capability use.
//!
//! Windows keeps one timestamp pair per app and overwrites it. The archive is
//! the answer to that: every run imports whatever the OS still remembers, and
//! nothing already stored is ever rewritten or pruned.
//!
//! Storage sits behind the [`Storage`] trait for two reasons: tests can use an
//! in-memory implementation instead of a temp file, and the crate can be
//! type-checked on a machine with no C toolchain (`--no-default-features`),
//! which is what keeps the portable core actually portable.

use crate::model::{ConsentEvent, Session, Source};
use std::sync::Mutex;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncStats {
    pub sessions_new: usize,
    pub events_new: usize,
}

/// What a caller needs from an archive. Deliberately small: every method here
/// is something the report actually asks for, so the trait cannot drift into a
/// general-purpose database wrapper.
pub trait Storage {
    /// Import what the OS remembers. Returns how many rows were new.
    fn sync(&mut self, sessions: &[Session], events: &[ConsentEvent]) -> anyhow::Result<SyncStats>;
    /// Sessions that started at or after `since_ms`, newest first.
    fn sessions_since(&self, since_ms: i64) -> anyhow::Result<Vec<Session>>;
    fn session_count(&self) -> anyhow::Result<i64>;
    fn event_count(&self) -> anyhow::Result<i64>;
    /// Highest consent-event timestamp already stored, for incremental imports.
    fn last_event_ts(&self) -> anyhow::Result<Option<i64>>;
    /// Oldest consent event on record, for "changes since".
    fn first_event_ts(&self) -> anyhow::Result<Option<i64>>;
    /// The most recent sensor-setting changes, newest first.
    fn recent_sensor_events(&self, limit: usize) -> anyhow::Result<Vec<ConsentEvent>>;
    /// How many sensor-setting changes the archive holds.
    fn sensor_event_count(&self) -> anyhow::Result<i64>;
    fn location(&self) -> String;
}

/// The three capabilities the product reports on, as a SQL predicate.
#[cfg(feature = "sqlite")]
pub(crate) const SENSOR_SQL: &str = "('microphone','webcam','location')";

/// The archive when the SQLite feature is on (the default, and the only build
/// that ships).
#[cfg(feature = "sqlite")]
pub use sqlite_impl::SqliteStore;

/// In-memory archive. Used by tests, and by `--no-default-features` builds that
/// exist only to prove the core compiles without a C toolchain.
#[derive(Debug, Default)]
pub struct MemoryStore {
    sessions: Mutex<Vec<Session>>,
    events: Mutex<Vec<ConsentEvent>>,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }
}

fn identity_of(session: &Session) -> (String, String, i64) {
    session.identity()
}

impl Storage for MemoryStore {
    fn sync(&mut self, sessions: &[Session], events: &[ConsentEvent]) -> anyhow::Result<SyncStats> {
        let mut stats = SyncStats::default();
        let mut stored = self.sessions.lock().expect("session store poisoned");
        for session in sessions {
            let key = identity_of(session);
            if stored.iter().any(|known| known.identity() == key) {
                continue;
            }
            let mut copy = session.clone();
            copy.source = Source::Archive;
            stored.push(copy);
            stats.sessions_new += 1;
        }
        let mut stored_events = self.events.lock().expect("event store poisoned");
        for event in events {
            let duplicate = stored_events.iter().any(|known| {
                known.ts == event.ts
                    && known.capability == event.capability
                    && known.app == event.app
                    && known.new == event.new
            });
            if duplicate {
                continue;
            }
            let mut copy = event.clone();
            copy.ts = event.ts;
            stored_events.push(copy);
            stats.events_new += 1;
        }
        Ok(stats)
    }

    fn sessions_since(&self, since_ms: i64) -> anyhow::Result<Vec<Session>> {
        let stored = self.sessions.lock().expect("session store poisoned");
        let mut sessions: Vec<Session> = stored
            .iter()
            .filter(|session| session.start >= since_ms)
            .cloned()
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.start));
        Ok(sessions)
    }

    fn session_count(&self) -> anyhow::Result<i64> {
        Ok(self.sessions.lock().expect("session store poisoned").len() as i64)
    }

    fn event_count(&self) -> anyhow::Result<i64> {
        Ok(self.events.lock().expect("event store poisoned").len() as i64)
    }

    fn last_event_ts(&self) -> anyhow::Result<Option<i64>> {
        Ok(self
            .events
            .lock()
            .expect("event store poisoned")
            .iter()
            .map(|event| event.ts)
            .max())
    }

    fn first_event_ts(&self) -> anyhow::Result<Option<i64>> {
        Ok(self
            .events
            .lock()
            .expect("event store poisoned")
            .iter()
            .map(|event| event.ts)
            .min())
    }

    fn recent_sensor_events(&self, limit: usize) -> anyhow::Result<Vec<ConsentEvent>> {
        let stored = self.events.lock().expect("event store poisoned");
        let mut events: Vec<ConsentEvent> = stored
            .iter()
            .filter(|event| crate::SENSOR_CAPABILITIES.contains(&event.capability.as_str()))
            .cloned()
            .collect();
        events.sort_by_key(|event| std::cmp::Reverse(event.ts));
        events.truncate(limit);
        Ok(events)
    }

    fn sensor_event_count(&self) -> anyhow::Result<i64> {
        Ok(self
            .events
            .lock()
            .expect("event store poisoned")
            .iter()
            .filter(|event| crate::SENSOR_CAPABILITIES.contains(&event.capability.as_str()))
            .count() as i64)
    }

    fn location(&self) -> String {
        "in-memory".to_string()
    }
}

/// `%LOCALAPPDATA%\stoolie\stoolie.db`.
pub fn default_path() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("stoolie").join("stoolie.db")
}

/// Open the archive this build uses. With the default `sqlite` feature that is
/// the on-disk database; without it, memory, and the caller is told so through
/// [`Storage::location`].
pub fn open_default() -> anyhow::Result<Box<dyn Storage>> {
    #[cfg(feature = "sqlite")]
    {
        Ok(Box::new(SqliteStore::open(&default_path())?))
    }
    #[cfg(not(feature = "sqlite"))]
    {
        Ok(Box::new(MemoryStore::new()))
    }
}

#[cfg(feature = "sqlite")]
mod sqlite_impl {
    use super::SENSOR_SQL;
    use super::{ConsentEvent, Session, Source, Storage, SyncStats};
    use anyhow::{Context, Result};
    use rusqlite::{params, Connection};
    use std::path::{Path, PathBuf};

    pub struct SqliteStore {
        connection: Connection,
        path: PathBuf,
    }

    impl SqliteStore {
        pub fn open(path: &Path) -> Result<SqliteStore> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            let connection =
                Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
            connection.pragma_update(None, "journal_mode", "WAL")?;
            connection.pragma_update(None, "synchronous", "NORMAL")?;
            let store = SqliteStore {
                connection,
                path: path.to_path_buf(),
            };
            store.migrate()?;
            Ok(store)
        }

        fn migrate(&self) -> Result<()> {
            self.connection.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS sessions (
                    capability TEXT NOT NULL,
                    app        TEXT NOT NULL,
                    app_path   TEXT,
                    start_ts   INTEGER NOT NULL,
                    stop_ts    INTEGER,
                    source     TEXT NOT NULL,
                    first_seen INTEGER NOT NULL,
                    PRIMARY KEY (capability, app, start_ts)
                );
                CREATE INDEX IF NOT EXISTS sessions_by_start ON sessions (start_ts);
                -- Empty strings, never NULL: SQLite treats NULLs as distinct in
                -- a primary key, which would silently duplicate every "all apps"
                -- row.
                CREATE TABLE IF NOT EXISTS consent_events (
                    ts         INTEGER NOT NULL,
                    event_id   INTEGER NOT NULL,
                    capability TEXT NOT NULL,
                    app        TEXT NOT NULL DEFAULT '',
                    old_value  TEXT NOT NULL DEFAULT '',
                    new_value  TEXT NOT NULL DEFAULT '',
                    caller     TEXT NOT NULL DEFAULT '',
                    PRIMARY KEY (ts, capability, app, new_value)
                );
                CREATE INDEX IF NOT EXISTS events_by_ts ON consent_events (ts);
                CREATE TABLE IF NOT EXISTS meta (
                    key   TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                "#,
            )?;
            Ok(())
        }
    }

    impl Storage for SqliteStore {
        /// Inserts are `OR IGNORE`: the first row we saw for a
        /// (capability, app, start) is the row we keep, so re-running never
        /// rewrites history.
        fn sync(&mut self, sessions: &[Session], events: &[ConsentEvent]) -> Result<SyncStats> {
            let now = crate::util::now_ms();
            let mut stats = SyncStats::default();
            let transaction = self.connection.transaction()?;
            {
                let mut insert = transaction.prepare(
                    "INSERT OR IGNORE INTO sessions
                     (capability, app, app_path, start_ts, stop_ts, source, first_seen)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for session in sessions {
                    let changed = insert.execute(params![
                        session.capability,
                        session.app,
                        session.app_path,
                        session.start,
                        session.stop,
                        session.source.label(),
                        now,
                    ])?;
                    stats.sessions_new += changed;
                }
                let mut insert = transaction.prepare(
                    "INSERT OR IGNORE INTO consent_events
                     (ts, event_id, capability, app, old_value, new_value, caller)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for event in events {
                    let changed = insert.execute(params![
                        event.ts,
                        event.event_id,
                        event.capability,
                        event.app.clone().unwrap_or_default(),
                        event.old.clone().unwrap_or_default(),
                        event.new.clone().unwrap_or_default(),
                        event.caller.clone().unwrap_or_default(),
                    ])?;
                    stats.events_new += changed;
                }
            }
            transaction.commit()?;
            Ok(stats)
        }

        fn sessions_since(&self, since_ms: i64) -> Result<Vec<Session>> {
            let mut statement = self.connection.prepare(
                "SELECT capability, app, app_path, start_ts, stop_ts
                 FROM sessions WHERE start_ts >= ?1 ORDER BY start_ts DESC",
            )?;
            let rows = statement.query_map([since_ms], |row| {
                Ok(Session {
                    capability: row.get(0)?,
                    app: row.get(1)?,
                    app_path: row.get(2)?,
                    start: row.get(3)?,
                    stop: row.get(4)?,
                    source: Source::Archive,
                    context: Vec::new(),
                })
            })?;
            Ok(rows.flatten().collect())
        }

        fn session_count(&self) -> Result<i64> {
            Ok(self
                .connection
                .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?)
        }

        fn event_count(&self) -> Result<i64> {
            Ok(self
                .connection
                .query_row("SELECT COUNT(*) FROM consent_events", [], |row| row.get(0))?)
        }

        fn last_event_ts(&self) -> Result<Option<i64>> {
            Ok(self
                .connection
                .query_row("SELECT MAX(ts) FROM consent_events", [], |row| row.get(0))?)
        }

        fn first_event_ts(&self) -> Result<Option<i64>> {
            Ok(self
                .connection
                .query_row("SELECT MIN(ts) FROM consent_events", [], |row| row.get(0))?)
        }

        fn recent_sensor_events(&self, limit: usize) -> Result<Vec<ConsentEvent>> {
            let mut statement = self.connection.prepare(&format!(
                "SELECT ts, event_id, capability, app, old_value, new_value, caller
                 FROM consent_events WHERE capability IN {SENSOR_SQL}
                 ORDER BY ts DESC LIMIT ?1"
            ))?;
            let rows = statement.query_map([limit as i64], |row| {
                Ok(ConsentEvent {
                    ts: row.get(0)?,
                    event_id: row.get(1)?,
                    capability: row.get(2)?,
                    app: Some(row.get::<_, String>(3)?).filter(|value| !value.is_empty()),
                    old: Some(row.get::<_, String>(4)?).filter(|value| !value.is_empty()),
                    new: Some(row.get::<_, String>(5)?).filter(|value| !value.is_empty()),
                    caller: Some(row.get::<_, String>(6)?).filter(|value| !value.is_empty()),
                })
            })?;
            Ok(rows.flatten().collect())
        }

        fn sensor_event_count(&self) -> Result<i64> {
            Ok(self.connection.query_row(
                &format!("SELECT COUNT(*) FROM consent_events WHERE capability IN {SENSOR_SQL}"),
                [],
                |row| row.get(0),
            )?)
        }

        fn location(&self) -> String {
            self.path.display().to_string()
        }
    }

    impl SqliteStore {
        pub fn path(&self) -> &Path {
            &self.path
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(capability: &str, app: &str, start: i64, stop: Option<i64>) -> Session {
        Session {
            capability: capability.to_string(),
            app: app.to_string(),
            app_path: None,
            start,
            stop,
            source: Source::Registry,
            context: Vec::new(),
        }
    }

    fn event(ts: i64) -> ConsentEvent {
        ConsentEvent {
            ts,
            event_id: 1004,
            capability: "microphone".into(),
            app: Some("Photos".into()),
            old: Some("Prompt".into()),
            new: Some("Allow".into()),
            caller: Some("SystemSettings.exe".into()),
        }
    }

    /// The same contract must hold for every implementation, so the assertions
    /// live once and run against each one.
    fn contract(store: &mut dyn Storage) {
        let first = [session(
            "microphone",
            "Discord.exe",
            1_700_000_000_000,
            Some(1_700_000_014_100),
        )];
        assert_eq!(
            store
                .sync(&first, &[event(1_700_000_000_000)])
                .unwrap()
                .sessions_new,
            1
        );
        // Idempotent.
        assert_eq!(
            store
                .sync(&first, &[event(1_700_000_000_000)])
                .unwrap()
                .sessions_new,
            0
        );
        // A later stop for the same start must not rewrite the archive.
        let across_runs = [session(
            "microphone",
            "Discord.exe",
            1_700_000_000_000,
            Some(1_700_000_099_000),
        )];
        store.sync(&across_runs, &[]).unwrap();
        let stored = store.sessions_since(0).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].stop, Some(1_700_000_014_100));
        // Cursor.
        assert_eq!(store.last_event_ts().unwrap(), Some(1_700_000_000_000));
        assert_eq!(store.session_count().unwrap(), 1);
        assert_eq!(store.event_count().unwrap(), 1);
        // Window.
        assert!(store.sessions_since(1_700_000_000_001).unwrap().is_empty());
        assert!(!store.location().is_empty());
    }

    #[test]
    fn memory_store_honours_the_contract() {
        contract(&mut MemoryStore::new());
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_store_honours_the_contract() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SqliteStore::open(&directory.path().join("stoolie.db")).unwrap();
        contract(&mut store);
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_and_memory_agree() {
        let directory = tempfile::tempdir().unwrap();
        let mut sqlite = SqliteStore::open(&directory.path().join("stoolie.db")).unwrap();
        let mut memory = MemoryStore::new();
        let sessions = [session("webcam", "Zoom.exe", 1_700_000_000_000, None)];
        let events = [event(1_700_000_000_000)];
        let a = sqlite.sync(&sessions, &events).unwrap();
        let b = memory.sync(&sessions, &events).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            sqlite.sessions_since(0).unwrap()[0].app,
            memory.sessions_since(0).unwrap()[0].app
        );
    }

    #[test]
    fn the_cursor_advances_only_with_new_events() {
        let mut store = MemoryStore::new();
        assert_eq!(store.last_event_ts().unwrap(), None);
        store.sync(&[], &[event(1_000)]).unwrap();
        assert_eq!(store.last_event_ts().unwrap(), Some(1_000));
        store.sync(&[], &[event(500)]).unwrap();
        assert_eq!(store.last_event_ts().unwrap(), Some(1_000));
    }
}
