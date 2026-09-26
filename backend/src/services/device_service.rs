//! Answers calls from the device (the MCU sketch driving the touch LCD). Uses the
//! same services as the API, so a session started on the LCD is the same as one
//! started from the web or with curl. Protocol: `domain/device.rs`.

use std::sync::Arc;

use super::{ReadingService, ServiceError, SleepService};
use crate::domain::device::{DeviceReply, DeviceRequest, NO_SCORE};

pub struct DeviceService {
    sleep: Arc<SleepService>,
    readings: Arc<ReadingService>,
}

impl DeviceService {
    pub fn new(sleep: Arc<SleepService>, readings: Arc<ReadingService>) -> Self {
        Self { sleep, readings }
    }

    pub async fn handle(&self, request: DeviceRequest) -> Result<DeviceReply, ServiceError> {
        Ok(match request {
            DeviceRequest::SleepStart => {
                // Starting while a session is open keeps it open: still sleeping.
                self.sleep.start().await?;
                DeviceReply::Bool(true)
            }
            DeviceRequest::SleepEnd => {
                self.sleep.end().await?;
                DeviceReply::Bool(false)
            }
            DeviceRequest::SleepState => DeviceReply::Bool(self.sleep.current().await?.is_some()),
            DeviceRequest::Score => {
                let current = self.readings.current().await?;
                DeviceReply::Float(current.and_then(|r| r.score).map_or(NO_SCORE, |s| s.total))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use tokio::sync::broadcast;

    use super::*;
    use crate::domain::reading::WARM_UP_SECS;
    use crate::domain::{LiveEvent, Reading};
    use crate::repositories::{ReadingRepository, SqliteReadingRepository, SqliteSessionRepository};

    fn service(readings: &[Reading]) -> (DeviceService, broadcast::Receiver<LiveEvent>) {
        let repo = Arc::new(SqliteReadingRepository::in_memory().unwrap());
        for r in readings {
            repo.save(r, &r.flags()).unwrap();
        }
        let sessions = Arc::new(SqliteSessionRepository::in_memory().unwrap());
        let (events, rx) = broadcast::channel(8);
        let sleep = Arc::new(SleepService::new(sessions, repo.clone(), events));
        (DeviceService::new(sleep, Arc::new(ReadingService::new(repo))), rx)
    }

    fn reading(uptime_s: u64) -> Reading {
        Reading {
            received_at: Utc::now() - Duration::seconds(5),
            eco2_ppm: 477.0,
            tvoc_ppb: 11.0,
            temp_f: Some(76.64),
            humidity_pct: Some(49.8),
            uptime_s,
        }
    }

    #[tokio::test]
    async fn start_and_end_sleep_from_the_device() {
        let (device, mut events) = service(&[]);
        assert_eq!(device.handle(DeviceRequest::SleepState).await.unwrap(), DeviceReply::Bool(false));

        assert_eq!(device.handle(DeviceRequest::SleepStart).await.unwrap(), DeviceReply::Bool(true));
        assert_eq!(device.handle(DeviceRequest::SleepState).await.unwrap(), DeviceReply::Bool(true));
        let LiveEvent::Sleep(started) = events.try_recv().unwrap() else { panic!("expected sleep") };
        assert_eq!(started.ended_at, None);

        // A second tap keeps the same session and publishes nothing new.
        assert_eq!(device.handle(DeviceRequest::SleepStart).await.unwrap(), DeviceReply::Bool(true));
        assert!(events.try_recv().is_err());

        assert_eq!(device.handle(DeviceRequest::SleepEnd).await.unwrap(), DeviceReply::Bool(false));
        assert_eq!(device.handle(DeviceRequest::SleepState).await.unwrap(), DeviceReply::Bool(false));
        let LiveEvent::Sleep(ended) = events.try_recv().unwrap() else { panic!("expected sleep") };
        assert_eq!(ended.id, started.id);
        assert!(ended.ended_at.is_some());

        // Ending with nothing open is harmless.
        assert_eq!(device.handle(DeviceRequest::SleepEnd).await.unwrap(), DeviceReply::Bool(false));
    }

    #[tokio::test]
    async fn score_is_the_latest_total_or_minus_one() {
        let (device, _) = service(&[]);
        assert_eq!(device.handle(DeviceRequest::Score).await.unwrap(), DeviceReply::Float(NO_SCORE));

        // 76.64 °F is scored as the shown 76.6: (100 + 34 + 100) / 3 = 78.0
        let (device, _) = service(&[reading(WARM_UP_SECS)]);
        assert_eq!(device.handle(DeviceRequest::Score).await.unwrap(), DeviceReply::Float(78.0));

        // A flagged (warming-up) reading has no score.
        let (device, _) = service(&[reading(10)]);
        assert_eq!(device.handle(DeviceRequest::Score).await.unwrap(), DeviceReply::Float(NO_SCORE));
    }
}
