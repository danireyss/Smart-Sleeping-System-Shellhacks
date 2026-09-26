//! Storage interfaces. Services depend on these traits, never on SQL.

pub mod sqlite_repo;

use std::error::Error;

use chrono::{DateTime, Utc};

use crate::domain::{Flag, Reading};

pub use sqlite_repo::SqliteReadingRepository;

pub type RepoError = Box<dyn Error + Send + Sync>;

pub trait ReadingRepository: Send + Sync {
    /// Stores a reading with its validation flags. Returns the new row id.
    fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError>;

    /// The most recent reading, if any.
    fn latest(&self) -> Result<Option<Reading>, RepoError>;

    /// Readings with `start <= received_at < end`, oldest first.
    fn range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Reading>, RepoError>;
}
