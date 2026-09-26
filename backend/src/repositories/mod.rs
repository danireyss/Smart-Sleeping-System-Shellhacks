//! Storage interfaces. Services depend on these traits, never on SQL.

pub mod sqlite_repo;

use std::error::Error;

use crate::domain::{Flag, Reading};

pub use sqlite_repo::SqliteReadingRepository;

pub type RepoError = Box<dyn Error + Send + Sync>;

pub trait ReadingRepository: Send + Sync {
    /// Stores a reading with its validation flags. Returns the new row id.
    fn save(&self, reading: &Reading, flags: &[Flag]) -> Result<i64, RepoError>;
}
