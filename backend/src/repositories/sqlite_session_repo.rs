//! SQLite storage for sleep sessions. Timestamps use the same fixed-width UTC
//! text as readings. A partial unique index allows at most one open session.

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::sqlite_repo::{open_connection, parse_timestamp, timestamp};
use super::{RepoError, SessionRepository};
use crate::domain::sleep::{SleepSession, StartOutcome};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sleep_sessions (
    id         INTEGER PRIMARY KEY,
    started_at TEXT NOT NULL,
    ended_at   TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS sleep_sessions_one_open
    ON sleep_sessions ((ended_at IS NULL)) WHERE ended_at IS NULL;
CREATE INDEX IF NOT EXISTS sleep_sessions_ended_at ON sleep_sessions (ended_at);
";

const SELECT_SESSION: &str = "SELECT id, started_at, ended_at FROM sleep_sessions";

pub struct SqliteSessionRepository {
    conn: Mutex<Connection>,
}

impl SqliteSessionRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RepoError> {
        Self::init(open_connection(path)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self, RepoError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, RepoError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

impl SessionRepository for SqliteSessionRepository {
    fn start(&self, at: DateTime<Utc>) -> Result<StartOutcome, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        if let Some(open) = open_session(&conn)? {
            return Ok(StartOutcome::AlreadyOpen(open));
        }
        conn.execute("INSERT INTO sleep_sessions (started_at) VALUES (?1)", [timestamp(at)])?;
        Ok(StartOutcome::Started(SleepSession {
            id: conn.last_insert_rowid(),
            started_at: at,
            ended_at: None,
        }))
    }

    fn end(&self, at: DateTime<Utc>) -> Result<Option<SleepSession>, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let Some(open) = open_session(&conn)? else { return Ok(None) };
        // Never end before the start, even if the clock moved backwards.
        let ended_at = at.max(open.started_at);
        conn.execute(
            "UPDATE sleep_sessions SET ended_at = ?1 WHERE id = ?2",
            params![timestamp(ended_at), open.id],
        )?;
        Ok(Some(SleepSession { ended_at: Some(ended_at), ..open }))
    }

    fn current(&self) -> Result<Option<SleepSession>, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        open_session(&conn)
    }

    fn latest_ended(&self) -> Result<Option<SleepSession>, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let sql = format!(
            "{SELECT_SESSION} WHERE ended_at IS NOT NULL ORDER BY ended_at DESC, id DESC LIMIT 1"
        );
        conn.query_row(&sql, [], raw_row).optional()?.map(to_session).transpose()
    }

    fn ended(&self, limit: usize) -> Result<Vec<SleepSession>, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let sql = format!(
            "{SELECT_SESSION} WHERE ended_at IS NOT NULL ORDER BY ended_at DESC, id DESC LIMIT ?1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([limit as i64], raw_row)?;
        rows.map(|row| to_session(row?)).collect()
    }

    fn get(&self, id: i64) -> Result<Option<SleepSession>, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let sql = format!("{SELECT_SESSION} WHERE id = ?1");
        conn.query_row(&sql, [id], raw_row).optional()?.map(to_session).transpose()
    }
}

fn open_session(conn: &Connection) -> Result<Option<SleepSession>, RepoError> {
    let sql = format!("{SELECT_SESSION} WHERE ended_at IS NULL");
    conn.query_row(&sql, [], raw_row).optional()?.map(to_session).transpose()
}

type RawRow = (i64, String, Option<String>);

fn raw_row(row: &Row) -> rusqlite::Result<RawRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn to_session((id, started_at, ended_at): RawRow) -> Result<SleepSession, RepoError> {
    Ok(SleepSession {
        id,
        started_at: parse_timestamp(&started_at)?,
        ended_at: ended_at.as_deref().map(parse_timestamp).transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    fn t(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, hour, 0, 0).unwrap()
    }

    fn started(outcome: StartOutcome) -> SleepSession {
        match outcome {
            StartOutcome::Started(s) => s,
            other => panic!("expected Started, got {other:?}"),
        }
    }

    #[test]
    fn start_then_end() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        assert_eq!(repo.current().unwrap(), None);

        let s = started(repo.start(t(2)).unwrap());
        assert_eq!(s.ended_at, None);
        assert_eq!(repo.current().unwrap(), Some(s.clone()));
        assert_eq!(repo.latest_ended().unwrap(), None);

        let ended = repo.end(t(9)).unwrap().unwrap();
        assert_eq!(ended, SleepSession { ended_at: Some(t(9)), ..s });
        assert_eq!(repo.current().unwrap(), None);
        assert_eq!(repo.latest_ended().unwrap(), Some(ended));
    }

    #[test]
    fn only_one_open_session() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        let first = started(repo.start(t(2)).unwrap());
        assert_eq!(repo.start(t(3)).unwrap(), StartOutcome::AlreadyOpen(first));
    }

    #[test]
    fn database_rejects_a_second_open_session() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        repo.start(t(2)).unwrap();
        let conn = repo.conn.lock().unwrap();
        let second = conn.execute(
            "INSERT INTO sleep_sessions (started_at) VALUES (?1)",
            [timestamp(t(3))],
        );
        assert!(second.is_err());
    }

    #[test]
    fn end_without_open_session_is_none() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        assert_eq!(repo.end(t(9)).unwrap(), None);
    }

    #[test]
    fn end_never_precedes_start() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        repo.start(t(9)).unwrap();
        let ended = repo.end(t(8)).unwrap().unwrap();
        assert_eq!(ended.ended_at, Some(t(9)));
    }

    #[test]
    fn latest_ended_is_most_recent() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        repo.start(t(1)).unwrap();
        repo.end(t(2)).unwrap();
        repo.start(t(3)).unwrap();
        let second = repo.end(t(4) + Duration::milliseconds(250)).unwrap().unwrap();
        repo.start(t(5)).unwrap(); // open sessions are not "ended"
        assert_eq!(repo.latest_ended().unwrap(), Some(second));
    }

    #[test]
    fn ended_lists_newest_first_and_skips_the_open_session() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        let mut ended = Vec::new();
        for h in [1, 3, 5] {
            repo.start(t(h)).unwrap();
            ended.push(repo.end(t(h + 1)).unwrap().unwrap());
        }
        repo.start(t(7)).unwrap();
        ended.reverse();
        assert_eq!(repo.ended(10).unwrap(), ended);
        assert_eq!(repo.ended(2).unwrap(), ended[..2]);
    }

    #[test]
    fn get_by_id() {
        let repo = SqliteSessionRepository::in_memory().unwrap();
        repo.start(t(1)).unwrap();
        let s = repo.end(t(2)).unwrap().unwrap();
        assert_eq!(repo.get(s.id).unwrap(), Some(s));
        assert_eq!(repo.get(999).unwrap(), None);
    }
}
