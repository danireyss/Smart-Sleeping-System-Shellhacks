//! Storage interfaces. Services depend on these traits, never on SQL.

pub mod sqlite_repo;
pub mod sqlite_session_repo;

use std::error::Error;

use chrono::{DateTime, Utc};

use crate::domain::sleep::{SleepSession, StartOutcome};
use crate::domain::{Flag, Reading};

pub use sqlite_repo::SqliteReadingRepository;
pub use sqlite_session_repo::SqliteSessionRepository;

pub type RepoError = Box<dyn Error + Send + Sync>;

pub trait ReadingRepository: Send + Sync {
    /// Stores a reading with its validation flags. Returns the new row id.
    fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError>;

    /// The most recent reading, if any.
    fn latest(&self) -> Result<Option<Reading>, RepoError>;

    /// Readings with `start <= received_at < end`, oldest first.
    fn range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Reading>, RepoError>;
}

pub trait SessionRepository: Send + Sync {
    /// Opens a session starting at `at`, unless one is already open.
    fn start(&self, at: DateTime<Utc>) -> Result<StartOutcome, RepoError>;

    /// Ends the open session at `at`. `None` if no session is open.
    fn end(&self, at: DateTime<Utc>) -> Result<Option<SleepSession>, RepoError>;

    /// The open session, if any.
    fn current(&self) -> Result<Option<SleepSession>, RepoError>;

    /// The session that ended most recently, if any.
    fn latest_ended(&self) -> Result<Option<SleepSession>, RepoError>;

    /// Ended sessions, most recent first, at most `limit`.
    fn ended(&self, limit: usize) -> Result<Vec<SleepSession>, RepoError>;

    /// A session by id (open or ended).
    fn get(&self, id: i64) -> Result<Option<SleepSession>, RepoError>;
}
