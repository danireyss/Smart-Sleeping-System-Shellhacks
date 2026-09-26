//! Sleep sessions and nightly reports. Repository calls are blocking, so they
//! run on tokio's blocking pool. Every start and end (from the device LCD or the
//! API) is published as a `LiveEvent::Sleep` for GET /api/stream.

use std::sync::Arc;

use chrono::{SubsecRound, Utc};
use tokio::sync::broadcast;
use tokio::task::spawn_blocking;

use super::ServiceError;
use crate::domain::sleep::{night_report, NightReport, NightSummary, SleepSession, StartOutcome};
use crate::domain::LiveEvent;
use crate::repositories::{ReadingRepository, SessionRepository};

pub struct SleepService {
    sessions: Arc<dyn SessionRepository>,
    readings: Arc<dyn ReadingRepository>,
    events: broadcast::Sender<LiveEvent>,
}

impl SleepService {
    pub fn new(
        sessions: Arc<dyn SessionRepository>,
        readings: Arc<dyn ReadingRepository>,
        events: broadcast::Sender<LiveEvent>,
    ) -> Self {
        Self { sessions, readings, events }
    }

    /// Starts a session now, unless one is already open.
    pub async fn start(&self) -> Result<StartOutcome, ServiceError> {
        let sessions = self.sessions.clone();
        let outcome = spawn_blocking(move || sessions.start(Utc::now().trunc_subsecs(3))).await??;
        if let StartOutcome::Started(session) = &outcome {
            self.publish(session);
        }
        Ok(outcome)
    }

    /// Ends the open session now. `None` if no session is open.
    pub async fn end(&self) -> Result<Option<SleepSession>, ServiceError> {
        let sessions = self.sessions.clone();
        let ended = spawn_blocking(move || sessions.end(Utc::now().trunc_subsecs(3))).await??;
        if let Some(session) = &ended {
            self.publish(session);
        }
        Ok(ended)
    }

    fn publish(&self, session: &SleepSession) {
        // An error only means nobody is subscribed right now.
        let _ = self.events.send(LiveEvent::Sleep(session.clone()));
    }

    /// The open session, if any.
    pub async fn current(&self) -> Result<Option<SleepSession>, ServiceError> {
        let sessions = self.sessions.clone();
        spawn_blocking(move || sessions.current()).await?
    }

    /// Report for the most recently ended session, if any.
    pub async fn latest_night(&self) -> Result<Option<NightReport>, ServiceError> {
        let (sessions, readings) = (self.sessions.clone(), self.readings.clone());
        spawn_blocking(move || match sessions.latest_ended()? {
            Some(session) => report_for(&*readings, &session),
            None => Ok(None),
        })
        .await?
    }

    /// Report for one session, or `None` if it doesn't exist or is still open.
    pub async fn night(&self, id: i64) -> Result<Option<NightReport>, ServiceError> {
        let (sessions, readings) = (self.sessions.clone(), self.readings.clone());
        spawn_blocking(move || match sessions.get(id)? {
            Some(session) => report_for(&*readings, &session),
            None => Ok(None),
        })
        .await?
    }

    /// Summaries of ended sessions, most recent first (for the history calendar).
    pub async fn nights(&self) -> Result<Vec<NightSummary>, ServiceError> {
        let (sessions, readings) = (self.sessions.clone(), self.readings.clone());
        spawn_blocking(move || {
            let mut nights = Vec::new();
            for session in sessions.ended(MAX_NIGHTS)? {
                if let Some(report) = report_for(&*readings, &session)? {
                    nights.push(NightSummary::from(&report));
                }
            }
            Ok(nights)
        })
        .await?
    }
}

/// Ended sessions listed in the history (a year of nights).
const MAX_NIGHTS: usize = 366;

/// The report for an ended session (`None` while it is still open).
fn report_for(
    readings: &dyn ReadingRepository,
    session: &SleepSession,
) -> Result<Option<NightReport>, ServiceError> {
    let Some(ended_at) = session.ended_at else { return Ok(None) };
    let rows = readings.range(session.started_at, ended_at)?;
    Ok(Some(night_report(session.id, session.started_at, ended_at, &rows)))
}
