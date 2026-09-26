//! SQLite storage for readings.
//!
//! `received_at` is stored as fixed-width UTC text (`YYYY-MM-DDTHH:MM:SS.mmmZ`) so
//! string order matches time order and the index works for range queries.
//! `flags` is a comma-separated list of `Flag::as_str` names; empty means valid.
//! `light_level`, `sound_db`, `sound_peak_db` are NULL when there was no webcam
//! sample; they are added to older databases on open (see `migrate`).

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{ReadingRepository, RepoError};
use crate::domain::ambient::Ambient;
use crate::domain::{Flag, Reading};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

const SELECT_READING: &str = "SELECT received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, \
     uptime_s, light_level, sound_db, sound_peak_db FROM readings";

/// Columns added after the first release, created on open if missing.
const ADDED_COLUMNS: [(&str, &str); 3] =
    [("light_level", "REAL"), ("sound_db", "REAL"), ("sound_peak_db", "REAL")];

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
        migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

/// Adds columns that older databases (like the board's) don't have yet.
fn migrate(conn: &Connection) -> Result<(), RepoError> {
    let mut stmt = conn.prepare("PRAGMA table_info(readings)")?;
    let existing: Vec<String> =
        stmt.query_map([], |row| row.get::<_, String>(1))?.collect::<Result<_, _>>()?;
    for (name, kind) in ADDED_COLUMNS {
        if !existing.iter().any(|c| c == name) {
            conn.execute_batch(&format!("ALTER TABLE readings ADD COLUMN {name} {kind}"))?;
        }
    }
    Ok(())
}

impl ReadingRepository for SqliteReadingRepository {
    fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError> {
        let flags = flags.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(",");
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO readings
                (received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, uptime_s, flags,
                 light_level, sound_db, sound_peak_db)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                timestamp(reading.received_at),
                reading.eco2_ppm,
                reading.tvoc_ppb,
                reading.temp_f,
                reading.humidity_pct,
                reading.uptime_s as i64,
                flags,
                reading.ambient.light_level,
                reading.ambient.sound_db,
                reading.ambient.sound_peak_db,
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

/// A row as read, before the timestamp and uptime are converted.
struct RawRow {
    received_at: String,
    eco2_ppm: f64,
    tvoc_ppb: f64,
    temp_f: Option<f64>,
    humidity_pct: Option<f64>,
    uptime_s: i64,
    ambient: Ambient,
}

fn raw_row(row: &Row) -> rusqlite::Result<RawRow> {
    Ok(RawRow {
        received_at: row.get(0)?,
        eco2_ppm: row.get(1)?,
        tvoc_ppb: row.get(2)?,
        temp_f: row.get(3)?,
        humidity_pct: row.get(4)?,
        uptime_s: row.get(5)?,
        ambient: Ambient { light_level: row.get(6)?, sound_db: row.get(7)?, sound_peak_db: row.get(8)? },
    })
}

fn to_reading(raw: RawRow) -> Result<Reading, RepoError> {
    Ok(Reading {
        received_at: parse_timestamp(&raw.received_at)?,
        eco2_ppm: raw.eco2_ppm,
        tvoc_ppb: raw.tvoc_ppb,
        temp_f: raw.temp_f,
        humidity_pct: raw.humidity_pct,
        uptime_s: u64::try_from(raw.uptime_s)?,
        ambient: raw.ambient,
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
            ambient: Ambient::default(),
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
    fn stores_light_and_sound() {
        let repo = SqliteReadingRepository::in_memory().unwrap();
        let ambient = Ambient { light_level: Some(3.5), sound_db: Some(31.2), sound_peak_db: Some(47.8) };
        let r = Reading { ambient, ..reading() };
        repo.save(&r, &[]).unwrap();
        assert_eq!(repo.latest().unwrap(), Some(r));
    }

    #[test]
    fn migrates_a_database_created_before_light_and_sound() {
        // The original schema, as on the board before this change, with one row.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE readings (
                id INTEGER PRIMARY KEY, received_at TEXT NOT NULL, eco2_ppm REAL NOT NULL,
                tvoc_ppb REAL NOT NULL, temp_f REAL, humidity_pct REAL,
                uptime_s INTEGER NOT NULL, flags TEXT NOT NULL);
             INSERT INTO readings (received_at, eco2_ppm, tvoc_ppb, temp_f, humidity_pct, uptime_s, flags)
             VALUES ('2026-09-26T05:41:05.000Z', 432, 4, 77.7, 49.0, 1300, '');",
        )
        .unwrap();
        let repo = SqliteReadingRepository::init(conn).unwrap();

        // The old row reads back with no light/sound; new rows store them.
        assert_eq!(repo.latest().unwrap(), Some(reading()));
        let later = Reading {
            received_at: reading().received_at + chrono::Duration::minutes(1),
            ambient: Ambient { light_level: Some(2.0), ..Ambient::default() },
            ..reading()
        };
        repo.save(&later, &[]).unwrap();
        assert_eq!(repo.latest().unwrap(), Some(later));

        // Opening again is a no-op.
        let conn = repo.conn.into_inner().unwrap();
        assert!(SqliteReadingRepository::init(conn).is_ok());
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
