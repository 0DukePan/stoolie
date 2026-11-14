//! The archive: stoolie's own append-only record of capability use.
//!
//! Windows keeps one timestamp pair per app and overwrites it. The archive is
//! the answer to that: every run imports whatever the OS still remembers, and
//! nothing already stored is ever rewritten or pruned.

use crate::model::{ConsentEvent, Session, Source};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

pub struct Store {
    connection: Connection,
    path: PathBuf,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SyncStats {
    pub sessions_new: usize,
    pub events_new: usize,
}

impl Store {
    /// `%LOCALAPPDATA%\stoolie\stoolie.db`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join("stoolie").join("stoolie.db")
    }

    pub fn open_default() -> Result<Store> {
        Store::open(&Self::default_path())
    }

    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let connection =
            Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        let store = Store {
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
            -- Empty strings, never NULL: SQLite treats NULLs as distinct in a
            -- primary key, which would silently duplicate every "all apps" row.
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
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Import what the OS remembers. Inserts are `OR IGNORE`: the first row we
    /// saw for a (capability, app, start) is the row we keep, so re-running
    /// never rewrites history.
    pub fn sync(&mut self, sessions: &[Session], events: &[ConsentEvent]) -> Result<SyncStats> {
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

    /// Sessions that started at or after `since`, newest first.
    pub fn sessions_since(&self, since: i64) -> Result<Vec<Session>> {
        let mut statement = self.connection.prepare(
            "SELECT capability, app, app_path, start_ts, stop_ts
             FROM sessions WHERE start_ts >= ?1 ORDER BY start_ts DESC",
        )?;
        let rows = statement.query_map([since], |row| {
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

    pub fn session_count(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?)
    }

    pub fn event_count(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM consent_events", [], |row| row.get(0))?)
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

    #[test]
    fn sync_is_idempotent_and_never_rewrites() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(&directory.path().join("stoolie.db")).unwrap();
        let first = [session(
            "microphone",
            "Discord.exe",
            1_700_000_000,
            Some(1_700_000_014),
        )];
        let stats = store.sync(&first, &[]).unwrap();
        assert_eq!(stats.sessions_new, 1);

        // Same session again: no duplicate.
        let stats = store.sync(&first, &[]).unwrap();
        assert_eq!(stats.sessions_new, 0);

        // A later stop value for the same start must not rewrite the archive.
        let across_runs = [session(
            "microphone",
            "Discord.exe",
            1_700_000_000,
            Some(1_700_000_099),
        )];
        store.sync(&across_runs, &[]).unwrap();
        let stored = store.sessions_since(0).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].stop, Some(1_700_000_014));
    }

    #[test]
    fn consent_events_deduplicate() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(&directory.path().join("stoolie.db")).unwrap();
        let event = ConsentEvent {
            ts: 1_700_000_000,
            event_id: 1004,
            capability: "microphone".into(),
            app: Some("Photos".into()),
            old: Some("Prompt".into()),
            new: Some("Allow".into()),
            caller: Some("SystemSettings.exe".into()),
        };
        assert_eq!(
            store
                .sync(&[], std::slice::from_ref(&event))
                .unwrap()
                .events_new,
            1
        );
        assert_eq!(store.sync(&[], &[event]).unwrap().events_new, 0);
        assert_eq!(store.event_count().unwrap(), 1);
    }
}
