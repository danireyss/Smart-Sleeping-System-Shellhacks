//! SQLite storage for readings.
//!
//! `received_at` is stored as fixed-width UTC text (`YYYY-MM-DDTHH:MM:SS.mmmZ`) so
//! string order matches time order and the index works for range queries.
//! `flags` is a comma-separated list of `Flag::as_str` names; empty means valid.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{ReadingRepository, RepoError};
use crate::domain::{Flag, Reading};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

const SELECT_READING: &str =
    "SELECT received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, uptime_s FROM readings";

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

impl ReadingRepository for SqliteReadingRepository {
    fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError> {
        let flags = flags.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(",");
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO readings
                (received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, uptime_s, flags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                timestamp(reading.received_at),
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

    fn latest(&self) -> Result<Option<Reading>, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let sql = format!("{SELECT_READING} ORDER BY received_at DESC, id DESC LIMIT 1");
        let row = conn.query_row(&sql, [], raw_row).optional()?;
        row.map(to_reading).transpose()
    }

    fn range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Reading>, RepoError> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let sql = format!(
            "{SELECT_READING} WHERE received_at >= ?1 AND received_at < ?2 ORDER BY received_at, id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![timestamp(start), timestamp(end)], raw_row)?;
        rows.map(|row| to_reading(row?)).collect()
    }
}

/// Opens a file database in WAL mode (the API reads while ingest writes) with a
/// busy timeout (the readings and sessions repositories each hold a connection).
pub(super) fn open_connection(path: impl AsRef<Path>) -> Result<Connection, RepoError> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

/// Fixed-width UTC text, so string order matches time order.
pub(super) fn timestamp(t: DateTime<Utc>) -> String {
    t.format(TIMESTAMP_FORMAT).to_string()
}

pub(super) fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, RepoError> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}

type RawRow = (String, f64, f64, Option<f64>, Option<f64>, i64);

fn raw_row(row: &Row) -> rusqlite::Result<RawRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
}

fn to_reading(
    (received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, uptime_s): RawRow,
) -> Result<Reading, RepoError> {
    Ok(Reading {
        received_at: parse_timestamp(&received_at)?,
        eco2_ppm,
        tvoc_ppb,
        temp_f,
        humidity_pct,
        uptime_s: u64::try_from(uptime_s)?,
    })
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
    fn latest_returns_newest_reading() {
        let repo = SqliteReadingRepository::in_memory().unwrap();
        assert_eq!(repo.latest().unwrap(), None);

        let older = reading();
        let newer = Reading {
            received_at: older.received_at + chrono::Duration::seconds(10),
            temp_f: None,
            ..reading()
        };
        repo.save(&newer, &[]).unwrap();
        repo.save(&older, &[]).unwrap();
        assert_eq!(repo.latest().unwrap(), Some(newer));
    }

    #[test]
    fn range_is_half_open_and_ordered() {
        let repo = SqliteReadingRepository::in_memory().unwrap();
        let t0 = reading().received_at;
        let at = |secs| Reading { received_at: t0 + chrono::Duration::seconds(secs), ..reading() };
        for secs in [20, 0, 10, 30] {
            repo.save(&at(secs), &[]).unwrap();
        }
        let got = repo.range(t0, t0 + chrono::Duration::seconds(30)).unwrap();
        assert_eq!(got, vec![at(0), at(10), at(20)]);
    }

    #[test]
    fn round_trips_millisecond_timestamps() {
        let repo = SqliteReadingRepository::in_memory().unwrap();
        let r = Reading {
            received_at: Utc.with_ymd_and_hms(2026, 9, 26, 6, 7, 53).unwrap()
                + chrono::Duration::milliseconds(673),
            ..reading()
        };
        repo.save(&r, &[]).unwrap();
        assert_eq!(repo.latest().unwrap(), Some(r));
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
