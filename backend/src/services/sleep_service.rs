//! Sleep sessions and nightly reports. Repository calls are blocking, so they
//! run on tokio's blocking pool.

use std::sync::Arc;

use chrono::{SubsecRound, Utc};
use tokio::task::spawn_blocking;

use super::ServiceError;
use crate::domain::sleep::{night_report, NightReport, SleepSession, StartOutcome};
use crate::repositories::{ReadingRepository, SessionRepository};

pub struct SleepService {
    sessions: Arc<dyn SessionRepository>,
    readings: Arc<dyn ReadingRepository>,
}

impl SleepService {
    pub fn new(sessions: Arc<dyn SessionRepository>, readings: Arc<dyn ReadingRepository>) -> Self {
        Self { sessions, readings }
    }

    /// Starts a session now, unless one is already open.
    pub async fn start(&self) -> Result<StartOutcome, ServiceError> {
        let sessions = self.sessions.clone();
        spawn_blocking(move || sessions.start(Utc::now().trunc_subsecs(3))).await?
    }

    /// Ends the open session now. `None` if no session is open.
    pub async fn end(&self) -> Result<Option<SleepSession>, ServiceError> {
        let sessions = self.sessions.clone();
        spawn_blocking(move || sessions.end(Utc::now().trunc_subsecs(3))).await?
    }

    /// The open session, if any.
    pub async fn current(&self) -> Result<Option<SleepSession>, ServiceError> {
        let sessions = self.sessions.clone();
        spawn_blocking(move || sessions.current()).await?
    }

    /// Report for the most recently ended session, if any.
    pub async fn latest_night(&self) -> Result<Option<NightReport>, ServiceError> {
        let (sessions, readings) = (self.sessions.clone(), self.readings.clone());
        spawn_blocking(move || {
            let Some(session) = sessions.latest_ended()? else { return Ok(None) };
            let Some(ended_at) = session.ended_at else { return Ok(None) };
            let rows = readings.range(session.started_at, ended_at)?;
            Ok(Some(night_report(session.id, session.started_at, ended_at, &rows)))
        })
        .await?
    }
}
