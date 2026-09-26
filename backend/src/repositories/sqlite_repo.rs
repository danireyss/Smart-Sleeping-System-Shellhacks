//! SQLite storage for readings.
//!
//! `received_at` is stored as fixed-width UTC text (`YYYY-MM-DDTHH:MM:SS.mmmZ`) so
//! string order matches time order and the index works for range queries.
//! `flags` is a comma-separated list of `Flag::as_str` names; empty means valid.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use super::{ReadingRepository, RepoError};
use crate::domain::{Flag, Reading};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS readings (
    id           INTEGER PRIMARY KEY,
    received_at  TEXT    NOT NULL,
    eco2_ppm     REAL    NOT NULL,
    tvoc_ppb     REAL    NOT NULL,
    temp_f       REAL,
    humidity_pct REAL,
    uptime_s     INTEGER NOT NULL,
    flags        TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS readings_received_at ON readings (received_at);
";

pub struct SqliteReadingRepository {
    conn: Mutex<Connection>,
}

impl SqliteReadingRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RepoError> {
        let conn = Connection::open(path)?;
        // WAL lets the API read while ingest writes.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
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

impl ReadingRepository for SqliteReadingRepository {
    fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError> {
        let flags = flags.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(",");
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO readings
                (received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, uptime_s, flags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                reading.received_at.format(TIMESTAMP_FORMAT).to_string(),
                reading.eco2_ppm,
                reading.tvoc_ppb,
                reading.temp_f,
                reading.humidity_pct,
                reading.uptime_s as i64,
                flags,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    type Row = (String, f64, f64, Option<f64>, Option<f64>, i64, String);

    fn row(repo: &SqliteReadingRepository, id: i64) -> Row {
        repo.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, uptime_s, flags
                 FROM readings WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .unwrap()
    }

    fn reading() -> Reading {
        Reading {
            received_at: Utc.with_ymd_and_hms(2026, 9, 26, 5, 41, 5).unwrap(),
            eco2_ppm: 432.0,
            tvoc_ppb: 4.0,
            temp_f: Some(77.7),
            humidity_pct: Some(49.0),
            uptime_s: 1300,
        }
    }

    #[test]
    fn stores_all_fields() {
        let repo = SqliteReadingRepository::in_memory().unwrap();
        let id = repo.save(&reading(), &[]).unwrap();
        assert_eq!(
            row(&repo, id),
            (
                "2026-09-26T05:41:05.000Z".to_string(),
                432.0,
                4.0,
                Some(77.7),
                Some(49.0),
                1300,
                String::new(),
            )
        );
    }

    #[test]
    fn stores_missing_values_and_flags() {
        let repo = SqliteReadingRepository::in_memory().unwrap();
        let r = Reading { temp_f: None, humidity_pct: None, uptime_s: 30, ..reading() };
        let id = repo.save(&r, &r.flags()).unwrap();
        let (_, _, _, temp, humidity, _, flags) = row(&repo, id);
        assert_eq!((temp, humidity), (None, None));
        assert_eq!(flags, "warm_up,temp_missing,humidity_missing");
    }

    #[test]
    fn received_at_is_indexed() {
        let repo = SqliteReadingRepository::in_memory().unwrap();
        let plan: String = repo
            .conn
            .lock()
            .unwrap()
            .query_row(
                "EXPLAIN QUERY PLAN SELECT * FROM readings WHERE received_at >= '2026-09-26'",
                [],
                |r| r.get(3),
            )
            .unwrap();
        assert!(plan.contains("readings_received_at"), "plan: {plan}");
    }
}
