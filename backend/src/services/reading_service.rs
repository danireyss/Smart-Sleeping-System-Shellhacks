//! Read side for the API and (later) the agent tools. Repository calls are
//! blocking, so they run on tokio's blocking pool.

use std::error::Error;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::task::spawn_blocking;

use crate::domain::summary::{summarize, Summary};
use crate::domain::ScoredReading;
use crate::repositories::ReadingRepository;

pub type ServiceError = Box<dyn Error + Send + Sync>;

pub struct ReadingService {
    repo: Arc<dyn ReadingRepository>,
}

impl ReadingService {
    pub fn new(repo: Arc<dyn ReadingRepository>) -> Self {
        Self { repo }
    }

    /// The latest reading with its flags and score, or `None` if there are no readings.
    pub async fn current(&self) -> Result<Option<ScoredReading>, ServiceError> {
        let repo = self.repo.clone();
        let latest = spawn_blocking(move || repo.latest()).await??;
        Ok(latest.map(ScoredReading::from))
    }

    /// Readings with `start <= received_at < end`, oldest first, with flags and scores.
    pub async fn readings(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ScoredReading>, ServiceError> {
        let repo = self.repo.clone();
        let readings = spawn_blocking(move || repo.range(start, end)).await??;
        Ok(readings.into_iter().map(ScoredReading::from).collect())
    }

    /// Statistics for readings with `start <= received_at < end`.
    pub async fn summary(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Summary, ServiceError> {
        let repo = self.repo.clone();
        let readings = spawn_blocking(move || repo.range(start, end)).await??;
        Ok(summarize(start, end, &readings))
    }
}
